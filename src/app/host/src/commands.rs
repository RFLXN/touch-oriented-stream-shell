//! 호스트 명령 등록 및 실행 관리(덱의 버튼·스위치·노브 위젯용).
//!
//! 덱은 실행 커맨드를 직접 보내지 않고, 호스트에 미리 등록된 항목 중 ID만 지정한다.
//! 따라서 덱이 타인에게 넘어가더라도 등록 목록에 없는 명령은 실행되지 않는다.
//! 명령 관리 권한은 호스트(`toss-connector command ...`)에만 있으며, 덱별로 command.run 권한도 허용해야 한다(trust 모듈, 기본값 false).
//!
//! 명령 종류 (kind, 덱에서 대응하는 위젯)
//!   run      단발성 커맨드 실행 (상태 없음)                                   → 버튼
//!   switch   켜기(on)·끄기(off) 커맨드                                        → 스위치 버튼
//!   stepped  여러 단계(예: off / passive / active / auto), 단계별 커맨드      → 단계 노브
//!   linear   범위 내 수치(예: 0~100), 값 설정 커맨드 1개                      → 연속 노브
//!
//! 상태 추적 (switch, stepped, linear): 현재 값은 호스트가 관리한다.
//! 등록 시 상태 확인 커맨드(state)를 지정하면 Watcher가 every 초마다 실행해 값이 바뀔 때만 덱에 푸시한다.
//!   switch   출력이 on/1/true/yes면 켜짐, off/0/false/no면 꺼짐. 그 외 출력은 종료 코드 0이면 켜짐
//!            (예: `pgrep -x obs`, `systemctl --user is-active foo`)
//!   stepped  출력이 단계 이름(대소문자 무시) 또는 0-based 인덱스
//!   linear   출력이 숫자(`%` 접미사 허용)
//! state가 비어 있으면 마지막으로 설정한 값만 메모리에서 기억한다(toss-connector 재시작 시 초기화).
//!
//! 값 설정 커맨드에는 두 가지 방식으로 값이 전달된다: 커맨드 내 `{}` 치환 및 환경 변수 TOSS_VALUE.
//! 주입되는 값은 덱이 보낸 문자열이 아니다. 덱은 숫자만 보내고, 호스트에서 그 숫자를 범위와 단위에 맞춰 정규화한 뒤
//! linear는 수치, stepped는 등록된 단계 이름, switch는 on/off를 주입한다.
//!
//! 설정 디렉터리의 commands.json에 저장하며 호스트의 모든 덱이 같은 목록을 공유한다.
//! Discord 동작 (discord): 셸 커맨드 대신 `discord`에 Discord 동작 ID(`discord::ACTIONS`, 예: `mute`, `deaf`)를 지정할 수 있다.
//! 이때 종류는 그 동작이 정하고, 값 설정은 Discord 앱의 로컬 RPC로 보내며, 상태는 Discord가 보내는 변경 이벤트에서 읽는다
//! (상태 확인 커맨드와 주기는 쓰지 않는다). 연결과 토큰은 `discord::live`가 맡는다(토큰은 덱 금고에서 빌린다).
//!
//! 실행은 `sh -c`(윈도우는 `cmd /C`)에 위임한다. run은 프로세스만 띄우고 종료를 대기하지 않으며(GUI 앱 실행이 많음),
//! 값 설정 커맨드는 순서 역전을 막기 위해 잠깐 종료를 기다린다.
//! 상태 확인 커맨드는 별도 프로세스 그룹으로 실행해 타임아웃 시 하위 프로세스까지 일괄 종료한다.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use toss_proto::{CommandInfo, KIND_LINEAR, KIND_RUN, KIND_STEPPED, KIND_SWITCH};

use crate::{discord, jsonfile};

/// 이름과 커맨드 최대 길이
const NAME_MAX: usize = 40;
const RUN_MAX: usize = 2000;
/// ID 최대 길이
const ID_MAX: usize = 64;
/// 최대 등록 가능 개수
const COUNT_MAX: usize = 200;
/// 단계 개수 및 단계 이름 최대 길이
const STEPS_MAX: usize = 12;
const STEP_NAME_MAX: usize = 16;
/// 범위를 단위로 나눈 칸(틱)의 최대 수
const TICKS_MAX: f64 = 10_000.0;
/// 상태 확인 주기(초): 기본값 및 허용 범위
const EVERY_DEFAULT: f64 = 2.0;
const EVERY_MIN: f64 = 0.5;
const EVERY_MAX: f64 = 3600.0;
/// 상태 확인 커맨드 대기 타임아웃. 초과 시 프로세스 종료 후 상태를 None(알 수 없음)으로 처리
const STATE_WAIT: Duration = Duration::from_secs(3);
/// 값 설정 커맨드 대기 타임아웃. 초과 시 백그라운드로 넘기고 다음 작업 진행
const SET_WAIT: Duration = Duration::from_secs(5);
/// Discord 동작 명령의 값을 다시 읽는 주기(이벤트로 갱신된 메모리 값을 읽으므로 짧다)
const DISCORD_EVERY: Duration = Duration::from_millis(100);
/// 값 설정 후 상태 반영이 지연되는 프로그램을 위해 상태를 한 번 더 재조회하기까지의 지연 시간
const RECHECK: Duration = Duration::from_millis(400);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Step {
    pub name: String,
    /// 이 단계 선택 시 실행할 커맨드. 비어 있으면 Command.set을 사용({}에 단계 이름 주입)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Command {
    /// 등록 시 부여되는 ID (16진수 8자). 이름을 변경해도 유지됨
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// 종류 (toss-proto KIND_*). 생략 시 run
    #[serde(default = "kind_run")]
    pub kind: String,
    /// run: 실행할 셸 커맨드
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
    /// switch: 켜기/끄기 커맨드
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub off: String,
    /// stepped: 단계 목록 (순서대로)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    /// linear 및 개별 커맨드가 없는 단계에서 사용할 값 설정 커맨드 ({} 자리에 값 주입)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub set: String,
    /// linear: 최솟값, 최댓값, 단위
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub step: f64,
    /// 현재 상태를 조회하는 커맨드 (비우면 마지막 설정값만 기억)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub state: String,
    /// 상태 확인 주기 (초). 0이면 기본값 사용
    #[serde(default, skip_serializing_if = "is_zero")]
    pub every: f64,
    /// 셸 커맨드 대신 실행할 Discord 동작 ID(`discord::ACTIONS`)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub discord: String,
}

pub(crate) fn kind_run() -> String {
    KIND_RUN.to_string()
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl Command {
    /// 단발성 실행 명령 생성
    pub fn run(name: &str, run: &str) -> Self {
        Self { name: name.into(), kind: KIND_RUN.into(), run: run.into(), ..Self::default() }
    }

    /// 스위치 명령 생성(켜기, 끄기, 상태 확인 커맨드; 미사용 시 빈 문자열)
    pub fn switch(name: &str, on: &str, off: &str, state: &str) -> Self {
        Self { name: name.into(), kind: KIND_SWITCH.into(), on: on.into(), off: off.into(), state: state.into(), ..Self::default() }
    }

    /// 단계 명령 생성: (단계 이름, 개별 커맨드 — 비어 있으면 set 사용) 목록
    pub fn stepped(name: &str, steps: &[(&str, &str)], set: &str, state: &str) -> Self {
        let steps = steps.iter().map(|(n, r)| Step { name: n.to_string(), run: r.to_string() }).collect();
        Self { name: name.into(), kind: KIND_STEPPED.into(), steps, set: set.into(), state: state.into(), ..Self::default() }
    }

    /// 범위 명령 생성
    pub fn linear(name: &str, (min, max, step): (f64, f64, f64), set: &str, state: &str) -> Self {
        Self { name: name.into(), kind: KIND_LINEAR.into(), min, max, step, set: set.into(), state: state.into(), ..Self::default() }
    }

    /// Discord 동작 명령 생성(종류는 동작이 정한다)
    pub fn discord(name: &str, action: &str) -> Self {
        Self { name: name.into(), kind: discord::action(action).map_or(KIND_RUN, |a| a.kind).into(), discord: action.into(), ..Self::default() }
    }

    /// 상태 값을 가지는 명령인지 여부
    pub fn has_value(&self) -> bool {
        self.kind != KIND_RUN
    }

    /// 지정한 Discord 동작(셸 커맨드 명령이면 `None`)
    pub fn action(&self) -> Option<&'static discord::Action> {
        (!self.discord.is_empty()).then(|| discord::action(&self.discord)).flatten()
    }

    /// 현재 값을 알아낼 수 있는지 여부(상태 확인 커맨드나 Discord 이벤트)
    pub fn watched(&self) -> bool {
        self.has_value() && (!self.state.is_empty() || self.action().is_some())
    }

    /// 상태 확인 주기 반환(미지정 시 기본값, 범위 초과 시 클램프)
    fn every(&self) -> Duration {
        if self.action().is_some() {
            return DISCORD_EVERY;
        }
        Duration::from_secs_f64(if self.every > 0.0 { self.every.clamp(EVERY_MIN, EVERY_MAX) } else { EVERY_DEFAULT })
    }

    /// 덱에 전달할 메타데이터 생성 (실행 커맨드 제외)
    pub fn info(&self) -> CommandInfo {
        CommandInfo {
            kind: self.kind.clone(),
            steps: self.steps.iter().map(|s| s.name.clone()).collect(),
            min: self.min,
            max: self.max,
            step: if self.kind == KIND_LINEAR { self.step } else { 0.0 },
            ..CommandInfo::run(&self.id, &self.name)
        }
    }

    /// 등록 가능한 형식인지 검증하고, 공백 제거 및 미사용 필드 정리 후 반환
    pub(crate) fn checked(&self) -> Result<Command, String> {
        let line = |what: &str, s: &str, needed: bool| -> Result<String, String> {
            let s = s.trim();
            if s.len() > RUN_MAX {
                return Err(format!("{what} 커맨드가 너무 깁니다"));
            }
            if needed && s.is_empty() {
                return Err(format!("{what} 커맨드가 비어 있습니다"));
            }
            Ok(s.to_string())
        };
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX || name.chars().any(char::is_control) {
            return Err(format!("이름은 1~{NAME_MAX}자여야 합니다"));
        }
        let mut c = Command { id: self.id.clone(), name: name.to_string(), kind: self.kind.clone(), ..Command::default() };
        if !self.discord.is_empty() {
            let a = discord::action(&self.discord).ok_or_else(|| format!("알 수 없는 Discord 동작: {} (사용 가능: {})", self.discord, discord::ACTIONS.iter().map(|a| a.id).collect::<Vec<_>>().join(", ")))?;
            (c.kind, c.discord) = (a.kind.to_string(), a.id.to_string());
            return Ok(c);
        }
        match self.kind.as_str() {
            KIND_RUN => {
                c.run = line("실행", &self.run, true)?;
                return Ok(c);
            }
            KIND_SWITCH => {
                c.on = line("켜기", &self.on, true)?;
                c.off = line("끄기", &self.off, true)?;
            }
            KIND_STEPPED => {
                c.set = line("값 설정", &self.set, false)?;
                if self.steps.len() < 2 || self.steps.len() > STEPS_MAX {
                    return Err(format!("단계는 2~{STEPS_MAX}개여야 합니다"));
                }
                for s in &self.steps {
                    let step = Step { name: s.name.trim().to_string(), run: line("단계", &s.run, false)? };
                    if step.name.is_empty() || step.name.chars().count() > STEP_NAME_MAX || step.name.chars().any(char::is_control) {
                        return Err(format!("단계 이름은 1~{STEP_NAME_MAX}자여야 합니다: {}", s.name));
                    }
                    if c.steps.iter().any(|o| o.name.eq_ignore_ascii_case(&step.name)) {
                        return Err(format!("중복된 단계 이름입니다: {}", step.name));
                    }
                    if step.run.is_empty() {
                        if c.set.is_empty() {
                            return Err(format!("단계 '{}'의 커맨드가 없습니다 (개별 커맨드 또는 공통 커맨드를 지정하세요)", step.name));
                        }
                        if !step.name.chars().all(|ch| ch.is_ascii_alphanumeric() || "._+-".contains(ch)) {
                            return Err(format!("공통 커맨드의 {{}}에 들어갈 단계 이름은 영문자·숫자·._+-만 사용할 수 있습니다: {}", step.name));
                        }
                    }
                    c.steps.push(step);
                }
            }
            KIND_LINEAR => {
                c.set = line("값 설정", &self.set, true)?;
                let (min, max, step) = (self.min, self.max, if self.step == 0.0 { 1.0 } else { self.step });
                if !(min.is_finite() && max.is_finite() && step.is_finite()) || min >= max || step <= 0.0 {
                    return Err("범위 설정이 올바르지 않습니다 (min < max, step > 0)".into());
                }
                if (max - min) / step > TICKS_MAX {
                    return Err("단위가 너무 세밀합니다 (최대 10000단계까지 허용)".into());
                }
                (c.min, c.max, c.step) = (min, max, step);
            }
            other => return Err(format!("알 수 없는 종류: {other}")),
        }
        c.state = line("상태 확인", &self.state, false)?;
        if self.every != 0.0 {
            if !(EVERY_MIN..=EVERY_MAX).contains(&self.every) {
                return Err(format!("상태 확인 주기는 {EVERY_MIN}~{EVERY_MAX}초여야 합니다"));
            }
            if c.state.is_empty() {
                return Err("상태 확인 주기는 상태 확인 커맨드가 있을 때만 지정할 수 있습니다".into());
            }
            c.every = self.every;
        }
        Ok(c)
    }

    /// 덱에서 전달된 값을 명령 스펙에 맞게 검증·정규화한다(스위치 0/1, 단계 인덱스, 범위 단위 정렬).
    /// 맞출 수 없는 값(숫자가 아님, 없는 단계, 값이 없는 종류)은 거부한다.
    pub fn normalize(&self, value: f64) -> Result<f64, String> {
        if !value.is_finite() {
            return Err("유효한 숫자가 아닙니다".into());
        }
        match self.kind.as_str() {
            KIND_SWITCH if value == 0.0 || value == 1.0 => Ok(value),
            KIND_SWITCH => Err("스위치 값은 0 또는 1이어야 합니다".into()),
            KIND_STEPPED if value.fract() == 0.0 && value >= 0.0 && (value as usize) < self.steps.len() => Ok(value),
            KIND_STEPPED => Err("존재하지 않는 단계 인덱스입니다".into()),
            KIND_LINEAR if !(self.min < self.max && self.step > 0.0) => Err("범위 설정이 올바르지 않은 명령입니다".into()),
            KIND_LINEAR => {
                let ticks = ((value.clamp(self.min, self.max) - self.min) / self.step).round();
                Ok(((self.min + ticks * self.step).min(self.max) * 1e6).round() / 1e6)
            }
            _ => Err("값을 설정할 수 없는 명령 종류입니다".into()),
        }
    }

    /// 정규화된 value를 적용할 때 실행할 커맨드와 TOSS_VALUE 문자열 반환
    fn set_line(&self, value: f64) -> Option<(String, String)> {
        match self.kind.as_str() {
            KIND_SWITCH => Some(if value != 0.0 { (self.on.clone(), "on".into()) } else { (self.off.clone(), "off".into()) }),
            KIND_STEPPED => {
                let s = self.steps.get(value as usize)?;
                Some(if s.run.is_empty() { (self.set.replace("{}", &s.name), s.name.clone()) } else { (s.run.clone(), s.name.clone()) })
            }
            KIND_LINEAR => {
                let text = show_number(value);
                Some((self.set.replace("{}", &text), text))
            }
            _ => None,
        }
    }

    /// 값 설정 커맨드 실행 및 완료 대기. 실행하지 못했거나 커맨드가 실패로 끝나면 그 사유를 반환한다.
    /// 타임아웃 내 미종료 시 백그라운드 실행으로 두고 성공 처리(GUI 실행 등 고려).
    pub fn set_value(&self, value: f64) -> Result<(), String> {
        if let Some(a) = self.action() {
            return a.write(value);
        }
        let (line, text) = self.set_line(value).ok_or("값을 설정할 수 없는 명령 종류입니다")?;
        run_wait(&line, &text, SET_WAIT)
    }

    /// 상태 확인 커맨드를 실행해 현재 값을 파싱한다. 실패 시 None
    pub fn read_value(&self) -> Option<f64> {
        if let Some(a) = self.action() {
            return a.read();
        }
        if self.state.is_empty() || !self.has_value() {
            return None;
        }
        let (ok, out) = output(&self.state, STATE_WAIT).ok()?;
        let text = out.trim();
        match self.kind.as_str() {
            KIND_SWITCH => Some(f64::from(on_off(text).unwrap_or(ok))),
            KIND_STEPPED => self
                .steps
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(text))
                .or_else(|| text.parse::<usize>().ok().filter(|i| *i < self.steps.len()))
                .map(|i| i as f64),
            KIND_LINEAR => {
                let first = text.split_whitespace().next()?.trim_end_matches('%');
                let v = first.parse::<f64>().ok().filter(|v| v.is_finite())?;
                Some(v.max(self.min).min(self.max))
            }
            _ => None,
        }
    }

    /// 표시용 문자열 변환 (스위치 on/off, 단계 이름, 숫자)
    pub fn show_value(&self, value: Option<f64>) -> String {
        match (self.kind.as_str(), value) {
            (_, None) => "알 수 없음".into(),
            (KIND_SWITCH, Some(v)) => if v != 0.0 { "on" } else { "off" }.into(),
            (KIND_STEPPED, Some(v)) => self.steps.get(v as usize).map(|s| s.name.clone()).unwrap_or_default(),
            (_, Some(v)) => show_number(v),
        }
    }

    /// 사용자 입력 문자열(on/off, 단계 이름/번호, 숫자)을 정규화된 값으로 파싱 (`toss-connector command set`)
    pub fn parse_value(&self, text: &str) -> Result<f64, String> {
        let raw = match self.kind.as_str() {
            KIND_SWITCH => f64::from(on_off(text).ok_or("on 또는 off를 입력하세요")?),
            KIND_STEPPED => self
                .steps
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(text))
                .map(|i| i as f64)
                .or_else(|| text.parse::<f64>().ok())
                .ok_or_else(|| format!("단계 이름을 입력하세요: {}", self.steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))?,
            _ => text.trim_end_matches('%').parse::<f64>().map_err(|_| "숫자를 입력하세요".to_string())?,
        };
        self.normalize(raw)
    }
}

/// 숫자를 커맨드 주입용 문자열로 포맷(정수는 소수점 생략, 소수는 뒤쪽 0 제거)
fn show_number(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.6}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// on/off 키워드 파싱 (대소문자 무관)
fn on_off(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// ID 유효성 검사: 영문자·숫자·'-'·'_' 조합 1~ID_MAX자 (프로토콜 및 콘솔 URL 경로에 사용)
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= ID_MAX && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// 명령 관리·실행 실패 사유 (웹 콘솔에서 HTTP 상태 코드로 매핑)
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// 시스템 설정(strict)으로 잠겨 있어 추가·수정·삭제 불가
    Locked,
    /// 시스템 관리 설정에서 고정된 명령이라 수정·삭제 불가
    Managed,
    /// 존재하지 않는 명령
    NotFound,
    /// 잘못된 형식/인자 (사유)
    Invalid(String),
    /// 파일 I/O 또는 실행 오류 (사유)
    Io(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::Locked => "명령 목록이 시스템 설정(Nix)으로 잠겨 있습니다. Nix 설정에서 변경하세요",
            Error::Managed => "시스템 설정(Nix)으로 고정된 명령입니다. Nix 설정에서 변경하세요",
            Error::NotFound => "등록되지 않은 명령입니다",
            Error::Invalid(why) | Error::Io(why) => why,
        })
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

/// 명령 목록 변경 감지용 스탬프 (사용자 파일 스탬프, 시스템 관리 설정 스탬프)
pub type Stamp = (jsonfile::Stamp, crate::managed::Stamp);

/// 등록된 명령 저장소. 다음 두 소스를 합쳐서 제공한다:
///   - 시스템 관리 설정 (managed 모듈, NixOS 모듈이 생성하는 읽기 전용 파일; 수정·삭제 불가)
///   - 사용자 설정 (설정 디렉터리의 commands.json; strict 모드에서는 무시되며 추가 등록 불가)
///
/// ID나 이름이 중복되면 시스템 관리 설정이 우선한다.
/// commands.json은 사용자가 직접 편집할 수 있으므로 읽을 때마다 항목별로 검증한다(Command::checked, valid_id).
/// 유효하지 않은 항목은 무시하고 경고를 1회 출력하되 파일에서 삭제하지는 않는다.
#[derive(Clone)]
pub struct Commands {
    path: PathBuf,
    managed: Option<PathBuf>,
}

impl Commands {
    /// 사용자 파일만 사용하는 저장소 생성 (테스트용. 실제 실행에서는 Store::commands 사용)
    #[cfg(test)]
    pub fn new(dir: &Path) -> Self {
        Self::with(dir, None)
    }

    /// 시스템 관리 설정 경로 지정 (None이면 미사용)
    pub fn with(dir: &Path, managed: Option<PathBuf>) -> Self {
        Self { path: dir.join("commands.json"), managed }
    }

    fn managed(&self) -> Option<crate::managed::Managed> {
        crate::managed::Managed::read(self.managed.as_deref())
    }

    /// 시스템 설정(strict)으로 인해 사용자 추가·수정이 잠겨 있는지 여부
    pub fn locked(&self) -> bool {
        self.managed().is_some_and(|m| m.strict)
    }

    /// 시스템 관리 설정에 정의된 명령 ID 목록 (수정·삭제 불가)
    pub fn managed_ids(&self) -> Vec<String> {
        self.managed().map(|m| m.commands.into_iter().map(|c| c.id).collect()).unwrap_or_default()
    }

    /// 사용자 파일에서 유효한 명령만 검증·정규화해 로드한다. 잘못된 항목은 제외.
    fn user_list(&self) -> Vec<Command> {
        let (raw, mut trouble) = match jsonfile::load::<Vec<Command>>(&self.path) {
            Ok(list) => (list.unwrap_or_default(), None),
            Err(e) => (Vec::new(), Some(format!("{}을(를) 읽을 수 없습니다: {e}. 복구될 때까지 사용자 등록 명령이 없는 것으로 처리합니다", self.path.display()))),
        };
        let (mut out, mut bad) = (Vec::new(), Vec::new());
        for c in raw {
            match c.checked() {
                Ok(ok) if valid_id(&ok.id) => out.push(ok),
                Ok(_) => bad.push(format!("{} (ID 누락 또는 형식 오류)", c.name)),
                Err(e) => bad.push(format!("{} ({e})", c.name)),
            }
        }
        if !bad.is_empty() {
            trouble = Some(format!("{}의 유효하지 않은 명령 {}개를 무시합니다: {}", self.path.display(), bad.len(), bad.join(", ")));
        }
        jsonfile::complain(&self.path, trouble.as_deref());
        out
    }

    /// 사용 가능한 전체 명령 목록 반환(시스템 관리 명령 우선, strict면 관리 명령만)
    pub fn list(&self) -> Vec<Command> {
        let Some(m) = self.managed() else { return self.user_list() };
        let mut out = m.commands.clone();
        if !m.strict {
            out.extend(self.user_list().into_iter().filter(|c| !m.commands.iter().any(|k| k.id == c.id || k.name == c.name)));
        }
        out
    }

    /// 변경 감지용 스탬프 반환
    pub fn stamp(&self) -> Stamp {
        (jsonfile::stamp(&self.path), crate::managed::stamp(self.managed.as_deref()))
    }

    /// 덱에 전달할 메타데이터 목록 반환 (실행 커맨드 제외)
    pub fn infos(&self) -> Vec<CommandInfo> {
        self.list().iter().map(Command::info).collect()
    }

    /// 사용자 파일 수정 전 호출: 중복 확인용 관리 명령 목록 반환 (strict 모드면 Err(Locked))
    fn fixed(&self) -> Result<Vec<Command>, Error> {
        match self.managed() {
            Some(m) if m.strict => Err(Error::Locked),
            Some(m) => Ok(m.commands),
            None => Ok(Vec::new()),
        }
    }

    /// 새 명령 등록 (ID 자동 생성)
    pub fn add(&self, spec: Command) -> Result<Command, Error> {
        let fixed = self.fixed()?;
        let mut cmd = spec.checked().map_err(Error::Invalid)?;
        jsonfile::edit(&self.path, |user: &mut Vec<Command>| {
            if user.len() >= COUNT_MAX {
                return Err(Error::Invalid(format!("명령은 최대 {COUNT_MAX}개까지 등록할 수 있습니다")));
            }
            if fixed.iter().chain(user.iter()).any(|c| c.name == cmd.name) {
                return Err(Error::Invalid(format!("이미 같은 이름의 명령이 있습니다: {}", cmd.name)));
            }
            let mut n = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos() as u32).unwrap_or(1);
            while fixed.iter().chain(user.iter()).any(|c| c.id == format!("{n:08x}")) {
                n = n.wrapping_add(1);
            }
            cmd.id = format!("{n:08x}");
            user.push(cmd.clone());
            Ok(cmd)
        })
    }

    /// 기존 명령 수정: ID는 유지하고 스펙을 교체한다(덱 위젯이 ID를 참조하므로 계속 동작하나, kind 변경 시 위젯과 불일치할 수 있음).
    pub fn replace(&self, id: &str, spec: Command) -> Result<Command, Error> {
        let fixed = self.fixed()?;
        if fixed.iter().any(|c| c.id == id) {
            return Err(Error::Managed);
        }
        jsonfile::edit(&self.path, |user: &mut Vec<Command>| {
            let at = user.iter().position(|c| c.id == id).ok_or(Error::NotFound)?;
            let mut cmd = spec.checked().map_err(Error::Invalid)?;
            if fixed.iter().chain(user.iter()).any(|c| c.id != id && c.name == cmd.name) {
                return Err(Error::Invalid(format!("이미 같은 이름의 명령이 있습니다: {}", cmd.name)));
            }
            cmd.id = id.to_string();
            user[at] = cmd.clone();
            Ok(cmd)
        })
    }

    /// ID 또는 이름으로 검색 (ID 접두사 매칭 지원, 모호하면 None)
    pub fn find(&self, key: &str) -> Option<Command> {
        let list = self.list();
        if let Some(c) = list.iter().find(|c| c.id == key || c.name == key) {
            return Some(c.clone());
        }
        let mut by_prefix = list.iter().filter(|c| !key.is_empty() && c.id.starts_with(key));
        match (by_prefix.next(), by_prefix.next()) {
            (Some(c), None) => Some(c.clone()),
            _ => None,
        }
    }

    /// 정확한 ID로 검색 (덱에서 넘어온 요청 처리용)
    pub fn by_id(&self, id: &str) -> Option<Command> {
        self.list().into_iter().find(|c| c.id == id)
    }

    /// 명령 삭제 (관리 명령이거나 strict 잠금 상태면 거절)
    pub fn remove(&self, id: &str) -> Result<(), Error> {
        if self.fixed()?.iter().any(|c| c.id == id) {
            return Err(Error::Managed);
        }
        jsonfile::edit(&self.path, |user: &mut Vec<Command>| {
            let before = user.len();
            user.retain(|c| c.id != id);
            if user.len() == before {
                return Err(Error::NotFound);
            }
            Ok(())
        })
    }

    /// 덱 요청 실행: 등록된 ID이고 종류가 run일 때만 실행
    pub fn run(&self, id: &str) -> Result<(), Error> {
        Self::run_by(&self.by_id(id).ok_or(Error::NotFound)?)
    }

    /// 등록된 run 명령 실행 (덱 요청, 로컬 CLI, 웹 콘솔 공용)
    pub fn run_by(cmd: &Command) -> Result<(), Error> {
        if cmd.kind != KIND_RUN {
            return Err(Error::Invalid("단발성 실행(run) 명령이 아닙니다".into()));
        }
        if let Some(a) = cmd.action() {
            return a.write(1.0).map_err(Error::Io);
        }
        start(&cmd.run).map_err(Error::Io)
    }
}

/// 사용자 홈 디렉터리에서 셸 커맨드를 실행할 Process 생성
fn shell(line: &str) -> Process {
    let mut p = if cfg!(windows) {
        let mut p = Process::new("cmd");
        p.arg("/C").arg(line);
        p
    } else {
        let mut p = Process::new("sh");
        p.arg("-c").arg(line);
        p
    };
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| Path::new(h).is_dir()) {
        p.current_dir(home);
    }
    p.stdin(Stdio::null()).stderr(Stdio::null());
    p
}

/// 커맨드 비동기 실행(종료를 기다리지 않고 백그라운드 스레드에서 회수)
fn start(run: &str) -> Result<(), String> {
    let mut child = shell(run).stdout(Stdio::null()).spawn().map_err(|e| format!("실행 실패: {e}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// 값 설정 커맨드를 실행하고 최대 wait까지 종료를 대기한다. 실패로 끝나면 그 사유를 반환한다.
/// 제한 시간 내 미종료 시 백그라운드로 넘기고 성공 처리(GUI 앱 실행 등).
fn run_wait(line: &str, value: &str, wait: Duration) -> Result<(), String> {
    let mut child = shell(line).env("TOSS_VALUE", value).stdout(Stdio::null()).spawn().map_err(|e| format!("실행 실패: {e}"))?;
    let until = Instant::now() + wait;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(status.code().map_or("커맨드가 시그널에 의해 종료되었습니다".to_string(), |code| format!("커맨드가 실패했습니다 (종료 코드 {code})"))),
            Ok(None) if Instant::now() >= until => {
                thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// 타임아웃 시 셸과 하위 프로세스 전체를 함께 종료하기 위해 독립 프로세스 그룹으로 실행한다(kill_group).
#[cfg(unix)]
fn own_group(p: &mut Process) {
    std::os::unix::process::CommandExt::process_group(p, 0);
}

#[cfg(not(unix))]
fn own_group(_: &mut Process) {}

/// own_group으로 생성한 프로세스 그룹 전체에 SIGKILL을 보낸다.
/// 유닉스에서는 그룹 ID가 리더 PID와 같으므로 음수 PID로 kill(2)을 호출한다. 인자는 정수 둘뿐이고 메모리를 건드리지 않는다.
fn kill_group(child: &mut Child) {
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        if let Ok(pid) = i32::try_from(child.id()) {
            unsafe {
                kill(-pid, 9);
            }
        }
    }
    let _ = child.kill();
}

/// 커맨드를 실행해 (성공 여부, stdout 앞부분)을 반환한다.
/// wait 내 미종료 시 프로세스 그룹 전체를 종료하고 실패를 반환하며,
/// 메인 셸 종료 후 백그라운드 자식이 stdout 파이프를 잡고 있어도 0.5초 후 강제 종료해 읽은 출력을 반환한다.
fn output(line: &str, wait: Duration) -> Result<(bool, String), String> {
    let mut process = shell(line);
    process.stdout(Stdio::piped());
    own_group(&mut process);
    let mut child = process.spawn().map_err(|e| format!("실행 실패: {e}"))?;
    let mut pipe = child.stdout.take().ok_or("stdout 파이프를 열 수 없습니다")?;
    let kept = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel::<()>();
    let sink = kept.clone();
    thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut kept = sink.lock().unwrap();
            if kept.len() < 4096 {
                kept.extend_from_slice(&buf[..n]);
            }
        }
        let _ = tx.send(());
    });
    let until = Instant::now() + wait;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() >= until => {
                kill_group(&mut child);
                let _ = child.wait();
                return Err("제한 시간 내에 끝나지 않았습니다".into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    };
    if rx.recv_timeout(Duration::from_millis(500)).is_err() {
        kill_group(&mut child);
    }
    let out = kept.lock().unwrap().clone();
    Ok((status.success(), String::from_utf8_lossy(&out).into_owned()))
}

/// 상태값을 가지는 명령을 관리하는 워커 스레드. 덱이 연결되어 있고 `command.run` 권한이 허용된 동안에만 동작한다.
///   - set 요청이 들어오면 커맨드를 순차 실행한다. 같은 명령에 여러 값이 밀려 있으면 마지막 값만 적용한다
///     (노브를 돌릴 때 값이 연속으로 들어오기 때문).
///   - 상태 조회 커맨드를 주기적으로 실행해 값이 바뀌었을 때만 `on_value`로 알린다. set 직후에는 즉시 다시 조회해
///     값이 바뀌지 않았더라도 결과를 통지한다(설정이 반영되지 않았을 때 덱 UI를 원래 값으로 되돌리기 위함).
///     상태 조회 커맨드가 없는 항목은 set한 값을 그대로 통지한다.
///
/// 명령 목록(`commands.json`)이 변경되면 자동으로 반영하며, 드롭되면 스레드를 종료한다.
pub struct Watcher {
    tx: mpsc::Sender<(String, f64)>,
    stop: Arc<AtomicBool>,
}

impl Watcher {
    pub fn start(cmds: Commands, on_value: impl Fn(String, Option<f64>) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<(String, f64)>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        thread::spawn(move || {
            let mut list: Vec<Command> = Vec::new();
            let mut stamp: Option<Stamp> = None;
            let mut known: HashMap<String, Option<f64>> = HashMap::new();
            let mut due: HashMap<String, Instant> = HashMap::new();
            let mut again: HashMap<String, Instant> = HashMap::new();
            let mut wanted: Vec<(String, f64)> = Vec::new();
            let mut hold: Option<discord::Hold> = None;
            loop {
                if wanted.is_empty() {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(w) => wanted.push(w),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                wanted.extend(rx.try_iter());
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                if stamp != Some(cmds.stamp()) {
                    stamp = Some(cmds.stamp());
                    let fresh = cmds.list();
                    let same = |id: &String| fresh.iter().any(|c| c.id == *id && list.contains(c));
                    known.retain(|id, _| same(id));
                    due.retain(|id, _| same(id));
                    list = fresh;
                    if list.iter().any(|c| c.action().is_some()) != hold.is_some() {
                        hold = hold.is_none().then(discord::hold);
                    }
                }
                let mut latest: Vec<(String, f64)> = Vec::new();
                for (id, v) in wanted.drain(..) {
                    latest.retain(|(other, _)| *other != id);
                    latest.push((id, v));
                }
                for (id, value) in latest {
                    let Some(c) = list.iter().find(|c| c.id == id) else { continue };
                    let done = c.set_value(value);
                    if let Err(e) = &done {
                        eprintln!("명령 {}: 값을 설정하지 못했습니다: {e}", c.name);
                    }
                    if c.watched() {
                        let now = Instant::now();
                        known.remove(&id);
                        due.insert(id.clone(), now);
                        again.insert(id, now + RECHECK);
                    } else if done.is_ok() && known.insert(id.clone(), Some(value)) != Some(Some(value)) {
                        on_value(id, Some(value));
                    }
                }
                for c in list.iter().filter(|c| c.watched()) {
                    let now = Instant::now();
                    if due.get(&c.id).is_some_and(|t| now < *t) {
                        continue;
                    }
                    let value = c.read_value();
                    let next = again.remove(&c.id).filter(|t| *t > Instant::now()).unwrap_or_else(|| Instant::now() + c.every());
                    due.insert(c.id.clone(), next);
                    if known.insert(c.id.clone(), value) != Some(value) {
                        on_value(c.id.clone(), value);
                    }
                    wanted.extend(rx.try_iter());
                    if !wanted.is_empty() || stopped.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        });
        Self { tx, stop }
    }

    /// 값 설정 요청 전달 (value는 Command::normalize를 거친 값)
    pub fn set(&self, id: &str, value: f64) {
        let _ = self.tx.send((id.to_string(), value));
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> (Commands, PathBuf) {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-cmd-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (Commands::new(&dir), dir)
    }

    #[test]
    fn commands_are_registered_found_and_removed() {
        let (cmds, dir) = temp("registry");
        assert!(cmds.list().is_empty());
        let a = cmds.add(Command::run(" Open terminal ", "foot")).unwrap();
        let b = cmds.add(Command::run("Lock", "loginctl lock-session")).unwrap();
        assert_eq!((a.name.as_str(), a.id.len()), ("Open terminal", 8));
        assert_ne!(a.id, b.id);
        assert_eq!(cmds.infos(), [CommandInfo::run(&a.id, "Open terminal"), CommandInfo::run(&b.id, "Lock")]);
        assert!(!serde_json::to_string(&cmds.infos()).unwrap().contains("loginctl"));
        assert_eq!(cmds.find("Lock"), Some(b.clone()));
        assert_eq!(cmds.find(&a.id), Some(a.clone()));
        assert!(cmds.find("no such").is_none());
        assert!(cmds.add(Command::run("Lock", "x")).is_err());
        assert!(cmds.add(Command::run("", "x")).is_err());
        assert!(cmds.add(Command::run("Empty", "  ")).is_err());
        assert_eq!(cmds.remove(&a.id), Ok(()));
        assert_eq!(cmds.remove(&a.id), Err(Error::NotFound));
        assert_eq!(cmds.list(), [b]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// kind 필드가 없던 구버전 파일은 run 명령으로 읽힌다.
    #[test]
    fn a_file_from_before_kinds_still_reads() {
        let (cmds, dir) = temp("old");
        fs::write(dir.join("commands.json"), r#"[{"id":"0000abcd","name":"Terminal","run":"foot"}]"#).unwrap();
        assert_eq!(cmds.list(), [Command { id: "0000abcd".into(), ..Command::run("Terminal", "foot") }]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn kinds_are_checked_and_tell_the_deck_only_their_shape() {
        let (cmds, dir) = temp("kinds");
        let sw = cmds.add(Command::switch("Mic", "mic on", "mic off", "mic state")).unwrap();
        let st = cmds.add(Command::stepped("Profile", &[("power-saver", ""), ("balanced", ""), ("performance", "")], "profile set {}", "profile get")).unwrap();
        let li = cmds.add(Command { every: 1.0, ..Command::linear("Volume", (0.0, 100.0, 5.0), "vol {}%", "vol get") }).unwrap();
        let infos = cmds.infos();
        assert_eq!(infos[0], CommandInfo { kind: KIND_SWITCH.into(), ..CommandInfo::run(&sw.id, "Mic") });
        assert_eq!(infos[1], CommandInfo { kind: KIND_STEPPED.into(), steps: vec!["power-saver".into(), "balanced".into(), "performance".into()], ..CommandInfo::run(&st.id, "Profile") });
        assert_eq!(infos[2], CommandInfo { kind: KIND_LINEAR.into(), min: 0.0, max: 100.0, step: 5.0, ..CommandInfo::run(&li.id, "Volume") });
        let wire = serde_json::to_string(&infos).unwrap();
        for secret in ["mic on", "mic state", "profile set", "vol get"] {
            assert!(!wire.contains(secret), "{secret}가 덱 페이로드에 노출됨");
        }

        assert!(cmds.add(Command::switch("NoOff", "x", "", "")).is_err());
        assert!(cmds.add(Command::stepped("One", &[("a", "x")], "", "")).is_err());
        assert!(cmds.add(Command::stepped("Twice", &[("a", "x"), ("A", "y")], "", "")).is_err());
        assert!(cmds.add(Command::stepped("NoLine", &[("a", "x"), ("b", "")], "", "")).is_err());
        assert!(cmds.add(Command::stepped("Unsafe", &[("a", ""), ("b; echo x", "")], "set {}", "")).is_err());
        assert!(cmds.add(Command::stepped("Fine", &[("a", ""), ("B mode", "set-b")], "set {}", "")).is_ok());
        assert!(cmds.add(Command::linear("Backwards", (10.0, 0.0, 1.0), "x {}", "")).is_err());
        assert!(cmds.add(Command::linear("TooFine", (0.0, 1.0, 0.000_01), "x {}", "")).is_err());
        assert!(cmds.add(Command { every: 1.0, ..Command::switch("Every", "x", "y", "") }).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_from_the_deck_are_fitted_to_the_command() {
        let sw = Command::switch("s", "on", "off", "");
        assert_eq!((sw.normalize(1.0), sw.normalize(0.0)), (Ok(1.0), Ok(0.0)));
        assert!(sw.normalize(2.0).is_err() && sw.normalize(f64::NAN).is_err());
        let st = Command::stepped("p", &[("a", "to-a"), ("b", ""), ("c", "")], "to {}", "");
        assert_eq!(st.normalize(2.0), Ok(2.0));
        assert!(st.normalize(3.0).is_err() && st.normalize(-1.0).is_err() && st.normalize(0.5).is_err());
        let li = Command::linear("v", (0.0, 100.0, 5.0), "vol {}%", "");
        assert_eq!((li.normalize(42.0), li.normalize(43.0), li.normalize(-9.0), li.normalize(1e9)), (Ok(40.0), Ok(45.0), Ok(0.0), Ok(100.0)));
        let fine = Command::linear("f", (0.0, 1.0, 0.1), "f {}", "");
        assert_eq!(fine.normalize(0.31), Ok(0.3));
        assert!(Command::run("r", "x").normalize(1.0).is_err());

        assert_eq!(sw.set_line(1.0), Some(("on".into(), "on".into())));
        assert_eq!(st.set_line(0.0), Some(("to-a".into(), "a".into())));
        assert_eq!(st.set_line(1.0), Some(("to b".into(), "b".into())));
        assert_eq!(li.set_line(45.0), Some(("vol 45%".into(), "45".into())));
        assert_eq!(fine.set_line(0.3), Some(("f 0.3".into(), "0.3".into())));

        assert_eq!((sw.parse_value("ON"), st.parse_value("C"), st.parse_value("1"), li.parse_value("52%")), (Ok(1.0), Ok(2.0), Ok(1.0), Ok(50.0)));
        assert!(st.parse_value("z").is_err());
    }

    /// 시스템 관리 명령과 사용자 명령 병합 규칙: strict면 관리 명령만 노출되고 추가 불가.
    /// non-strict면 둘을 합치되 관리 명령은 수정·삭제 불가하며 중복 시 관리 명령이 우선한다.
    #[test]
    fn managed_commands_are_fixed_and_strict_hides_the_users_own() {
        let (plain, dir) = temp("managed");
        let mine = plain.add(Command::run("Mine", "true")).unwrap();
        plain.add(Command::run("Lock", "user-lock")).unwrap();
        let file = dir.join("managed.json");
        let write = |strict: bool| fs::write(&file, serde_json::json!({ "strict": strict, "commands": [{ "id": "aaaa0001", "name": "Lock", "run": "nix-lock" }] }).to_string()).unwrap();
        let cmds = Commands::with(&dir, Some(file.clone()));

        assert_eq!(cmds.list().len(), 2);
        assert!(!cmds.locked());
        let before = cmds.stamp();

        write(true);
        assert_ne!(cmds.stamp(), before, "관리 설정 생성이 stamp에 반영되지 않음");
        assert!(cmds.locked() && cmds.managed_ids() == ["aaaa0001"]);
        assert_eq!(cmds.list().iter().map(|c| (c.name.as_str(), c.run.as_str())).collect::<Vec<_>>(), [("Lock", "nix-lock")]);
        assert!(cmds.by_id(&mine.id).is_none() && cmds.run(&mine.id).is_err());
        assert_eq!(cmds.add(Command::run("New", "true")), Err(Error::Locked));
        assert!(cmds.replace("aaaa0001", Command::run("Lock", "x")).is_err());
        assert!(cmds.remove("aaaa0001").is_err() && cmds.remove(&mine.id).is_err());

        write(false);
        assert!(!cmds.locked());
        assert_eq!(cmds.list().iter().map(|c| (c.name.as_str(), c.run.as_str())).collect::<Vec<_>>(), [("Lock", "nix-lock"), ("Mine", "true")]);
        let new = cmds.add(Command::run("New", "true")).unwrap();
        assert!(cmds.add(Command::run("Lock", "x")).is_err());
        assert!(cmds.replace(&new.id, Command::run("Lock", "x")).is_err());
        assert_eq!(cmds.replace(&new.id, Command::run("Newer", "true")).unwrap().name, "Newer");
        assert_eq!(cmds.replace("aaaa0001", Command::run("Lock", "x")), Err(Error::Managed));
        assert!(cmds.remove("aaaa0001").is_err());
        assert_eq!(cmds.remove(&new.id), Ok(()));
        let text = fs::read_to_string(dir.join("commands.json")).unwrap();
        assert!(!text.contains("nix-lock") && text.contains("user-lock"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn only_registered_commands_run() {
        let (cmds, dir) = temp("run");
        let mark = dir.join("ran");
        let c = cmds.add(Command::run("Touch", &format!("touch '{}'", mark.display()))).unwrap();
        let sw = cmds.add(Command::switch("Sw", &format!("touch '{}'", mark.display()), "true", "")).unwrap();
        assert!(cmds.run("deadbeef").is_err());
        assert!(cmds.run(&format!("touch '{}'", mark.display())).is_err());
        assert!(cmds.run(&sw.id).is_err());
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(!mark.exists());
        cmds.run(&c.id).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !mark.exists() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(mark.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn state_is_read_from_the_state_command() {
        let sw = |state: &str| Command::switch("s", "true", "true", state).read_value();
        assert_eq!((sw("echo on"), sw("echo No"), sw("true"), sw("false"), sw("echo 1234")), (Some(1.0), Some(0.0), Some(1.0), Some(0.0), Some(1.0)));
        assert_eq!(Command::switch("s", "true", "true", "exec sleep 30").read_value(), None);
        let st = |state: &str| Command::stepped("p", &[("off", "x"), ("Auto", "y")], "", state).read_value();
        assert_eq!((st("echo auto"), st("echo 0"), st("echo other"), st("echo 7")), (Some(1.0), Some(0.0), None, None));
        let li = |state: &str| Command::linear("v", (0.0, 100.0, 1.0), "x {}", state).read_value();
        assert_eq!((li("echo 42"), li("echo '37.5% [on]'"), li("echo 250"), li("echo loud")), (Some(42.0), Some(37.5), Some(100.0), None));
        assert_eq!(li(""), None);
    }

    /// 상태 감시 스레드 동작 검증: 초기 상태 보고, 값 설정 후 변경 보고, 연속 요청 시 마지막 값만 반영, state 미지정 명령은 설정값 그대로 보고
    #[cfg(unix)]
    #[test]
    fn the_watcher_sets_values_and_reports_changes() {
        let (cmds, dir) = temp("watch");
        let file = dir.join("vol");
        fs::write(&file, "20\n").unwrap();
        let vol = cmds.add(Command::linear("Volume", (0.0, 100.0, 5.0), &format!("echo {{}} > '{}'", file.display()), &format!("cat '{}'", file.display()))).unwrap();
        let flag = dir.join("flag");
        let mode = cmds.add(Command::stepped("Mode", &[("off", ""), ("auto", "")], &format!("echo $TOSS_VALUE > '{}'", flag.display()), "")).unwrap();

        let (tx, rx) = mpsc::channel::<(String, Option<f64>)>();
        let tx = std::sync::Mutex::new(tx);
        let watcher = Watcher::start(cmds.clone(), move |id, v| {
            let _ = tx.lock().unwrap().send((id, v));
        });
        let next = || rx.recv_timeout(Duration::from_secs(5)).expect("상태 이벤트 수신 타임아웃");
        assert_eq!(next(), (vol.id.clone(), Some(20.0)));

        for v in [30.0, 55.0, 70.0] {
            watcher.set(&vol.id, v);
        }
        let mut last = None;
        while last != Some(70.0) {
            let (id, v) = next();
            assert_eq!(id, vol.id);
            last = v;
        }
        assert_eq!(fs::read_to_string(&file).unwrap().trim(), "70");

        fs::write(&file, "15\n").unwrap();
        assert_eq!(next(), (vol.id.clone(), Some(15.0)));

        watcher.set(&mode.id, 1.0);
        assert_eq!(next(), (mode.id.clone(), Some(1.0)));
        assert_eq!(fs::read_to_string(&flag).unwrap().trim(), "auto");

        drop(watcher);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 수동 편집된 commands.json 검증: 잘못된 항목(역전된 범위, ID 누락, 범위 초과 주기, 알 수 없는 종류)은 무시하고 파일에서는 지우지 않는다.
    #[test]
    fn entries_edited_by_hand_are_checked_when_read() {
        let (cmds, dir) = temp("byhand");
        let file = dir.join("commands.json");
        fs::write(
            &file,
            r#"[{"id":"aa01","name":"Good","run":"true"},
                {"id":"aa02","name":"Backwards","kind":"linear","min":10,"max":0,"step":1,"set":"x {}"},
                {"name":"NoId","run":"true"},
                {"id":"a b/c","name":"BadId","run":"true"},
                {"id":"aa03","name":"Forever","kind":"switch","on":"x","off":"y","state":"z","every":1e300},
                {"id":"aa04","name":"What","kind":"dial","run":"true"}]"#,
        )
        .unwrap();
        assert_eq!(cmds.list().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Good"]);
        assert!(cmds.by_id("").is_none() && cmds.by_id("aa02").is_none());

        let backwards = Command { id: "aa02".into(), ..Command::linear("Backwards", (10.0, 0.0, 1.0), "x {}", "") };
        assert!(backwards.normalize(5.0).is_err());
        let forever = Command { every: 1e300, ..Command::switch("Forever", "x", "y", "z") };
        assert_eq!(forever.every(), Duration::from_secs_f64(EVERY_MAX));

        cmds.add(Command::run("New", "true")).unwrap();
        assert_eq!(cmds.add(Command::run("Backwards", "true")), Err(Error::Invalid("이미 같은 이름의 명령이 있습니다: Backwards".into())));
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("Backwards") && text.contains("NoId") && text.contains("New"), "{text}");
        assert_eq!(cmds.list().len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 손상된 commands.json: 빈 목록으로 취급하되, 추가/삭제를 시도해도 기존 파일을 덮어쓰지 않는다.
    #[test]
    fn a_broken_file_is_never_written_over() {
        let (cmds, dir) = temp("brokenfile");
        let file = dir.join("commands.json");
        let broken = r#"[{"id":"aa01","name":"Good","run":"true"},"#;
        fs::write(&file, broken).unwrap();
        assert!(cmds.list().is_empty());
        assert!(matches!(cmds.add(Command::run("New", "true")), Err(Error::Io(_))));
        assert!(matches!(cmds.remove("aa01"), Err(Error::Io(_))));
        assert_eq!(fs::read_to_string(&file).unwrap(), broken);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_set_command_that_fails_is_reported() {
        assert_eq!(Command::switch("s", "true", "exit 3", "").set_value(1.0), Ok(()));
        assert!(Command::switch("s", "true", "exit 3", "").set_value(0.0).unwrap_err().contains('3'));
        assert!(Command::run("r", "true").set_value(1.0).is_err());
    }

    /// 상태 확인 커맨드가 백그라운드 자식을 남기고 종료해도 읽은 출력은 유지하고, 타임아웃 시 자식 프로세스 그룹까지 종료한다.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_state_command_keeps_its_output_and_leaves_nothing_behind() {
        let (_, dir) = temp("group");
        let st = |state: &str| Command::stepped("p", &[("off", "x"), ("auto", "y")], "", state).read_value();
        let alive = |file: &Path| {
            let pid = fs::read_to_string(file).unwrap().trim().to_string();
            fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| !s.contains(") Z "))
        };
        let gone = |file: &Path| {
            let until = Instant::now() + Duration::from_secs(3);
            while alive(file) && Instant::now() < until {
                thread::sleep(Duration::from_millis(20));
            }
            !alive(file)
        };

        let left = dir.join("left");
        assert_eq!(st(&format!("echo auto; sleep 30 & echo $! > '{}'", left.display())), Some(1.0));
        assert!(gone(&left), "백그라운드 자식 프로세스가 남아 있음");

        let slow = dir.join("slow");
        let began = Instant::now();
        assert_eq!(st(&format!("sleep 30 & echo $! > '{}'; wait", slow.display())), None);
        assert!(began.elapsed() < STATE_WAIT + Duration::from_secs(2));
        assert!(gone(&slow), "타임아웃된 커맨드의 자식 프로세스가 남아 있음");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discord_commands_take_their_kind_from_the_action_and_drop_shell_lines() {
        let (cmds, dir) = temp("discord");
        let spec = Command { on: "rm -rf ~".into(), state: "true".into(), every: 5.0, ..Command::discord("Mic", "mute") };
        let c = cmds.add(spec).unwrap();
        assert_eq!((c.kind.as_str(), c.discord.as_str(), c.on.as_str(), c.state.as_str(), c.every), (KIND_SWITCH, "mute", "", "", 0.0));
        assert!(c.watched() && c.action().is_some());
        assert_eq!(c.info(), CommandInfo { kind: KIND_SWITCH.into(), ..CommandInfo::run(&c.id, "Mic") });
        let forced = cmds.add(Command { kind: KIND_LINEAR.into(), ..Command::discord("Deaf", "deaf") }).unwrap();
        assert_eq!(forced.kind, KIND_SWITCH);
        assert!(cmds.add(Command::discord("Nope", "self_destruct")).unwrap_err().to_string().contains("mute"));
        let saved = fs::read_to_string(dir.join("commands.json")).unwrap();
        assert!(saved.contains("\"discord\": \"mute\"") && !saved.contains("rm -rf"), "{saved}");
        assert_eq!(cmds.by_id(&c.id), Some(c.clone()));
        assert!(!Command::run("Plain", "true").watched());
        let _ = fs::remove_dir_all(&dir);
    }

}
