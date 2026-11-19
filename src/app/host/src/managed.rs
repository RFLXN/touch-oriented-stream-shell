//! 시스템 관리 설정: 호스트 관리자(NixOS 모듈)가 덱 권한과 등록 명령을 사전에 고정하는 기능.
//!
//! 사용자 설정(decks.json의 perms, commands.json)과 별도로 읽기 전용 파일에서 읽어 온다:
//!   /etc/toss-connector/managed.json      NixOS 모듈 programs.toss-connector.config가 생성 (nix/nixos-module.nix)
//!   { "strict": true, "permissions": ["media.info", "media.control"], "commands": [ { "id": "...", "name": "...", ... } ] }
//! 이 파일이 없으면 시스템 관리 설정이 없는 것으로 보고 모든 설정을 사용자에게 맡긴다.
//!
//! strict (기본값 true)
//!   true   명시된 항목만 허용된다. 권한은 목록에 있는 것만 활성화되며 덱별 설정은 무시된다.
//!          명령도 여기에 적힌 것만 사용되며(사용자 commands.json 무시) CLI·웹 콘솔에서 권한·명령을 수정할 수 없다.
//!   false  명시된 항목만 고정된다. 여기에 적힌 권한은 끌 수 없고 적힌 명령은 수정·삭제할 수 없지만,
//!          나머지 권한은 덱별로 선택할 수 있고 추가 명령도 등록할 수 있다.
//! 권한 설정은 페어링된 모든 덱에 동일하게 적용된다(새 덱도 호스트에서 숫자 확인 후 페어링하므로).
//! 키 쌍, 페어링 목록, 덱 주소, 콘솔 토큰 등 런타임 데이터는 관리 대상이 아니므로 strict 모드에서도 페어링이 가능하다.
//!
//! 파일은 생성 시 검증된다(`toss-connector managed check <파일>`, 모듈 빌드 중 실행): 알 수 없는 권한이나 필드,
//! 잘못된 형식의 명령, 중복 ID/이름이 있으면 빌드가 실패한다. 그럼에도 런타임에 손상된 파일을 만나면
//! 보안상 아무것도 허용하지 않는 strict 모드로 폴백한다(잠금 파일이 깨졌다고 잠금이 풀려서는 안 되기 때문).
//!
//! 실행 중인 toss-connector는 이 파일의 변경(stamp)을 감지해 NixOS 스위치 시 재시작 없이 곧 반영한다.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::commands::{self, Command, Step};
use crate::jsonfile;
use crate::trust::{self, Choices};

/// 시스템 관리 설정 경로. 환경 변수 TOSS_CONNECTOR_MANAGED로 재정의 가능(빈 문자열이면 미사용)
pub const DEFAULT_PATH: &str = "/etc/toss-connector/managed.json";
pub const PATH_ENV: &str = "TOSS_CONNECTOR_MANAGED";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Managed {
    pub strict: bool,
    /// 강제 활성화할 권한 ID 목록 (trust 모듈 테이블 기준)
    pub permissions: Vec<String>,
    /// 등록된 명령 목록. ID까지 포함된다(모듈에서 이름 기반 해시로 생성).
    pub commands: Vec<Command>,
}

/// 역직렬화용 파일 스키마. Managed와 동일하나 오타로 필드가 조용히 무시되는 것을 막기 위해
/// 명령과 단계 내부까지 알 수 없는 필드를 전부 거부한다.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    #[serde(default = "yes")]
    strict: bool,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    commands: Vec<WrittenCommand>,
}

/// JSON에 기록되는 명령 정의: commands::Command의 필드와 1:1 대응(From 구현에서 누락 없이 변환)
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenCommand {
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default = "commands::kind_run")]
    kind: String,
    #[serde(default)]
    run: String,
    #[serde(default)]
    on: String,
    #[serde(default)]
    off: String,
    #[serde(default)]
    steps: Vec<WrittenStep>,
    #[serde(default)]
    set: String,
    #[serde(default)]
    min: f64,
    #[serde(default)]
    max: f64,
    #[serde(default)]
    step: f64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    every: f64,
    #[serde(default)]
    discord: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WrittenStep {
    name: String,
    #[serde(default)]
    run: String,
}

impl From<WrittenCommand> for Command {
    fn from(c: WrittenCommand) -> Self {
        let WrittenCommand { id, name, kind, run, on, off, steps, set, min, max, step, state, every, discord } = c;
        let steps = steps.into_iter().map(|WrittenStep { name, run }| Step { name, run }).collect();
        Command { id, name, kind, run, on, off, steps, set, min, max, step, state, every, discord }
    }
}

fn yes() -> bool {
    true
}

/// 손상된 파일 발견 시 출력할 경고 메시지
fn unreadable(path: &Path, why: &str) -> String {
    format!("시스템 관리 설정({})을 읽을 수 없습니다: {why}. 모든 권한을 차단합니다", path.display())
}

/// 사용할 managed 파일 경로 반환. 테스트 환경에서는 호스트 머신의 파일 영향이 없도록 None을 반환한다.
pub fn default_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    match std::env::var_os(PATH_ENV) {
        Some(p) if p.is_empty() => None,
        Some(p) => Some(p.into()),
        None => Some(DEFAULT_PATH.into()),
    }
}

/// 파일 변경 감지 스탬프. Nix 스토어 파일은 mtime이 항상 같으므로 심볼릭 링크가 가리키는 실제 경로도 포함한다.
pub type Stamp = Option<(PathBuf, Option<SystemTime>, u64)>;

pub fn stamp(path: Option<&Path>) -> Stamp {
    let real = fs::canonicalize(path?).ok()?;
    let meta = fs::metadata(&real).ok()?;
    Some((real, meta.modified().ok(), meta.len()))
}

impl Managed {
    /// 아무것도 허용하지 않는 잠금 상태 (손상된 파일 폴백용)
    fn closed() -> Self {
        Self { strict: true, permissions: Vec::new(), commands: Vec::new() }
    }

    /// JSON 문자열을 파싱·검증하고 정규화된 구조체 반환(앞뒤 공백 제거 등). 오류 시 구체적인 사유 반환
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: Written = serde_json::from_str(text).map_err(|e| format!("JSON 파싱 실패: {e}"))?;
        let mut out = Managed { strict: raw.strict, permissions: Vec::new(), commands: Vec::new() };
        for p in &raw.permissions {
            let known = trust::find(p).ok_or_else(|| format!("알 수 없는 권한: {p} (사용 가능: {})", trust::PERMISSIONS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")))?;
            if !out.permissions.iter().any(|x| x == known.id) {
                out.permissions.push(known.id.to_string());
            }
        }
        for c in raw.commands.into_iter().map(Command::from) {
            let checked = c.checked().map_err(|e| format!("명령 {}: {e}", c.name))?;
            if !commands::valid_id(&checked.id) {
                return Err(format!("명령 {}: ID는 영문자·숫자·'-'·'_' 조합 1~64자여야 합니다", checked.name));
            }
            if let Some(other) = out.commands.iter().find(|o| o.id == checked.id || o.name == checked.name) {
                return Err(format!("명령 {}와(과) {}: ID 또는 이름이 중복됩니다", other.name, checked.name));
            }
            out.commands.push(checked);
        }
        Ok(out)
    }

    /// 파일에서 관리 설정을 읽는다. 파일이 없으면 None, 읽기/파싱 실패 시 경고를 1회 출력하고 모든 권한이 차단된 strict 상태를 반환한다.
    pub fn read(path: Option<&Path>) -> Option<Self> {
        let path = path?;
        let read = match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                jsonfile::complain(path, None);
                return None;
            }
            Err(e) => Err(e.to_string()),
        };
        jsonfile::complain(path, read.as_ref().err().map(|why| unreadable(path, why)).as_deref());
        Some(read.unwrap_or_else(|_| Self::closed()))
    }

    /// 지정 권한의 고정 여부: Some(true/false)면 사용자 변경 불가, None이면 덱별로 자유롭게 설정 가능
    pub fn fixed(&self, id: &str) -> Option<bool> {
        let listed = self.permissions.iter().any(|p| p == id);
        if self.strict {
            Some(listed)
        } else {
            listed.then_some(true)
        }
    }

    /// 사용자 설정 위에 관리 설정을 덮어써 최종 적용 권한을 계산한다.
    pub fn apply(&self, choices: &Choices) -> Choices {
        let mut out = choices.clone();
        for p in trust::PERMISSIONS {
            if let Some(on) = self.fixed(p.id) {
                out.insert(p.id.to_string(), on);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_fixes_everything_and_loose_fixes_only_what_is_listed() {
        let strict = Managed::parse(r#"{"permissions":["media.info"]}"#).unwrap();
        assert!(strict.strict, "strict 생략 시 기본값은 true");
        assert_eq!((strict.fixed("media.info"), strict.fixed("media.control"), strict.fixed("command.run")), (Some(true), Some(false), Some(false)));
        let user = Choices::from([("command.run".to_string(), true), ("media.info".to_string(), false)]);
        let got = strict.apply(&user);
        assert_eq!((trust::allowed(&got, "media.info"), trust::allowed(&got, "media.control"), trust::allowed(&got, "command.run")), (true, false, false));

        let loose = Managed::parse(r#"{"strict":false,"permissions":["media.info"]}"#).unwrap();
        assert_eq!((loose.fixed("media.info"), loose.fixed("command.run")), (Some(true), None));
        let got = loose.apply(&user);
        assert_eq!((trust::allowed(&got, "media.info"), trust::allowed(&got, "media.control"), trust::allowed(&got, "command.run")), (true, true, true));
    }

    #[test]
    fn what_does_not_fit_is_refused_when_checked() {
        let ok = Managed::parse(r#"{"strict":false,"permissions":["command.run","command.run"],"commands":[{"id":"a1b2c3d4","name":" Term ","run":"foot"}]}"#).unwrap();
        assert_eq!((ok.permissions.len(), ok.commands[0].name.as_str(), ok.commands[0].id.as_str()), (1, "Term", "a1b2c3d4"));
        for (bad, why) in [
            (r#"{"permissions":["everything"]}"#, "알 수 없는 권한"),
            (r#"{"commands":[{"id":"a1","name":"X","run":""}]}"#, "빈 커맨드"),
            (r#"{"commands":[{"name":"X","run":"true"}]}"#, "ID 누락"),
            (r#"{"commands":[{"id":"a b","name":"X","run":"true"}]}"#, "잘못된 ID 문자"),
            (r#"{"commands":[{"id":"a1","name":"X","run":"true"},{"id":"a1","name":"Y","run":"true"}]}"#, "중복 ID"),
            (r#"{"commands":[{"id":"a1","name":"X","run":"true"},{"id":"a2","name":"X","run":"true"}]}"#, "중복 이름"),
            (r#"{"commands":[{"id":"a1","name":"V","kind":"linear","min":5,"max":1,"set":"x {}"}]}"#, "역전된 범위"),
            (r#"{"strictt":true}"#, "알 수 없는 필드"),
            (r#"{"commands":[{"id":"a1","name":"X","run":"true","sate":"x"}]}"#, "명령 내부 알 수 없는 필드"),
            (r#"{"commands":[{"id":"a1","name":"S","kind":"stepped","set":"x {}","steps":[{"name":"a"},{"name":"b","rnu":"y"}]}]}"#, "단계 내부 알 수 없는 필드"),
            ("not json", "잘못된 JSON"),
        ] {
            assert!(Managed::parse(bad).is_err(), "{why} 입력이 통과됨");
        }
    }

    #[test]
    fn a_missing_file_means_nothing_is_managed_but_a_broken_one_locks_everything() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-managed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(Managed::read(None), None);
        assert_eq!(Managed::read(Some(&dir.join("none.json"))), None);
        assert_eq!(stamp(Some(&dir.join("none.json"))), None);
        fs::write(dir.join("broken.json"), "{ nope").unwrap();
        let m = Managed::read(Some(&dir.join("broken.json"))).unwrap();
        assert!(m.strict && m.permissions.is_empty() && m.commands.is_empty());
        fs::write(dir.join("m.json"), r#"{"permissions":[]}"#).unwrap();
        let before = stamp(Some(&dir.join("m.json")));
        fs::write(dir.join("m.json"), r#"{"permissions":["media.info"]}"#).unwrap();
        assert_ne!(before, stamp(Some(&dir.join("m.json"))));
        let _ = fs::remove_dir_all(&dir);
    }

    /// `managed check`의 정규화된 출력은 다시 파싱해도 동일한 결과여야 한다(NixOS 모듈에서 /etc에 배치).
    #[test]
    fn what_check_writes_out_reads_back_the_same() {
        let text = r#"{"strict":false,"permissions":["command.run"],"commands":[
            {"id":"a1","name":"Term","run":"foot"},
            {"id":"a2","name":"Mic","kind":"switch","on":"mic on","off":"mic off","state":"mic get","every":5},
            {"id":"a3","name":"Mode","kind":"stepped","set":"mode {}","steps":[{"name":"a"},{"name":"b","run":"mode-b"}]},
            {"id":"a4","name":"Vol","kind":"linear","min":0,"max":100,"step":5,"set":"vol {}"}]}"#;
        let first = Managed::parse(text).unwrap();
        assert_eq!(Managed::parse(&serde_json::to_string_pretty(&first).unwrap()).unwrap(), first);
    }

    /// 파일 손상 경고는 1회만 출력하되, 정상 복구 후 다시 손상되면 재출력한다.
    #[test]
    fn a_file_that_breaks_again_is_reported_again() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-managed-again-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("m.json");
        let said = unreadable(&file, &Managed::parse("{ nope").unwrap_err());
        fs::write(&file, "{ nope").unwrap();
        assert!(Managed::read(Some(&file)).unwrap().strict);
        assert!(!jsonfile::complain(&file, Some(&said)), "손상 파일 읽기 경고가 출력되지 않음");
        fs::write(&file, r#"{"permissions":[]}"#).unwrap();
        assert!(!Managed::read(Some(&file)).unwrap().permissions.contains(&"media.info".to_string()));
        assert!(jsonfile::complain(&file, Some(&said)), "정상 복구 후 재손상 시 경고가 다시 출력되지 않음");
        let _ = fs::remove_dir_all(&dir);
    }
}
