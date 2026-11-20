//! 호스트 관리: 덱과 페어링된 PC 연결 처리. 프로토콜 정의는 `toss-proto`(`src/app/proto`), 전체 설계는 `docs/04-host-link.md` 참고.
//!
//! 호스트의 `toss-connector`가 접속하면 먼저 Noise 핸드셰이크로 공개키를 교환하고 이후 통신을 암호화한다.
//!   - 페어링된 키면 연결을 수락한다. 호스트가 보내는 상태(재생 정보 등)를 받아 변경 시 UI에 알리고(`Events`),
//!     UI에서 요청한 액션을 호스트로 전달한다.
//!   - 미등록 키면 덱이 페어링 모드이고 호스트도 페어링 요청(`pair: true`)을 보냈을 때만 페어링 절차를 시작한다. 그 외에는 거절 후 종료한다.
//!
//! 여러 호스트가 동시에 연결될 수 있으며, 같은 호스트가 재접속하면 기존 연결을 닫고 새 연결로 교체한다
//! (다른 네트워크 인터페이스로 접속해도 공개키가 같으면 동일 호스트로 취급한다).
//!
//! 모듈 구성
//!   mod.rs      UI 전달 상태(`HostsState` 등), 콜백(`Events`), 호스트 상태 관리(`Hosts`)와 액션 전달, 리스너 시작(`spawn`)
//!   session.rs  개별 연결 세션: 핸드셰이크, 수락/거절 판단, 페어링된 호스트 메시지 수신 루프
//!   pairing.rs  페어링 모드 관리, 후보 상태, 양측 확인 처리
//!   blob.rs     청크 단위로 전송되는 앨범 아트 바이너리 조합
//!   store.rs    페어링 목록과 덱 키 영구 저장
//!   vault.rs    덱 금고: 외부 연동 키 보관, 토큰 발급·갱신과 대여, 금고를 쓸 수 있는 호스트 관리
//!
//! 미디어 상태는 호스트에서 이벤트(곡 변경, 재생/일시정지, 탐색 등)가 발생할 때만 전송된다. 수신 시각을 기록해 두고,
//! UI에 전달할 때마다 경과 시간만큼 보정한 `position_ms`를 계산해 넘긴다(이후 실시간 카운트는 UI 측에서 처리).
//! 앨범 아트는 `Blob` 청크로 수신되며, 모든 청크가 모이면 해당 호스트의 `art` 필드로 UI에 전달한다.
//!
//! 음량 믹서 상태(`audio`)와 레벨(`audio_levels`)도 각자의 채널(`Events::audio`, `Events::audio_levels`)로 알린다.
//! 레벨은 배치에 그 호스트를 가리키는 음량 믹서가 있을 때만 구독하므로(`Watch`의 `audio`), 구독하지 않는 호스트의 레벨은 비운다.
//!
//! 호스트에 등록된 스위치/노브의 값(`command_value`)은 호스트 목록과 별도 채널(`Events::values`)로 알린다 —
//! 노브 조작 중 값이 빈번하게 바뀔 때마다 앨범 아트 등이 포함된 `HostsState` 전체를 다시 보내지 않기 위함이다.
//!
//! 락 규칙: `inner` 뮤텍스를 잡은 상태에서는 호스트 소켓 전송이나 파일 쓰기를 하지 않는다(응답이 멈춘 호스트 때문에 전체가 블로킹되는 것을 방지).
//! 단, 새로 연결된 호스트에 초기 `Watch`를 보내는 경우만 예외로 락 내부에서 전송해 그사이에 바뀐 `Watch`와 순서가 역전되지 않게 한다.

mod blob;
mod pairing;
mod session;
mod store;
mod vault;

use std::collections::{BTreeMap, HashMap};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use toss_proto::secure::{Conn, Identity};
use toss_proto::{Audio, CommandInfo, DeckMsg, Media, MetricInfo, DENIED_FORGOTTEN};

use blob::Blob;
use pairing::Pending;
use vault::Vault;
pub use vault::VaultState;

/// `accept()` 실패 시 재시도 대기 시간(FD 고갈 시 CPU 점유 방지)
pub const ACCEPT_RETRY: Duration = Duration::from_millis(100);
/// 호스트당 응답 대기 중인 액션 최대 개수(초과 시 가장 오래된 항목 제거)
const ASKED_MAX: usize = 64;

/// UI로 전달하는 전체 호스트 상태
#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct HostsState {
    /// 페어링된 호스트 목록(오프라인 호스트 포함)
    pub hosts: Vec<HostInfo>,
    pub pairing: PairingInfo,
}

/// 페어링된 개별 호스트 상태
#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct HostInfo {
    /// 호스트 공개키(hex)
    pub id: String,
    pub name: String,
    pub os: String,
    /// 이 호스트가 덱에 허용한 권한 목록(`toss-proto`의 `CAP_*`). 연결된 경우에만 채워진다.
    pub caps: Vec<String>,
    /// 연결 경로: `usb` / `lan` / `wlan`. 오프라인이면 빈 문자열
    pub via: String,
    pub connected: bool,
    /// 현재 재생 중인 미디어 정보(없거나 오프라인이면 `null`). `position_ms`는 전달 시점 기준 보정 값이다.
    pub media: Option<Media>,
    /// 앨범 아트(없으면 `null`). 현재 곡의 이미지인지 여부는 `media.art`와 `id`를 비교해 확인한다.
    pub art: Option<Art>,
    /// 호스트에 등록된 명령 목록(ID, 이름, 종류 및 범위 메타데이터). 호스트가 허용한 경우에만 수신된다.
    pub commands: Vec<CommandInfo>,
}

impl HostInfo {
    /// 오프라인 호스트 기본 상태: 저장된 키, 이름, OS 정보만 포함한다.
    fn offline(id: String, name: String, os: String) -> Self {
        Self { id, name, os, caps: Vec::new(), via: String::new(), connected: false, media: None, art: None, commands: Vec::new() }
    }
}

/// 호스트에 등록된 스위치/노브의 현재 값: `호스트 ID -> 명령 ID -> 값`. 값을 모르는 항목은 포함되지 않는다.
pub type Values = BTreeMap<String, BTreeMap<String, f64>>;

/// 호스트 시스템 자원: `호스트 ID -> (메트릭 목록, 현재 값 맵)`. 온라인이면서 메트릭을 제공하는 호스트만 포함된다.
pub type Meters = BTreeMap<String, (Vec<MetricInfo>, BTreeMap<String, f64>)>;

/// 호스트의 음량 믹서 상태: `호스트 ID -> 상태`. 온라인이면서 상태를 보낸 호스트만 포함된다(`toss-proto`의 `Audio`).
pub type Audios = BTreeMap<String, Audio>;

/// 음량 믹서의 레벨 미터 값(0~1): 앱 ID와 출력 장치 ID마다 마지막으로 받은 값
#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct Levels {
    pub apps: BTreeMap<String, f64>,
    pub outputs: BTreeMap<String, f64>,
}

/// 호스트별 레벨: `호스트 ID -> 레벨`. 덱이 구독 중인(배치에 음량 믹서가 있는) 호스트만 포함된다.
pub type AudioLevels = BTreeMap<String, Levels>;

/// 호스트에 알리는 구독(`DeckMsg::Watch`): 표시 중인 메트릭과 음량 레벨 구독 여부
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Watch {
    pub metrics: Vec<String>,
    pub audio: bool,
}

/// 호스트가 전송한 이미지 데이터. `data`는 base64 문자열 그대로이며 UI에서 `data:` URI로 렌더링한다.
#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct Art {
    pub id: String,
    pub mime: String,
    pub data: String,
}

#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct PairingInfo {
    /// 페어링 모드 활성화 여부
    pub active: bool,
    /// 현재 페어링 대기 중인 호스트(없으면 `null`)
    pub candidate: Option<Candidate>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct Candidate {
    pub name: String,
    pub os: String,
    pub via: String,
    /// 양쪽 화면에 동일하게 표시되는 6자리 검증 코드(`"123 456"`). UI 수락 요청(`pair_accept`) 시 이 값을 그대로 전달한다.
    pub code: String,
    /// 호스트 측 사용자 확인 여부 / 덱 측 사용자 수락 여부
    pub host_confirmed: bool,
    pub deck_accepted: bool,
}

/// 호스트 모듈 이벤트를 수신하는 트레이트(데몬 코어인 `state.rs`의 `Shared`가 구현).
/// `hosts`, `values`, `meters`는 이벤트 순서 보장을 위해 내부 락을 잡은 상태에서 호출되므로 빠르게 상태만 복사하고 즉시 반환해야 한다.
pub trait Events: Send + Sync {
    /// 호스트 목록이나 페어링 진행 상태 변경
    fn hosts(&self, state: HostsState);
    /// 스위치/노브 값 변경(모든 호스트 전체 맵)
    fn values(&self, values: Values);
    /// 호스트 메트릭 목록 또는 값 변경(모든 호스트 전체 맵)
    fn meters(&self, meters: Meters);
    /// 음량 믹서 상태 변경(모든 호스트 전체 맵)
    fn audio(&self, audio: Audios);
    /// 음량 레벨 변경(모든 호스트 전체 맵)
    fn audio_levels(&self, levels: AudioLevels);
    /// 위젯이 요청한 액션에 대한 호스트 응답(요청 위젯 ID, 성공 여부, 실패 사유)
    fn result(&self, widget: u32, ok: bool, error: String);
    /// 호스트가 덱 절전을 요청함. 진입 불가 시 사유 코드(`toss-proto`의 `SLEEP_*`)를 반환한다.
    fn sleep(&self) -> Result<(), String>;
    /// 금고 상태 변경(서비스 키 등록·연결 여부, 허용한 호스트, 허용 여부를 묻는 중인 요청)
    fn vault(&self, state: VaultState);
}

struct Entry {
    info: HostInfo,
    /// 현재 세션 식별자. 뒤늦게 종료된 이전 세션이 새 세션의 상태를 덮어쓰지 않도록 막는다.
    session: u64,
    conn: Option<Arc<Conn>>,
    /// 미디어 상태 수신 시각(경과 시간만큼 재생 위치를 보정하는 기준점)
    media_at: Instant,
    /// 수신 중인 앨범 아트 청크 버퍼
    blob: Option<Blob>,
    /// 이 호스트의 스위치/노브 현재 값(`명령 ID -> 값`)
    values: BTreeMap<String, f64>,
    /// 이 호스트에서 측정 가능한 메트릭 목록과 현재 구독 중인 메트릭의 최신 값
    metrics: Vec<MetricInfo>,
    metric_values: BTreeMap<String, f64>,
    /// 음량 믹서 상태와 레벨
    audio: Option<Audio>,
    levels: Levels,
    /// 응답 대기 중인 액션 맵(`액션 ID -> 요청 위젯 ID`). 응답은 해당 액션을 보낸 호스트의 것만 매칭한다.
    asked: HashMap<u64, u32>,
}

impl Entry {
    fn new(id: String, name: String, os: String) -> Self {
        Self { info: HostInfo::offline(id, name, os), session: 0, conn: None, media_at: Instant::now(), blob: None, values: BTreeMap::new(), metrics: Vec::new(), metric_values: BTreeMap::new(), audio: None, levels: Levels::default(), asked: HashMap::new() }
    }

    /// 세션 종료 또는 교체 시 런타임 상태를 초기화한다(키, 이름, OS 정보만 유지).
    fn reset(&mut self) {
        self.info = HostInfo::offline(std::mem::take(&mut self.info.id), std::mem::take(&mut self.info.name), std::mem::take(&mut self.info.os));
        self.blob = None;
        self.values.clear();
        self.metrics.clear();
        self.metric_values.clear();
        self.audio = None;
        self.levels = Levels::default();
        self.asked.clear();
    }

    /// UI 전달용 스냅샷 생성: 경과 시간을 반영해 `position_ms`를 현재 시점으로 보정한다.
    fn shown(&self) -> HostInfo {
        let mut info = self.info.clone();
        if let Some(m) = &mut info.media {
            m.position_ms = m.position_after(self.media_at.elapsed().as_millis() as u64);
        }
        info
    }
}

struct Inner {
    entries: Vec<Entry>,
    /// 호스트별 구독: 메트릭 ID 목록과 음량 레벨 구독 여부(배치 내 자원 미터·음량 믹서 위젯에서 파생, `set_watch`)
    watch: BTreeMap<String, Watch>,
    pairing_until: Option<Instant>,
    pending: Option<Pending>,
}

pub struct Hosts {
    inner: Mutex<Inner>,
    next_session: AtomicU64,
    next_action: AtomicU64,
    identity: Identity,
    deck_name: String,
    /// 페어링된 호스트 목록 저장 파일 경로(`store.rs`)
    store: PathBuf,
    /// 파일 저장 직렬화용 뮤텍스
    saving: Mutex<()>,
    events: Arc<dyn Events>,
    /// 외부 연동 키 보관소(`vault.rs`)
    vault: Vault,
}

impl Hosts {
    fn publish(&self, inner: &Inner) {
        self.events.hosts(HostsState {
            hosts: inner.entries.iter().map(Entry::shown).collect(),
            pairing: PairingInfo { active: inner.pairing_until.is_some(), candidate: inner.pending.as_ref().map(|p| p.info.clone()) },
        });
    }

    fn publish_values(&self, inner: &Inner) {
        self.events.values(inner.entries.iter().filter(|e| !e.values.is_empty()).map(|e| (e.info.id.clone(), e.values.clone())).collect());
    }

    fn publish_meters(&self, inner: &Inner) {
        self.events.meters(inner.entries.iter().filter(|e| !e.metrics.is_empty()).map(|e| (e.info.id.clone(), (e.metrics.clone(), e.metric_values.clone()))).collect());
    }

    fn publish_audio(&self, inner: &Inner) {
        self.events.audio(inner.entries.iter().filter_map(|e| Some((e.info.id.clone(), e.audio.clone()?))).collect());
    }

    fn publish_levels(&self, inner: &Inner) {
        let watched = |e: &&Entry| inner.watch.get(&e.info.id).is_some_and(|w| w.audio) && e.audio.is_some();
        self.events.audio_levels(inner.entries.iter().filter(watched).map(|e| (e.info.id.clone(), e.levels.clone())).collect());
    }

    /// 덱이 모니터링 중인 호스트 메트릭 목록이나 음량 레벨 구독이 변경되었을 때(배치 수정 시) 변경된 호스트에만 `Watch`를 전송해
    /// 해당 항목만 측정하게 한다. 호출 측(`state.rs`의 `edit_layout`)에서 직렬화를 보장해야 한다.
    pub fn set_watch(&self, watch: BTreeMap<String, Watch>) {
        let changed: Vec<(Arc<Conn>, Watch)> = {
            let mut inner = self.inner.lock().unwrap();
            let changed = inner
                .entries
                .iter()
                .filter(|e| inner.watch.get(&e.info.id) != watch.get(&e.info.id))
                .filter_map(|e| Some((e.conn.clone()?, watch.get(&e.info.id).cloned().unwrap_or_default())))
                .collect();
            for e in &mut inner.entries {
                if !watch.get(&e.info.id).is_some_and(|w| w.audio) {
                    e.levels = Levels::default();
                }
            }
            inner.watch = watch;
            self.publish_levels(&inner);
            changed
        };
        for (conn, w) in changed {
            let _ = conn.send(&DeckMsg::Watch { metrics: w.metrics, audio: w.audio });
        }
    }

    /// 호스트에 액션을 전송한다. `host`가 비어 있으면 연결된 첫 번째 호스트로 보낸다. 전송 성공 시 `true`.
    /// `widget`이 지정되면 응답 결과를 해당 위젯 앞으로 전달한다(`Events::result`).
    pub fn action(&self, host: &str, name: &str, args: serde_json::Value, widget: Option<u32>) -> bool {
        let id = self.next_action.fetch_add(1, Ordering::SeqCst);
        let found = {
            let mut inner = self.inner.lock().unwrap();
            inner.entries.iter_mut().find(|e| e.info.connected && (host.is_empty() || e.info.id == host)).and_then(|e| {
                let conn = e.conn.clone()?;
                if let Some(widget) = widget {
                    e.asked.insert(id, widget);
                    if e.asked.len() > ASKED_MAX {
                        if let Some(oldest) = e.asked.keys().copied().min() {
                            e.asked.remove(&oldest);
                        }
                    }
                }
                Some((conn, e.session))
            })
        };
        let Some((conn, session)) = found else {
            eprintln!("host: 대상 호스트가 연결되어 있지 않음({name})");
            return false;
        };
        if let Err(e) = conn.send(&DeckMsg::Action { id, name: name.to_string(), args }) {
            eprintln!("host: 액션 전송 실패({name}): {e}");
            if let Some(entry) = self.inner.lock().unwrap().entries.iter_mut().find(|e| e.session == session) {
                entry.asked.remove(&id);
            }
            return false;
        }
        true
    }

    /// 현재 상태를 UI에 재전송한다(새 UI 클라이언트 접속 시 현재 시점의 재생 위치를 계산해 보내기 위함).
    pub fn refresh(&self) {
        self.publish(&self.inner.lock().unwrap());
    }

    /// 호스트 페어링을 삭제한다. 연결되어 있으면 즉시 종료한다.
    pub fn forget(&self, id: &str) {
        let gone = {
            let mut inner = self.inner.lock().unwrap();
            let Some(pos) = inner.entries.iter().position(|e| e.info.id == id) else { return };
            let gone = inner.entries.remove(pos);
            self.publish(&inner);
            self.publish_values(&inner);
            self.publish_meters(&inner);
            self.publish_audio(&inner);
            self.publish_levels(&inner);
            gone
        };
        self.save();
        self.vault.forget_host(id);
        if let Some(conn) = gone.conn {
            let _ = conn.send(&DeckMsg::Denied { reason: DENIED_FORGOTTEN.to_string() });
            conn.shutdown();
        }
        eprintln!("host: 페어링 삭제: {}", gone.info.name);
    }

    /// 연결된 모든 호스트에 각자의 금고 상태를 보낸다(허용 여부가 호스트마다 다르다).
    fn announce_vault(&self) {
        let conns: Vec<(String, Arc<Conn>)> = self.inner.lock().unwrap().entries.iter().filter_map(|e| Some((e.info.id.clone(), e.conn.clone()?))).collect();
        for (key, conn) in conns {
            let _ = conn.send(&DeckMsg::VaultState { services: self.vault.status_for(&key) });
        }
    }

    /// 화면에서 묻고 있는 금고 사용 요청에 답한다(`allow`가 true면 허용).
    pub fn vault_answer(&self, allow: bool) {
        self.vault.answer(allow);
    }

    /// 이 서비스에 대한 호스트의 금고 사용 허용을 해제한다.
    pub fn vault_revoke(&self, service: &str, host: &str) {
        self.vault.revoke(service, host);
        self.announce_vault();
    }

    /// 덱의 Spotify 모듈이 금고에서 토큰을 얻는 연결(`spotify.rs`)
    pub fn spotify_link(self: &Arc<Self>) -> crate::spotify::Link {
        let (a, b) = (self.clone(), self.clone());
        crate::spotify::Link {
            token: Box::new(move |stale| a.vault.own_token(toss_proto::SERVICE_SPOTIFY, stale)),
            linked: Box::new(move || b.vault.linked(toss_proto::SERVICE_SPOTIFY)),
        }
    }

    /// 이 서비스의 키와 토큰을 덱에서 삭제한다.
    pub fn vault_forget(&self, service: &str) {
        self.vault.forget(service);
        eprintln!("vault: 화면에서 {service} 키를 삭제함");
        self.announce_vault();
    }
}

/// 호스트 연결 수신 스레드를 시작한다. 발생한 상태 변경은 `events`로 알린다.
/// `addr` 바인딩에 실패하면 주기적으로 재시도한다. `data_dir`은 덱 키와 페어링 목록을 저장하는 경로다(`store.rs`).
pub fn spawn(addr: SocketAddr, data_dir: PathBuf, deck_name: String, events: Arc<dyn Events>) -> Arc<Hosts> {
    let store = data_dir.join("hosts.json");
    let to_ui = events.clone();
    let vault = Vault::open(&data_dir, move |state| to_ui.vault(state));
    let hosts = Arc::new(Hosts {
        inner: Mutex::new(Inner { entries: store::load(&store), watch: BTreeMap::new(), pairing_until: None, pending: None }),
        next_session: AtomicU64::new(1),
        next_action: AtomicU64::new(1),
        identity: store::load_identity(&data_dir),
        deck_name,
        store,
        saving: Mutex::new(()),
        events,
        vault,
    });
    {
        let inner = hosts.inner.lock().unwrap();
        eprintln!("host: 덱 ID {}..., 페어링된 호스트 {}개", &hosts.identity.id()[..12], inner.entries.len());
        hosts.publish(&inner);
    }
    hosts.events.vault(hosts.vault.state());

    let me = hosts.clone();
    thread::spawn(move || {
        let mut warned = false;
        let socket = loop {
            match TcpListener::bind(addr) {
                Ok(s) => break s,
                Err(e) => {
                    if !warned {
                        eprintln!("host: {addr}에서 리슨할 수 없습니다(재시도 예정): {e}");
                        warned = true;
                    }
                    thread::sleep(Duration::from_secs(3));
                }
            }
        };
        eprintln!("host: {}에서 호스트 연결 대기 중", socket.local_addr().map(|a| a.to_string()).unwrap_or_default());
        for stream in socket.incoming() {
            let Ok(stream) = stream else {
                thread::sleep(ACCEPT_RETRY);
                continue;
            };
            let me = me.clone();
            thread::spawn(move || {
                if let Err(e) = me.session(stream) {
                    eprintln!("host: 세션 종료: {e}");
                }
            });
        }
    });

    let me = hosts.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        me.expire_pairing();
    });
    hosts
}
