//! 단일 덱 연결 세션 처리(연결이 끊길 때까지 실행).
//!
//!   - 호스트 상태를 이벤트 기반으로 덱에 전송: 재생 정보·앨범 아트, 등록된 명령 목록과 스위치·노브 값,
//!     측정 가능한 리소스 목록과 덱이 구독 중인 리소스 값, 음량 믹서 상태와 덱이 구독 중일 때의 레벨(`audio.rs`).
//!     재생 중 경과 시간은 보내지 않고 덱이 자체 계산하며, 전송 여부는 호스트가 판단한다(media::changed).
//!   - 덱의 액션 요청 처리: 미디어 제어, 음량·음소거·출력 장치 변경, 등록된 명령 실행 및 값 설정(덱은 등록 목록 중에서만 선택 가능),
//!     웹페이지 열기(`web.rs`).
//!   - 웹 콘솔과 CLI의 덱 금고 요청 중계(`vault.rs`): 덱에 보내고 응답을 요청한 쪽에 돌려주며, 덱이 알려 준 금고 상태를 기록한다.
//!
//! 모든 작업은 해당 덱의 권한(Policy) 내에서만 수행한다.
//! 특히 액션 처리 시 매 요청마다 설정 파일을 새로 읽어 권한을 검증해, 방금 해제한 권한이 실행되는 틈을 막는다.
//! 허용되지 않은 요청은 거절하고 로그에 기록한다.
//!
//! 단일 이벤트 루프 스레드가 Wake 이벤트를 받아 처리하며, 송신 피드는 MediaFeed·CommandFeed·MeterFeed·AudioFeed 네 갈래로 나뉘어
//! 각자 직전 전송 상태를 추적한다. Keep-alive ping은 별도 스레드에서 전송해 느린 플레이어나 커맨드에 블로킹되어도 연결이 끊기지 않는다.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use toss_proto::secure::{Conn, MAX_PLAIN};
use toss_proto::{
    CommandInfo, DeckMsg, HostMsg, Media, VaultService, ACTION_AUDIO_DEFAULT, ACTION_AUDIO_MUTE, ACTION_AUDIO_ROUTE, ACTION_AUDIO_VOLUME, ACTION_COMMAND_RUN, ACTION_COMMAND_SET, ACTION_WEB_OPEN, BLOB_CHUNK,
    CAP_AUDIO_CONTROL, CAP_AUDIO_INFO, CAP_COMMAND_RUN, CAP_MEDIA_CONTROL, CAP_MEDIA_INFO, CAP_RESOURCE_INFO, CAP_WEB_OPEN, ERROR_NOT_ALLOWED,
};

use crate::commands::{self, Commands};
use crate::link::{explain, start_pings, why_lost, Attached};
use crate::media::MediaControl;
use crate::store::Store;
use crate::trust::{self, Choices};
use crate::{art, audio, jsonfile, managed, media, meter, vault, web};

/// 권한 설정 및 명령 목록 파일 변경 확인 주기
const POLL_EVERY: Duration = Duration::from_millis(500);
/// 별도 시그널이 없어도 플레이어 상태를 확인하는 주기(탐색(Seeked) 시그널을 보내지 않는 플레이어 보완용)
const READ_EVERY: Duration = Duration::from_secs(2);
/// 플레이어 변경 시그널 수신 후 실제 읽기까지의 디바운스 지연(연속 시그널을 한 번에 처리)
const SETTLE: Duration = Duration::from_millis(40);
/// 앨범 아트 다운로드 실패 시 같은 이미지에 대해 재시도할 최대 횟수
const ART_TRIES: u32 = 3;

/// 현재 호스트 환경에서 실제로 지원 가능한 기능인지 확인(권한이 켜져 있어도 불가능한 기능은 광고하지 않음).
/// 음량 믹서는 리눅스에서 광고한다(오디오 서버가 없으면 상태가 null이다). 테스트에서는 PC의 실제 오디오 서버에 붙지 않도록 끈다.
pub fn can(has_media: bool, cap: &str) -> bool {
    match cap {
        CAP_MEDIA_INFO | CAP_MEDIA_CONTROL => has_media,
        CAP_AUDIO_INFO | CAP_AUDIO_CONTROL => cfg!(all(target_os = "linux", not(test))),
        CAP_COMMAND_RUN => true,
        CAP_RESOURCE_INFO => cfg!(target_os = "linux"),
        CAP_WEB_OPEN => cfg!(any(target_os = "linux", target_os = "macos")),
        _ => false,
    }
}

pub fn show_caps(caps: &[String]) -> String {
    if caps.is_empty() { "없음".to_string() } else { caps.join(", ") }
}

/// 연결된 덱에 적용되는 권한 정책(사용자 설정 위에 시스템 관리 설정을 병합).
/// decks.json이나 managed.json 변경 시 자동 재로드한다(다른 터미널 CLI, 웹 콘솔, NixOS 스위치 대응).
/// 파일이 손상된 동안에는 모든 권한을 차단하되 페어링 삭제와 구분해 연결은 유지한다(복구 시 정상화).
struct Policy<'a> {
    store: &'a Store,
    key: String,
    stamp: jsonfile::Stamp,
    managed_stamp: managed::Stamp,
    choices: Choices,
    /// 이 PC에서 해당 덱의 페어링이 삭제되었는지 여부
    gone: bool,
}

impl<'a> Policy<'a> {
    fn load(store: &'a Store, key: &str) -> Self {
        let mut p = Self { store, key: key.to_string(), stamp: None, managed_stamp: None, choices: Choices::new(), gone: false };
        p.read();
        p
    }

    fn read(&mut self) {
        self.stamp = self.store.stamp();
        self.managed_stamp = self.store.managed_stamp();
        let nothing = || trust::PERMISSIONS.iter().map(|p| (p.id.to_string(), false)).collect();
        match self.store.load_decks().map(|decks| decks.into_iter().find(|d| d.key == self.key)) {
            Ok(Some(d)) => self.choices = self.store.effective(&d.perms),
            Ok(None) => {
                self.choices = nothing();
                self.gone = true;
            }
            Err(_) => self.choices = nothing(),
        }
    }

    /// 파일 변경 감지 시 정책 재로드
    fn refresh(&mut self) {
        if self.store.stamp() != self.stamp || self.store.managed_stamp() != self.managed_stamp {
            self.read();
        }
    }

    fn allows(&self, cap: &str) -> bool {
        trust::allowed(&self.choices, cap)
    }
}

/// 세션 이벤트 루프를 깨우는 이벤트 종류
enum Wake {
    /// 덱의 액션 요청 (id, name, args)
    Action(u64, String, serde_json::Value),
    /// 미디어 플레이어 상태 변경 시그널
    Changed,
    /// 앨범 아트 다운로드·축소 결과 (이미지 URL, 결과)
    Art(String, Result<art::Art, String>),
    /// 스위치·노브 명령 상태 값 변경 (명령 ID, 값)
    Value(String, Option<f64>),
    /// 덱의 구독 변경 (리소스 ID 목록, 음량 레벨 구독 여부)
    Watch(Vec<String>, bool),
    /// 리소스 측정 스레드 이벤트 (카탈로그 또는 값 변경)
    Meter(meter::Event),
    /// 음량 믹서 스레드 이벤트 (상태 또는 레벨 변경)
    Audio(audio::Event),
    /// 웹 콘솔이나 CLI가 보낸 금고 요청 (덱에 전달하고 응답을 기다린다)
    Vault(vault::Call),
    /// 금고 요청에 대한 덱의 응답 (요청 번호, 결과)
    VaultReply(u64, Result<serde_json::Value, String>),
    /// 덱이 알려 준 금고 상태
    VaultState(Vec<VaultService>),
    /// 수신 스레드 종료(연결 끊김)
    Ended,
}

/// 대용량 데이터를 청크로 분할 전송(전송 중 다른 스레드의 ping이 끼어들 수 있음)
fn send_blob(conn: &Conn, id: &str, mime: &str, bytes: &[u8]) -> io::Result<()> {
    let count = bytes.len().div_ceil(BLOB_CHUNK) as u32;
    for (index, chunk) in bytes.chunks(BLOB_CHUNK).enumerate() {
        let data = base64::engine::general_purpose::STANDARD.encode(chunk);
        conn.send(&HostMsg::Blob { id: id.to_string(), mime: mime.to_string(), index: index as u32, count, data })?;
    }
    Ok(())
}

/// 미디어 정보 및 앨범 아트 전송 피드
#[derive(Default)]
struct MediaFeed {
    /// 마지막으로 전송한 미디어 정보와 전송 시각 (아직 미전송이면 None)
    sent: Option<(Option<Media>, Instant)>,
    /// 현재 곡의 앨범 아트 (URL, ID). 없으면 빈 문자열 쌍
    art: (String, String),
    /// 전송 완료 또는 포기 처리된 앨범 아트 ID
    art_done: String,
    /// 현재 비동기 다운로드 중인 앨범 아트 URL
    art_fetching: String,
    /// 다운로드 실패 기록 (ID, 실패 횟수)
    art_failed: (String, u32),
}

impl MediaFeed {
    /// 플레이어를 조회해 덱 상태와 다르면 전송하고, 미전송 앨범 아트가 있으면 비동기 다운로드를 시작한다.
    fn poll(&mut self, media: &mut Option<Box<dyn MediaControl>>, allowed: bool, conn: &Conn, tx: &Sender<Wake>) -> io::Result<()> {
        let read = if allowed { media.as_mut().and_then(|m| m.poll()) } else { None };
        let now = Instant::now();
        let cur = read.map(|n| {
            let id = if n.art_url.is_empty() { String::new() } else { art::id_of(&n.art_url) };
            self.art = (n.art_url, id.clone());
            Media { art: id, ..n.media }
        });
        if cur.is_none() {
            self.art = (String::new(), String::new());
        }
        let event = match &self.sent {
            None => true,
            Some((before, at)) => media::changed(before.as_ref(), at.elapsed(), cur.as_ref()),
        };
        if event {
            conn.send(&HostMsg::Media { media: cur.clone() })?;
            match &cur {
                Some(m) => eprintln!("재생 정보 전송: {} {}:{:02}{}", m.status, m.position_ms / 60_000, m.position_ms / 1000 % 60, if m.rate == 1.0 { String::new() } else { format!(" ×{}", m.rate) }),
                None => {
                    eprintln!("재생 정보 전송: 없음");
                    self.art_done.clear();
                }
            }
            self.sent = Some((cur, now));
        }
        let (url, id) = self.art.clone();
        if !id.is_empty() && id != self.art_done && self.art_fetching != url {
            self.art_fetching = url.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let got = art::fetch(&url);
                let _ = tx.send(Wake::Art(url, got));
            });
        }
        Ok(())
    }

    /// 앨범 아트 다운로드 완료 처리. 여전히 같은 곡이면 청크 전송하고, 실패 시 다음 폴링에 최대 ART_TRIES회까지 재시도한다.
    fn on_art(&mut self, url: String, got: Result<art::Art, String>, allowed: bool, conn: &Conn) -> io::Result<()> {
        if self.art_fetching == url {
            self.art_fetching.clear();
        }
        if url != self.art.0 || !allowed {
            return Ok(());
        }
        let id = self.art.1.clone();
        match got {
            Ok(art) => {
                send_blob(conn, &art.id, art.mime, &art.bytes)?;
                eprintln!("앨범 아트 전송: {}KB", art.bytes.len().div_ceil(1024));
                self.art_done = id;
            }
            Err(e) => {
                let tries = if self.art_failed.0 == id { self.art_failed.1 + 1 } else { 1 };
                eprintln!("앨범 아트 가져오기 실패 ({tries}회차): {e}");
                if tries >= ART_TRIES {
                    self.art_done = id.clone();
                }
                self.art_failed = (id, tries);
            }
        }
        Ok(())
    }
}

/// 명령 목록을 단일 메시지 최대 크기(MAX_PLAIN)에 맞게 앞에서부터 자른다.
/// 개수 상한만으로는 페이로드 초과를 막을 수 없으므로(크기 초과 시 전송 실패→재접속 루프 발생) 바이트 단위로 제한한다.
fn fitting(list: Vec<CommandInfo>) -> Vec<CommandInfo> {
    let mut used = serde_json::to_vec(&HostMsg::Commands { commands: Vec::new() }).map_or(0, |v| v.len());
    let mut out = Vec::new();
    for c in list {
        used += serde_json::to_vec(&c).map_or(0, |v| v.len()) + 1;
        if used > MAX_PLAIN {
            break;
        }
        out.push(c);
    }
    out
}

/// 등록된 명령 목록 및 스위치·노브 상태 전송과 덱 명령 요청 처리 피드
struct CommandFeed {
    cmds: Commands,
    /// 마지막으로 전송한 목록 (미전송 시 None)
    sent: Option<Vec<CommandInfo>>,
    /// 목록 전송 당시의 스탬프 (None이면 다음 폴링 시 목록을 다시 만든다)
    stamp: Option<commands::Stamp>,
    /// 상태 감시 및 값 설정 스레드 (command.run 권한이 있을 때만 생성)
    watcher: Option<commands::Watcher>,
    /// 현재까지 알려진 명령별 상태 값 (목록 재전송 시 함께 보냄)
    values: HashMap<String, Option<f64>>,
    /// 메시지 크기 제한으로 제외된 명령 개수 (변경 시에만 경고 출력)
    cut: usize,
}

impl CommandFeed {
    fn new(cmds: Commands) -> Self {
        Self { cmds, sent: None, stamp: None, watcher: None, values: HashMap::new(), cut: 0 }
    }

    /// 덱 요청 실행: 등록된 명령의 단발 실행(command.run) 또는 등록된 스위치·노브의 값 설정(command.set)
    fn act(&self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        let id = args["id"].as_str().unwrap_or_default();
        if name == ACTION_COMMAND_RUN {
            self.cmds.run(id).map_err(|e| e.to_string())?;
            eprintln!("명령 실행: {}", self.cmds.by_id(id).map(|c| c.name).unwrap_or_default());
            return Ok(());
        }
        match (self.cmds.by_id(id), args["value"].as_f64(), &self.watcher) {
            (None, _, _) => Err(commands::Error::NotFound.to_string()),
            (_, None, _) => Err("value 값이 없습니다".to_string()),
            (_, _, None) => Err("현재 값을 설정할 수 없는 상태입니다".to_string()),
            (Some(c), Some(v), Some(w)) => c.normalize(v).map(|v| {
                w.set(&c.id, v);
                eprintln!("명령 값 설정: {} = {}", c.name, c.show_value(Some(v)));
            }),
        }
    }

    /// 스위치·노브 상태 변경 처리: 값을 캐싱하고, 덱에 전송된 목록에 포함된 명령이면 전달
    fn on_value(&mut self, id: String, value: Option<f64>, conn: &Conn) -> io::Result<()> {
        self.values.insert(id.clone(), value);
        if self.watcher.is_some() && self.sent.as_ref().is_some_and(|l| l.iter().any(|c| c.id == id)) {
            conn.send(&HostMsg::CommandValue { id, value })?;
        }
        Ok(())
    }

    /// 명령 목록이나 권한 변경 시 목록을 재전송하고 기존 상태 값도 이어서 전달
    fn poll(&mut self, allowed: bool, conn: &Conn, tx: &Sender<Wake>) -> io::Result<()> {
        let stamp = self.cmds.stamp();
        if self.stamp.as_ref() == Some(&stamp) {
            return Ok(());
        }
        self.stamp = Some(stamp);
        let all = if allowed { self.cmds.infos() } else { Vec::new() };
        let count = all.len();
        let list = fitting(all);
        if count - list.len() != self.cut {
            self.cut = count - list.len();
            if self.cut > 0 {
                eprintln!("명령 목록이 단일 메시지 크기 제한을 초과해 {count}개 중 앞의 {}개만 전송합니다 (명령이나 단계 수를 줄이세요)", list.len());
            }
        }
        if allowed && self.watcher.is_none() {
            let tx = Mutex::new(tx.clone());
            self.watcher = Some(commands::Watcher::start(self.cmds.clone(), move |id, value| {
                let _ = tx.lock().unwrap().send(Wake::Value(id, value));
            }));
        } else if !allowed && self.watcher.is_some() {
            self.watcher = None;
            self.values.clear();
        }
        if self.sent.as_ref() != Some(&list) && !(self.sent.is_none() && list.is_empty()) {
            conn.send(&HostMsg::Commands { commands: list.clone() })?;
            self.values.retain(|id, _| list.iter().any(|c| c.id == *id));
            for (id, value) in &self.values {
                conn.send(&HostMsg::CommandValue { id: id.clone(), value: *value })?;
            }
        }
        self.sent = Some(list);
        Ok(())
    }
}

/// 시스템 리소스 목록 및 측정값 전송 피드
#[derive(Default)]
struct MeterFeed {
    /// 리소스 측정 스레드 (resource.info 권한이 있을 때만 활성)
    meter: Option<meter::Meter>,
    /// 덱이 구독 중인 리소스 ID 목록
    watch: Vec<String>,
    /// 카탈로그 전송 여부 (권한 해제 시 빈 목록으로 초기화하기 위해 추적)
    catalog_sent: bool,
}

impl MeterFeed {
    fn on_watch(&mut self, ids: Vec<String>) {
        self.watch = ids;
        if let Some(m) = &self.meter {
            m.watch(self.watch.clone());
        }
    }

    fn on_event(&mut self, event: meter::Event, conn: &Conn) -> io::Result<()> {
        if self.meter.is_none() {
            return Ok(());
        }
        conn.send(&match event {
            meter::Event::Catalog(metrics) => {
                self.catalog_sent = true;
                HostMsg::Metrics { metrics }
            }
            meter::Event::Values(values) => HostMsg::MetricValues { values },
        })
    }

    /// 권한 활성화 시 측정 시작, 비활성화 시 중지 및 덱 측 목록 초기화
    fn poll(&mut self, allowed: bool, conn: &Conn, tx: &Sender<Wake>) -> io::Result<()> {
        if allowed && self.meter.is_none() {
            let tx = Mutex::new(tx.clone());
            self.meter = Some(meter::Meter::start(self.watch.clone(), move |event| {
                let _ = tx.lock().unwrap().send(Wake::Meter(event));
            }));
        } else if !allowed && self.meter.is_some() {
            self.meter = None;
            if std::mem::take(&mut self.catalog_sent) {
                conn.send(&HostMsg::Metrics { metrics: Vec::new() })?;
            }
        }
        Ok(())
    }
}

/// 음량 믹서 상태와 레벨 전송 피드
#[derive(Default)]
struct AudioFeed {
    /// 음량 믹서 스레드 (audio.info 권한이 있을 때만 활성)
    mixer: Option<audio::Mixer>,
    /// 덱이 레벨을 구독 중인지
    watch: bool,
    /// 상태를 보낸 적이 있는지 (권한 해제 시 null로 지우기 위해 추적)
    sent: bool,
}

impl AudioFeed {
    fn on_watch(&mut self, on: bool) {
        self.watch = on;
        if let Some(m) = &self.mixer {
            m.watch(on);
        }
    }

    fn on_event(&mut self, event: audio::Event, conn: &Conn) -> io::Result<()> {
        if self.mixer.is_none() {
            return Ok(());
        }
        conn.send(&match event {
            audio::Event::State(audio) => {
                self.sent = true;
                HostMsg::Audio { audio }
            }
            audio::Event::Levels { apps, outputs } => HostMsg::AudioLevels { apps, outputs },
        })
    }

    fn act(&self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        match &self.mixer {
            Some(m) => m.act(name, args),
            None => Err("이 PC에서 음량 정보 공유가 꺼져 있습니다".to_string()),
        }
    }

    /// 권한 활성화 시 믹서 시작, 비활성화 시 중지 및 덱 측 상태 초기화
    fn poll(&mut self, allowed: bool, conn: &Conn, tx: &Sender<Wake>) -> io::Result<()> {
        if allowed && self.mixer.is_none() {
            let tx = Mutex::new(tx.clone());
            self.mixer = Some(audio::Mixer::start(self.watch, move |event| {
                let _ = tx.lock().unwrap().send(Wake::Audio(event));
            }));
        } else if !allowed && self.mixer.is_some() {
            self.mixer = None;
            if std::mem::take(&mut self.sent) {
                conn.send(&HostMsg::Audio { audio: None })?;
            }
        }
        Ok(())
    }
}

/// 단일 덱 연결 세션 상태
struct Session<'a> {
    conn: Arc<Conn>,
    tx: Sender<Wake>,
    policy: Policy<'a>,
    media: &'a mut Option<Box<dyn MediaControl>>,
    /// 덱에 마지막으로 전달한 권한 목록
    caps: Vec<String>,
    playing: MediaFeed,
    commands: CommandFeed,
    meters: MeterFeed,
    sound: AudioFeed,
    next_poll: Instant,
    next_read: Instant,
    /// 덱의 응답을 기다리는 금고 요청 (요청 번호 -> 응답 채널)
    vault_calls: HashMap<u64, Sender<Result<serde_json::Value, String>>>,
    next_vault: u64,
}

impl Session<'_> {
    /// 다음 주기 작업까지 남은 대기 시간
    fn wait(&self) -> Duration {
        self.next_poll.min(self.next_read).saturating_duration_since(Instant::now())
    }

    /// 덱 액션 처리. 매 요청마다 최신 설정을 다시 읽어 권한을 검증한다.
    fn act(&mut self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        self.policy.read();
        if let Err(why) = trust::check(&self.policy.choices, name) {
            eprintln!("덱 요청 거부됨 ({name}): {}", if why == ERROR_NOT_ALLOWED { "권한 없음" } else { why.as_str() });
            return Err(why);
        }
        let done = match name {
            ACTION_COMMAND_RUN | ACTION_COMMAND_SET => self.commands.act(name, args),
            ACTION_AUDIO_VOLUME | ACTION_AUDIO_MUTE | ACTION_AUDIO_ROUTE | ACTION_AUDIO_DEFAULT => self.sound.act(name, args),
            ACTION_WEB_OPEN => {
                let url = args["url"].as_str().unwrap_or_default();
                web::open(url).inspect(|()| eprintln!("웹페이지 열기: {url}"))
            }
            _ => match self.media.as_mut() {
                Some(m) => m.action(name, args),
                None => Err("현재 호스트에서 지원하지 않는 액션입니다".to_string()),
            },
        };
        if let Err(e) = &done {
            eprintln!("요청 실행 실패 ({name}): {e}");
        }
        done
    }

    /// 단일 이벤트 처리. 세션 종료가 필요하면 종료 사유를 반환한다.
    fn turn(&mut self, woke: Result<Wake, RecvTimeoutError>) -> io::Result<Option<String>> {
        match woke {
            Ok(Wake::Action(id, name, args)) => {
                let done = self.act(&name, &args);
                self.conn.send(&HostMsg::Result { id, ok: done.is_ok(), error: done.err().unwrap_or_default() })?;
                self.next_read = self.next_read.min(Instant::now() + Duration::from_millis(80));
            }
            Ok(Wake::Changed) => self.next_read = self.next_read.min(Instant::now() + SETTLE),
            Ok(Wake::Art(url, got)) => self.playing.on_art(url, got, self.policy.allows(CAP_MEDIA_INFO), &self.conn)?,
            Ok(Wake::Value(id, value)) => self.commands.on_value(id, value, &self.conn)?,
            Ok(Wake::Watch(ids, audio)) => {
                self.meters.on_watch(ids);
                self.sound.on_watch(audio);
            }
            Ok(Wake::Meter(event)) => self.meters.on_event(event, &self.conn)?,
            Ok(Wake::Audio(event)) => self.sound.on_event(event, &self.conn)?,
            Ok(Wake::Vault(call)) => {
                let id = self.next_vault;
                self.next_vault += 1;
                self.conn.send(&HostMsg::Vault { id, op: call.op, service: call.service, args: call.args })?;
                self.vault_calls.insert(id, call.reply);
            }
            Ok(Wake::VaultReply(id, got)) => {
                if let Some(reply) = self.vault_calls.remove(&id) {
                    let _ = reply.send(got);
                }
            }
            Ok(Wake::VaultState(services)) => vault::set_state(&self.conn.remote, services),
            Ok(Wake::Ended) | Err(RecvTimeoutError::Disconnected) => return Ok(Some(String::new())),
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if now >= self.next_poll {
            self.next_poll = now + POLL_EVERY;
            self.policy.refresh();
            if self.policy.gone {
                return Ok(Some("이 PC에서 해당 덱의 페어링이 삭제되었습니다".to_string()));
            }
            let has_media = self.media.is_some();
            let caps_now = trust::caps(&self.policy.choices, |cap| can(has_media, cap));
            if caps_now != self.caps {
                eprintln!("권한 변경됨: {}", show_caps(&caps_now));
                self.conn.send(&HostMsg::Caps { caps: caps_now.clone() })?;
                self.caps = caps_now;
                self.next_read = now;
                self.commands.stamp = None;
            }
            self.meters.poll(self.policy.allows(CAP_RESOURCE_INFO) && can(has_media, CAP_RESOURCE_INFO), &self.conn, &self.tx)?;
            self.sound.poll(self.policy.allows(CAP_AUDIO_INFO) && can(has_media, CAP_AUDIO_INFO), &self.conn, &self.tx)?;
            self.commands.poll(self.policy.allows(CAP_COMMAND_RUN), &self.conn, &self.tx)?;
        }
        if now >= self.next_read {
            self.next_read = now + READ_EVERY;
            self.playing.poll(self.media, self.policy.allows(CAP_MEDIA_INFO), &self.conn, &self.tx)?;
        }
        Ok(None)
    }
}

/// 덱 연결 세션을 실행하고, 종료 시 끊김 사유를 반환한다.
pub fn session(a: Attached, media: &mut Option<Box<dyn MediaControl>>, store: &Store) -> String {
    let Attached { conn, mut reader, caps, .. } = a;
    let policy = Policy::load(store, &conn.remote);
    let (tx, rx) = mpsc::channel::<Wake>();

    let (read_conn, read_tx) = (conn.clone(), tx.clone());
    let read_thread = thread::spawn(move || {
        let why = loop {
            let wake = match read_conn.recv::<DeckMsg>(&mut reader) {
                Ok(Some(DeckMsg::Action { id, name, args })) => Wake::Action(id, name, args),
                Ok(Some(DeckMsg::Watch { metrics, audio })) => Wake::Watch(metrics.into_iter().filter(|m| m.len() <= 64).take(256).collect(), audio),
                Ok(Some(DeckMsg::VaultReply { id, ok, error, data })) => Wake::VaultReply(id, if ok { Ok(data) } else { Err(error.chars().filter(|c| !c.is_control()).take(300).collect()) }),
                Ok(Some(DeckMsg::VaultState { services })) => Wake::VaultState(services.into_iter().take(16).collect()),
                Ok(Some(DeckMsg::Denied { reason })) => break explain(&reason).to_string(),
                Ok(_) => continue,
                Err(e) => break why_lost(&e),
            };
            if read_tx.send(wake).is_err() {
                break String::new();
            }
        };
        let _ = read_tx.send(Wake::Ended);
        why
    });
    let alive = start_pings(&conn);

    if let Some(m) = media.as_mut() {
        let tx = Mutex::new(tx.clone());
        m.on_change(Some(Box::new(move || {
            let _ = tx.lock().unwrap().send(Wake::Changed);
        })));
    }

    let line = Mutex::new(tx.clone());
    vault::attach(Box::new(move |call| line.lock().unwrap().send(Wake::Vault(call)).is_ok()));
    let commands = CommandFeed::new(store.commands());
    let mut s = Session {
        conn,
        tx,
        policy,
        media,
        caps,
        playing: MediaFeed::default(),
        commands,
        meters: MeterFeed::default(),
        sound: AudioFeed::default(),
        next_poll: Instant::now(),
        next_read: Instant::now(),
        vault_calls: HashMap::new(),
        next_vault: 1,
    };
    let result = loop {
        match s.turn(rx.recv_timeout(s.wait())) {
            Ok(None) => {}
            Ok(Some(why)) => break why,
            Err(e) => break why_lost(&e),
        }
    };
    vault::detach();
    for (_, reply) in s.vault_calls.drain() {
        let _ = reply.send(Err("덱 연결이 끊어졌습니다".into()));
    }
    alive.store(false, Ordering::SeqCst);
    if let Some(m) = s.media.as_mut() {
        m.on_change(None);
    }
    s.conn.shutdown();
    let read_end = read_thread.join().unwrap_or_default();
    if s.policy.gone || read_end.is_empty() { result } else { read_end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::attach;
    use crate::store::Deck;
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use toss_proto::secure::Identity;
    use toss_proto::{ACTION_MEDIA_NEXT, PROTO_VERSION, STATUS_PAIRED};

    /// 모의 미디어 플레이어. 외부에서 상태를 변경하고(set) 변경 시그널을 발생시킨다.
    #[derive(Default)]
    struct Fake {
        now: Option<media::Now>,
        /// 재생 시작 시각 (재생 중일 때 경과 위치 계산 기준)
        since: Option<Instant>,
        done: Vec<String>,
        notify: Option<media::Notify>,
        /// poll 시 강제 지연 시간 (응답이 느린 플레이어 시뮬레이션)
        stall: Duration,
    }
    #[derive(Clone, Default)]
    struct FakePlayer(Arc<Mutex<Fake>>);
    impl FakePlayer {
        fn stall(&self, each: Duration) {
            self.0.lock().unwrap().stall = each;
        }
        fn set(&self, title: &str, status: &str, position_ms: u64, art_url: &str) {
            let mut f = self.0.lock().unwrap();
            f.now = Some(media::Now {
                media: Media { title: title.into(), status: status.into(), position_ms, length_ms: 600_000, ..Media::default() },
                art_url: art_url.into(),
            });
            f.since = Some(Instant::now());
            if let Some(n) = &f.notify {
                n();
            }
        }
        fn done(&self) -> Vec<String> {
            self.0.lock().unwrap().done.clone()
        }
    }
    impl media::MediaControl for FakePlayer {
        fn poll(&mut self) -> Option<media::Now> {
            let stall = self.0.lock().unwrap().stall;
            thread::sleep(stall);
            let f = self.0.lock().unwrap();
            let mut now = f.now.clone()?;
            now.media.position_ms = now.media.position_after(f.since.map_or(0, |t| t.elapsed().as_millis() as u64));
            Some(now)
        }
        fn action(&mut self, name: &str, args: &serde_json::Value) -> Result<(), String> {
            self.0.lock().unwrap().done.push(if args.is_null() { name.to_string() } else { format!("{name} {args}") });
            Ok(())
        }
        fn on_change(&mut self, notify: Option<media::Notify>) {
            self.0.lock().unwrap().notify = notify;
        }
    }

    /// ping과 리소스 메시지는 건너뛰고 다음 메시지 수신 (리소스 테스트는 별도 함수 사용)
    fn next_msg(conn: &Conn, reader: &mut TcpStream) -> HostMsg {
        loop {
            match conn.recv::<HostMsg>(reader).unwrap() {
                Some(HostMsg::Ping | HostMsg::Metrics { .. } | HostMsg::MetricValues { .. }) | None => {}
                Some(m) => return m,
            }
        }
    }

    /// 모의 덱과 실제 session 스레드 연결 테스트 리그
    struct Rig {
        store: Store,
        dir: PathBuf,
        deck_key: String,
        conn: Arc<Conn>,
        reader: TcpStream,
        hello_caps: Vec<String>,
        host: thread::JoinHandle<String>,
    }

    fn rig(name: &str, player: FakePlayer) -> Rig {
        rig_with(name, player, None)
    }

    /// managed에 JSON 문자열을 주면 설정 디렉터리의 managed.json으로 사용
    fn rig_with(name: &str, player: FakePlayer, managed: Option<&str>) -> Rig {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let managed = managed.map(|text| {
            std::fs::write(dir.join("managed.json"), text).unwrap();
            dir.join("managed.json")
        });
        let store = Store::open(Some(dir.clone())).unwrap().with_managed(managed.clone());
        let (me, deck_id) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        store.remember(Deck { key: deck_id.id(), name: "deck".into(), addrs: Vec::new(), wake: String::new(), perms: Choices::new() }).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        let (store2, key) = (Store::open(Some(dir.clone())).unwrap().with_managed(managed), deck_id.id());
        let host = thread::spawn(move || {
            let decks = store2.decks();
            let caps_for = |k: &str| decks.iter().find(|d| d.key == k).map(|d| trust::caps(&store2.effective(&d.perms), |cap| can(true, cap)));
            let a = attach(&addr, &me, false, caps_for).unwrap().expect("페어링된 덱이어야 함");
            assert_eq!(a.conn.remote, key);
            let mut media: Option<Box<dyn media::MediaControl>> = Some(Box::new(player));
            session(a, &mut media, &store2)
        });

        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
        let (conn, mut reader) = Conn::establish(stream, &deck_id, false).unwrap();
        let conn = Arc::new(conn);
        let Some(HostMsg::Hello { caps, .. }) = conn.recv::<HostMsg>(&mut reader).unwrap() else { panic!("hello 메시지가 아님") };
        conn.send(&DeckMsg::Hello { proto: PROTO_VERSION, name: "deck".into(), status: STATUS_PAIRED.into(), addrs: Vec::new(), wake: String::new() }).unwrap();
        let pinger = conn.clone();
        thread::spawn(move || {
            while pinger.send(&DeckMsg::Ping).is_ok() {
                thread::sleep(Duration::from_millis(100));
            }
        });
        Rig { store, dir, deck_key: deck_id.id(), conn, reader, hello_caps: caps, host }
    }

    /// 연결 중 권한 변경 반영 확인: 해제된 액션은 차단되고, 변경된 권한과 미디어 초기화가 덱에 전송되며, 재허용 시 복구된다.
    #[test]
    fn permissions_are_enforced_and_follow_the_record_live() {
        let player = FakePlayer::default();
        player.set("곡", "paused", 0, "");
        let Rig { store, dir, deck_key, conn, mut reader, hello_caps, host } = rig("perms", player.clone());
        assert_eq!(hello_caps, [CAP_MEDIA_INFO, CAP_MEDIA_CONTROL, CAP_RESOURCE_INFO]);

        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(m) } if m.title == "곡"));
        conn.send(&DeckMsg::Action { id: 1, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null }).unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Result { id: 1, ok: true, error: String::new() });
        assert_eq!(player.done(), [ACTION_MEDIA_NEXT]);
        conn.send(&DeckMsg::Action { id: 9, name: toss_proto::ACTION_MEDIA_SEEK.into(), args: serde_json::json!({"offset_ms": -5000}) }).unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Result { id: 9, ok: true, error: String::new() });
        assert_eq!(player.done()[1], r#"media.seek {"offset_ms":-5000}"#);

        conn.send(&DeckMsg::Action { id: 2, name: "exec".into(), args: serde_json::json!({"cmd": "true"}) }).unwrap();
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Result { id: 2, ok: false, .. }));

        thread::sleep(Duration::from_millis(20));
        store
            .update(&deck_key, |d| {
                d.perms.insert(CAP_MEDIA_INFO.into(), false);
                d.perms.insert(CAP_MEDIA_CONTROL.into(), false);
                d.perms.insert(CAP_RESOURCE_INFO.into(), false);
            })
            .unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Caps { caps: Vec::new() });
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Media { media: None });
        conn.send(&DeckMsg::Action { id: 3, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null }).unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Result { id: 3, ok: false, error: ERROR_NOT_ALLOWED.into() });
        assert_eq!(player.done().len(), 2, "차단된 액션이 플레이어까지 전달됨");

        thread::sleep(Duration::from_millis(20));
        store.update(&deck_key, |d| { d.perms.insert(CAP_MEDIA_INFO.into(), true); }).unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Caps { caps: vec![CAP_MEDIA_INFO.into()] });
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(_) }));

        thread::sleep(Duration::from_millis(20));
        store.forget(&deck_key).unwrap();
        assert_eq!(host.join().unwrap(), "이 PC에서 해당 덱의 페어링이 삭제되었습니다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 명령 버튼: 권한 허용 전에는 목록도 전송되지 않고 실행도 거부된다.
    /// 허용 후에는 등록된 명령의 목록(ID와 이름만)이 전달되며, 그중 하나를 ID로 요청하면 실행된다.
    /// 등록되지 않은 것은 허용해도 실행되지 않는다.
    #[cfg(unix)]
    #[test]
    fn registered_commands_run_only_when_allowed() {
        let player = FakePlayer::default();
        let Rig { store, dir, deck_key, conn, mut reader, hello_caps, host } = rig("commands", player);
        assert!(!hello_caps.contains(&CAP_COMMAND_RUN.to_string()));
        let mark = dir.join("ran");
        let cmds = commands::Commands::new(&dir);
        let c = cmds.add(commands::Command::run("Touch", &format!("touch '{}'", mark.display()))).unwrap();
        let run = |id: u64, cmd: &str| conn.send(&DeckMsg::Action { id, name: ACTION_COMMAND_RUN.into(), args: serde_json::json!({"id": cmd}) }).unwrap();
        fn next_other(conn: &Conn, reader: &mut TcpStream) -> HostMsg {
            loop {
                match next_msg(conn, reader) {
                    HostMsg::Media { .. } => {}
                    m => return m,
                }
            }
        }

        run(1, &c.id);
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 1, ok: false, error: toss_proto::ERROR_NOT_ALLOWED.into() });

        thread::sleep(Duration::from_millis(20));
        store.update(&deck_key, |d| { d.perms.insert(CAP_COMMAND_RUN.into(), true); }).unwrap();
        assert!(matches!(next_other(&conn, &mut reader), HostMsg::Caps { caps } if caps.contains(&CAP_COMMAND_RUN.to_string())));
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Commands { commands: vec![toss_proto::CommandInfo::run(&c.id, "Touch")] });

        run(2, "deadbeef");
        assert!(matches!(next_other(&conn, &mut reader), HostMsg::Result { id: 2, ok: false, .. }));
        run(3, &format!("touch '{}'", mark.display()));
        assert!(matches!(next_other(&conn, &mut reader), HostMsg::Result { id: 3, ok: false, .. }));
        thread::sleep(Duration::from_millis(150));
        assert!(!mark.exists());

        run(4, &c.id);
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 4, ok: true, error: String::new() });
        let until = Instant::now() + Duration::from_secs(5);
        while !mark.exists() && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(mark.exists());

        thread::sleep(Duration::from_millis(20));
        cmds.add(commands::Command::run("Second", "true")).unwrap();
        assert!(matches!(next_other(&conn, &mut reader), HostMsg::Commands { commands } if commands.len() == 2));

        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 시스템 관리 설정(strict): 관리 설정에 명시된 권한·명령만 덱에 전달되며, 사용자 설정 변경은 무시된다.
    /// 관리 설정 파일이 변경되면 연결 중에도 곧 반영된다.
    #[cfg(unix)]
    #[test]
    fn what_the_system_fixed_is_what_the_deck_gets() {
        let strict = |perms: &str| format!(r#"{{"strict":true,"permissions":[{perms}],"commands":[{{"id":"aaaa0001","name":"Fixed","run":"true"}}]}}"#);
        let Rig { store, dir, deck_key, conn, mut reader, hello_caps, host } = rig_with("managed", FakePlayer::default(), Some(&strict(r#""command.run""#)));
        commands::Commands::new(&dir).add(commands::Command::run("Mine", "true")).unwrap();
        assert_eq!(hello_caps, [CAP_COMMAND_RUN]);
        fn next_other(conn: &Conn, reader: &mut TcpStream) -> HostMsg {
            loop {
                match next_msg(conn, reader) {
                    HostMsg::Media { .. } | HostMsg::CommandValue { .. } => {}
                    m => return m,
                }
            }
        }
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Commands { commands: vec![toss_proto::CommandInfo::run("aaaa0001", "Fixed")] });
        conn.send(&DeckMsg::Action { id: 1, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null }).unwrap();
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 1, ok: false, error: ERROR_NOT_ALLOWED.into() });
        thread::sleep(Duration::from_millis(20));
        store.update(&deck_key, |d| { d.perms.insert(CAP_MEDIA_CONTROL.into(), true); }).unwrap();
        conn.send(&DeckMsg::Action { id: 2, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null }).unwrap();
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 2, ok: false, error: ERROR_NOT_ALLOWED.into() });
        let run = |n: u64, id: &str| conn.send(&DeckMsg::Action { id: n, name: ACTION_COMMAND_RUN.into(), args: serde_json::json!({"id": id}) }).unwrap();
        run(3, "aaaa0001");
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 3, ok: true, error: String::new() });
        let mine = commands::Commands::new(&dir).find("Mine").unwrap();
        run(4, &mine.id);
        assert!(matches!(next_other(&conn, &mut reader), HostMsg::Result { id: 4, ok: false, .. }));

        std::fs::write(dir.join("managed.json"), r#"{"strict":true,"permissions":["media.info"]}"#).unwrap();
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Caps { caps: vec![CAP_MEDIA_INFO.into()] });
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Commands { commands: Vec::new() });
        run(5, "aaaa0001");
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 5, ok: false, error: ERROR_NOT_ALLOWED.into() });

        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 스위치·노브 명령: 허용 시 종류·스펙과 현재 값이 덱에 전달되며, 덱의 값 설정 요청 시 커맨드가 실행되고 변경된 상태가 보고된다.
    /// 범위 초과 값은 클램프되고, 잘못된 단계나 명령 종류 불일치는 거부된다. 권한 해제 시 값 설정도 차단된다.
    #[cfg(unix)]
    #[test]
    fn switches_and_knobs_report_their_state_and_take_values() {
        use commands::Command;
        let Rig { store, dir, deck_key, conn, mut reader, host, .. } = rig("knobs", FakePlayer::default());
        let cmds = commands::Commands::new(&dir);
        let file = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            path.display().to_string()
        };
        let (mic, mode, vol) = (file("mic", "off\n"), file("mode", "passive\n"), file("vol", "20\n"));
        let sw = cmds.add(Command::switch("Mic", &format!("echo on > '{mic}'"), &format!("echo off > '{mic}'"), &format!("cat '{mic}'"))).unwrap();
        let st = cmds.add(Command::stepped("Mode", &[("off", ""), ("passive", ""), ("active", ""), ("auto", "")], &format!("echo {{}} > '{mode}'"), &format!("cat '{mode}'"))).unwrap();
        let li = cmds.add(Command::linear("Volume", (0.0, 100.0, 5.0), &format!("echo {{}} > '{vol}'"), &format!("cat '{vol}'"))).unwrap();
        let run = cmds.add(Command::run("Plain", "true")).unwrap();

        fn next_other(conn: &Conn, reader: &mut TcpStream) -> HostMsg {
            loop {
                match next_msg(conn, reader) {
                    HostMsg::Media { .. } => {}
                    m => return m,
                }
            }
        }
        fn value_of(conn: &Conn, reader: &mut TcpStream, id: &str, want: Option<f64>) {
            loop {
                match next_other(conn, reader) {
                    HostMsg::CommandValue { id: got, value } if got == id && value == want => return,
                    HostMsg::CommandValue { .. } | HostMsg::Result { ok: true, .. } => {}
                    other => panic!("예상하지 않은 메시지 수신: {other:?}"),
                }
            }
        }
        let set = |n: u64, id: &str, value: f64| conn.send(&DeckMsg::Action { id: n, name: ACTION_COMMAND_SET.into(), args: serde_json::json!({"id": id, "value": value}) }).unwrap();

        set(1, &sw.id, 1.0);
        assert_eq!(next_other(&conn, &mut reader), HostMsg::Result { id: 1, ok: false, error: ERROR_NOT_ALLOWED.into() });

        thread::sleep(Duration::from_millis(20));
        store.update(&deck_key, |d| { d.perms.insert(CAP_COMMAND_RUN.into(), true); }).unwrap();
        assert!(matches!(next_other(&conn, &mut reader), HostMsg::Caps { .. }));
        let HostMsg::Commands { commands: list } = next_other(&conn, &mut reader) else { panic!("Commands 메시지가 아님") };
        assert_eq!(list.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(), ["switch", "stepped", "linear", "run"]);
        assert_eq!((list[1].steps.len(), list[2].min, list[2].max, list[2].step), (4, 0.0, 100.0, 5.0));
        assert!(!serde_json::to_string(&list).unwrap().contains("echo"));
        let mut first = std::collections::HashMap::new();
        while first.len() < 3 {
            if let HostMsg::CommandValue { id, value } = next_other(&conn, &mut reader) {
                first.insert(id, value);
            }
        }
        assert_eq!((first[&sw.id], first[&st.id], first[&li.id]), (Some(0.0), Some(1.0), Some(20.0)));

        set(2, &sw.id, 1.0);
        value_of(&conn, &mut reader, &sw.id, Some(1.0));
        assert_eq!(std::fs::read_to_string(&mic).unwrap().trim(), "on");
        set(3, &st.id, 3.0);
        value_of(&conn, &mut reader, &st.id, Some(3.0));
        assert_eq!(std::fs::read_to_string(&mode).unwrap().trim(), "auto");
        set(4, &li.id, 1e9);
        value_of(&conn, &mut reader, &li.id, Some(100.0));
        set(5, &li.id, 42.0);
        value_of(&conn, &mut reader, &li.id, Some(40.0));
        assert_eq!(std::fs::read_to_string(&vol).unwrap().trim(), "40");

        std::fs::write(&mode, "off\n").unwrap();
        value_of(&conn, &mut reader, &st.id, Some(0.0));

        let refused = |conn: &Conn, reader: &mut TcpStream, n: u64| loop {
            match next_other(conn, reader) {
                HostMsg::Result { id, ok, .. } if id == n => return !ok,
                _ => {}
            }
        };
        set(6, &st.id, 9.0);
        assert!(refused(&conn, &mut reader, 6));
        set(7, &run.id, 1.0);
        assert!(refused(&conn, &mut reader, 7));
        set(8, "deadbeef", 1.0);
        assert!(refused(&conn, &mut reader, 8));
        conn.send(&DeckMsg::Action { id: 9, name: ACTION_COMMAND_RUN.into(), args: serde_json::json!({"id": sw.id}) }).unwrap();
        assert!(refused(&conn, &mut reader, 9));
        assert_eq!(std::fs::read_to_string(&mode).unwrap().trim(), "off");

        thread::sleep(Duration::from_millis(20));
        store.update(&deck_key, |d| { d.perms.insert(CAP_COMMAND_RUN.into(), false); }).unwrap();
        loop {
            if let HostMsg::Commands { commands } = next_other(&conn, &mut reader) {
                assert!(commands.is_empty());
                break;
            }
        }
        set(10, &sw.id, 0.0);
        assert!(refused(&conn, &mut reader, 10));
        thread::sleep(Duration::from_millis(200));
        assert_eq!(std::fs::read_to_string(&mic).unwrap().trim(), "on");

        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 리소스 측정: 카탈로그 전송 후 덱이 구독(watch)한 항목만 값이 전송된다(구독 항목이 없으면 측정하지 않음).
    /// 권한 해제 시 빈 카탈로그를 보내 덱 화면을 비우고 측정을 중단한다.
    #[cfg(target_os = "linux")]
    #[test]
    fn resources_are_measured_only_while_the_deck_watches_them() {
        let Rig { store, dir, deck_key, conn, mut reader, host, .. } = rig("meter", FakePlayer::default());
        fn next_meter(conn: &Conn, reader: &mut TcpStream) -> HostMsg {
            loop {
                if let Some(m @ (HostMsg::Metrics { .. } | HostMsg::MetricValues { .. })) = conn.recv::<HostMsg>(reader).unwrap() {
                    return m;
                }
            }
        }
        let HostMsg::Metrics { metrics } = next_meter(&conn, &mut reader) else { panic!("Metrics 메시지가 아님") };
        assert!(metrics.iter().any(|m| m.id == "cpu") && metrics.iter().any(|m| m.id == "mem" && m.unit == "B" && m.max > 0.0));

        reader.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let quiet_until = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < quiet_until {
            if let Ok(Some(HostMsg::MetricValues { values })) = conn.recv::<HostMsg>(&mut reader) {
                panic!("구독하지 않은 상태에서 측정값이 전송됨: {values:?}");
            }
        }
        reader.set_read_timeout(Some(Duration::from_secs(8))).unwrap();

        conn.send(&DeckMsg::Watch { metrics: vec!["mem".into(), "nope".into()], audio: false }).unwrap();
        let HostMsg::MetricValues { values } = next_meter(&conn, &mut reader) else { panic!("MetricValues 메시지가 아님") };
        assert_eq!(values.keys().collect::<Vec<_>>(), ["mem"]);
        assert!(values["mem"] > 0.0);

        thread::sleep(Duration::from_millis(20));
        store.update(&deck_key, |d| { d.perms.insert(CAP_RESOURCE_INFO.into(), false); }).unwrap();
        loop {
            if let HostMsg::Metrics { metrics } = next_meter(&conn, &mut reader) {
                assert!(metrics.is_empty());
                break;
            }
        }
        reader.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let quiet_until = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < quiet_until {
            if let Ok(Some(HostMsg::MetricValues { values })) = conn.recv::<HostMsg>(&mut reader) {
                panic!("권한 해제 후에도 측정값이 전송됨: {values:?}");
            }
        }
        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 미디어 정보는 이벤트 발생 시에만 전송된다(단순 시간 경과로는 전송하지 않고 일시정지·탐색·곡 변경 시 즉시 전송).
    #[test]
    fn media_is_sent_on_events_not_as_time_passes() {
        let player = FakePlayer::default();
        player.set("첫 곡", "playing", 10_000, "");
        let Rig { dir, conn, mut reader, host, .. } = rig("events", player.clone());
        let Some(first) = (match next_msg(&conn, &mut reader) {
            HostMsg::Media { media } => media,
            other => panic!("Media 메시지가 아님: {other:?}"),
        }) else {
            panic!("재생 정보가 비어 있음")
        };
        assert_eq!((first.title.as_str(), first.status.as_str(), first.rate), ("첫 곡", "playing", 1.0));
        assert!(first.position_ms >= 10_000 && first.position_ms < 11_000);

        let quiet_until = Instant::now() + READ_EVERY + Duration::from_millis(600);
        reader.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        while Instant::now() < quiet_until {
            match conn.recv::<HostMsg>(&mut reader) {
                Ok(Some(HostMsg::Ping | HostMsg::Metrics { .. })) | Ok(None) | Err(_) => {}
                Ok(Some(other)) => panic!("이벤트 없이 메시지가 전송됨: {other:?}"),
            }
        }
        reader.set_read_timeout(Some(Duration::from_secs(8))).unwrap();

        let asked = Instant::now();
        player.set("첫 곡", "paused", 13_000, "");
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(m) } if m.status == "paused" && m.position_ms == 13_000));
        assert!(asked.elapsed() < Duration::from_millis(700), "변경 알림 후 전송이 지연됨: {:?}", asked.elapsed());

        player.set("첫 곡", "paused", 90_000, "");
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(m) } if m.position_ms == 90_000));
        player.set("둘째 곡", "playing", 0, "");
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(m) } if m.title == "둘째 곡"));

        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 앨범 아트 전송: Media 메시지가 아트 ID를 먼저 싣고 가며 축소된 이미지는 청크로 뒤따라 간다.
    /// 동일 곡의 상태만 바뀌면 재전송하지 않고, 곡이 바뀌어 아트가 달라지면 새로 보낸다.
    #[test]
    fn album_art_follows_the_track_as_a_shrunk_picture_in_pieces() {
        use image::{ImageFormat, Rgb, RgbImage};
        let pics = std::env::temp_dir().join(format!("toss-connector-test-pics-{}", std::process::id()));
        std::fs::create_dir_all(&pics).unwrap();
        let noisy = |seed: u32, path: &std::path::Path| {
            let mut x = seed;
            let img = RgbImage::from_fn(900, 900, |_, _| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                Rgb([(x >> 24) as u8, (x >> 16) as u8, (x >> 8) as u8])
            });
            img.save_with_format(path, ImageFormat::Png).unwrap();
            format!("file://{}", path.display())
        };
        let (url_a, url_b) = (noisy(1, &pics.join("a.png")), noisy(2, &pics.join("b.png")));

        let player = FakePlayer::default();
        player.set("첫 곡", "paused", 0, &url_a);
        let Rig { dir, conn, mut reader, host, .. } = rig("art", player.clone());

        fn collect(conn: &Conn, reader: &mut TcpStream) -> (String, Vec<u8>) {
            let (mut joined, mut expect) = (String::new(), 0);
            loop {
                let HostMsg::Blob { id, mime, index, count, data } = next_msg(conn, reader) else { panic!("Blob 청크가 아님") };
                assert_eq!((mime.as_str(), index), ("image/jpeg", expect));
                assert!(index + 1 == count || data.len() == BLOB_CHUNK / 3 * 4, "중간 청크의 base64 길이가 다름");
                joined.push_str(&data);
                expect += 1;
                if expect == count {
                    return (id, base64::engine::general_purpose::STANDARD.decode(joined).unwrap());
                }
            }
        }

        let HostMsg::Media { media: Some(m) } = next_msg(&conn, &mut reader) else { panic!("Media 메시지가 아님") };
        assert_eq!(m.art, art::id_of(&url_a));
        let (id, bytes) = collect(&conn, &mut reader);
        assert_eq!(id, m.art);
        assert!(bytes.len() > BLOB_CHUNK, "단일 청크에 들어가 버림 ({}바이트)", bytes.len());
        let pic = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg).unwrap();
        assert_eq!((pic.width(), pic.height()), (512, 512));

        player.set("첫 곡", "playing", 0, &url_a);
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(m) } if m.status == "playing"));
        player.set("둘째 곡", "playing", 0, &url_b);
        let HostMsg::Media { media: Some(m) } = next_msg(&conn, &mut reader) else { panic!("아트가 중복 전송되었거나 Media 메시지가 아님") };
        assert_eq!((m.title.as_str(), m.art.as_str()), ("둘째 곡", art::id_of(&url_b).as_str()));
        assert_eq!(collect(&conn, &mut reader).0, m.art);

        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&pics);
    }

    /// 미등록 피어에게는 hello도 전송하지 않고 즉시 종료(호스트 정보 노출 방지)
    #[test]
    fn nothing_is_said_to_a_stranger() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let me = Identity::generate().unwrap();
        let host = thread::spawn(move || attach(&addr, &me, false, |_| None).unwrap().is_none());
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let (conn, mut reader) = Conn::establish(stream, &Identity::generate().unwrap(), false).unwrap();
        assert!(!matches!(conn.recv::<HostMsg>(&mut reader), Ok(Some(_))));
        assert!(host.join().unwrap());
    }

    /// 플레이어 응답이 지연되어 세션 스레드가 대기 중이어도 별도 스레드의 ping은 계속 전송되어 연결이 유지된다.
    #[test]
    fn a_slow_player_does_not_stop_the_keep_alive() {
        let player = FakePlayer::default();
        player.set("곡", "paused", 0, "");
        player.stall(toss_proto::DEAD_AFTER + Duration::from_millis(1000));
        let Rig { dir, conn, mut reader, host, .. } = rig("stall", player);
        let (mut last, mut longest, mut pings) = (Instant::now(), Duration::ZERO, 0);
        loop {
            let got = conn.recv::<HostMsg>(&mut reader).unwrap();
            longest = longest.max(last.elapsed());
            last = Instant::now();
            match got {
                Some(HostMsg::Ping) => pings += 1,
                Some(HostMsg::Media { .. }) => break,
                _ => {}
            }
        }
        assert!(pings >= 4, "플레이어 대기 중 ping 수신 횟수 부족: {pings}회");
        assert!(longest < toss_proto::DEAD_AFTER, "{longest:?} 동안 무응답 발생");
        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 손상된 decks.json: 모든 권한은 차단하되 페어링 삭제처럼 연결을 끊지는 않으며, 파일 복구 시 정상 동작으로 돌아온다.
    #[test]
    fn an_unreadable_record_denies_everything_but_is_not_a_forgotten_pairing() {
        let player = FakePlayer::default();
        player.set("곡", "paused", 0, "");
        let Rig { dir, conn, mut reader, hello_caps, host, .. } = rig("unreadable", player.clone());
        assert!(matches!(next_msg(&conn, &mut reader), HostMsg::Media { media: Some(_) }));
        let good = std::fs::read_to_string(dir.join("decks.json")).unwrap();

        std::fs::write(dir.join("decks.json"), "[{ 고치다 말았다").unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Caps { caps: Vec::new() });
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Media { media: None });
        conn.send(&DeckMsg::Action { id: 1, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null }).unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Result { id: 1, ok: false, error: ERROR_NOT_ALLOWED.into() });
        assert!(player.done().is_empty());
        assert!(!host.is_finished(), "파일 파싱 실패로 연결이 종료됨");

        std::fs::write(dir.join("decks.json"), good).unwrap();
        assert_eq!(next_msg(&conn, &mut reader), HostMsg::Caps { caps: hello_caps });
        conn.shutdown();
        assert_ne!(host.join().unwrap(), "이 PC에서 해당 덱의 페어링이 삭제되었습니다");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 권한 해제 즉시 다음 액션부터 거절된다(폴링 주기를 기다리지 않고 매 액션마다 설정을 재조회하기 때문).
    #[test]
    fn a_permission_turned_off_is_refused_at_the_very_next_action() {
        let player = FakePlayer::default();
        let Rig { store, dir, deck_key, conn, mut reader, host, .. } = rig("atonce", player.clone());
        for n in 0..20u64 {
            let on = n % 2 == 0;
            store.update(&deck_key, |d| { d.perms.insert(CAP_MEDIA_CONTROL.into(), on); }).unwrap();
            conn.send(&DeckMsg::Action { id: n, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null }).unwrap();
            loop {
                match next_msg(&conn, &mut reader) {
                    HostMsg::Result { id, ok, .. } if id == n => {
                        assert_eq!(ok, on, "{n}회차: 방금 변경된 권한이 즉시 반영되지 않음");
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(player.done().len(), 10);
        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 앨범 아트 다운로드 실패 후에도 동일 곡에서 재시도해 정상 전송되는지 확인
    #[test]
    fn album_art_is_fetched_again_after_a_failure() {
        use image::{ImageFormat, Rgb, RgbImage};
        let pics = std::env::temp_dir().join(format!("toss-connector-test-pics-late-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pics);
        std::fs::create_dir_all(&pics).unwrap();
        let url = format!("file://{}", pics.join("late.png").display());
        let player = FakePlayer::default();
        player.set("곡", "paused", 0, &url);
        let Rig { dir, conn, mut reader, host, .. } = rig("artlate", player);
        let HostMsg::Media { media: Some(m) } = next_msg(&conn, &mut reader) else { panic!("Media 메시지가 아님") };
        thread::sleep(Duration::from_millis(300));
        RgbImage::from_pixel(64, 64, Rgb([10, 200, 90])).save_with_format(pics.join("late.png"), ImageFormat::Png).unwrap();
        let HostMsg::Blob { id, .. } = next_msg(&conn, &mut reader) else { panic!("재시도된 앨범 아트가 전송되지 않음") };
        assert_eq!(id, m.art);
        conn.shutdown();
        let _ = host.join();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&pics);
    }

    /// 단일 메시지 크기 제한을 초과하는 명령 목록은 들어갈 수 있는 만큼 앞에서부터 잘라서 전송한다.
    #[test]
    fn a_command_list_too_big_for_one_message_is_cut_to_fit() {
        let fat = |n: usize| CommandInfo { kind: toss_proto::KIND_STEPPED.into(), steps: (0..12).map(|s| format!("단계의이름이길다{s:02}-{n:04}")).collect(), ..CommandInfo::run(&format!("{n:08x}"), &"이름".repeat(20)) };
        let all: Vec<CommandInfo> = (0..400).map(fat).collect();
        assert!(serde_json::to_vec(&HostMsg::Commands { commands: all.clone() }).unwrap().len() > MAX_PLAIN);
        let cut = fitting(all.clone());
        assert!(!cut.is_empty() && cut.len() < all.len() && cut[..] == all[..cut.len()]);
        let size = serde_json::to_vec(&HostMsg::Commands { commands: cut.clone() }).unwrap().len();
        assert!(size <= MAX_PLAIN && size + serde_json::to_vec(&all[cut.len()]).unwrap().len() > MAX_PLAIN - 64, "{size}");
        let few: Vec<CommandInfo> = (0..5).map(fat).collect();
        assert_eq!(fitting(few.clone()), few);
    }
}
