//! 덱과 호스트 간 통신 프로토콜. 전체 설계(페어링, 권한, 저장소, 확장 방법)는 docs/04-host-link.md 참고.
//! 이 코드가 기준 구현이므로 메시지를 수정하면 해당 문서의 4.6절도 같이 업데이트해야 한다.
//!
//! - 덱: 토스 프론트. 데몬 `tossd`가 접속을 수신한다.
//! - 호스트: 덱에 연결되는 PC. PC에서 실행되는 `toss-connector`가 덱에 접속한다.
//!
//! 연결은 호스트가 먼저 건다(PC가 리스닝하면 Windows·macOS 방화벽에 걸리기 때문). 연결된 뒤에는 양방향으로 자유롭게 전송한다.
//! 호스트는 상태(재생 중인 곡 등)가 바뀔 때마다 푸시하고, 덱은 액션(곡 넘기기 등)을 요청한다.
//!
//! 신원과 암호화 (secure 모듈)
//!   기기마다 키 쌍을 하나씩 가진다. 공개키가 곧 기기 ID이며, 연결 경로(USB, 이더넷, Wi-Fi)가 달라도 키가 같으면 동일 기기다.
//!   접속 직후 Noise 핸드셰이크로 공개키를 교환하며, 이후 모든 메시지는 암호화된다.
//!
//! 페어링
//!   서로의 공개키를 저장하는 과정으로, 등록된 피어만 연결을 허용한다(USB/LAN 구분 없음).
//!   1. 덱 설정에서 페어링 모드를 켠다.
//!   2. 호스트에서 `toss-connector pair`를 실행한다(hello의 pair = true).
//!   3. 양쪽에 동일한 6자리 숫자가 표시된다(중간자 개입 시 숫자가 달라짐).
//!   4. 사용자가 양쪽 숫자를 비교해 일치하면 덱에서 수락을 누르고(→ 덱에 임시 승인), 호스트에서도 확인한다(→ pair_confirm).
//!   5. 양쪽 모두 확인하면 덱이 paired를 보내고, 각자 상대방의 키를 저장한다.
//!
//! 프레이밍: TCP 스트림 위에 "길이(2바이트, big-endian) + 암호문"을 이어 보낸다. 복호화하면 JSON이 나오며 "t" 필드로 메시지 타입을 구분한다.
//! 알 수 없는 타입은 무시해 한쪽만 구버전이어도 연결이 끊기지 않게 한다.
//!
//! Keep-alive: 양쪽 모두 PING_EVERY 주기로 ping을 전송한다. DEAD_AFTER 동안 아무것도 오지 않으면 연결이 끊긴 것으로 간주한다.
//!
//! 상태는 이벤트 기반으로 변경 시에만 전송한다. 예를 들어 미디어 정보는 곡 변경, 재생/일시정지, 탐색(seek) 시에만 보내며,
//! 재생 중 경과 시간은 수신 측(덱)에서 자체 계산한다. 전송 여부는 호스트가 판단한다.
//!
//! 대용량 데이터(앨범 아트)는 단일 메시지에 들어가지 않으므로 청크(blob)로 분할해 보낸다. 청크 전송 사이에 다른 메시지가 끼어들 수 있다.
//!
//! 미구현: 한 호스트가 여러 경로를 동시에 유지하며 즉시 전환하는 기능(현재는 끊기면 저장된 주소 목록으로 재접속).

pub mod secure;

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 메시지를 추가할 때는 올리지 않는다(알 수 없는 메시지는 무시됨). 기존 메시지의 의미가 바뀔 때 올린다.
///   3: media의 position_ms가 "전송 시점 위치"로 변경(수신 측에서 계산). rate, art, blob 추가
pub const PROTO_VERSION: u32 = 3;
/// 덱이 호스트 접속을 수신하는 포트
pub const DEFAULT_PORT: u16 = 7879;
/// USB 연결 시 덱의 고정 IP 주소
pub const USB_DECK_ADDR: &str = "172.16.42.1";

pub const PING_EVERY: Duration = Duration::from_millis(500);
pub const DEAD_AFTER: Duration = Duration::from_millis(2000);
/// 덱 페어링 모드 유지 시간
pub const PAIRING_WINDOW: Duration = Duration::from_secs(120);

/// 호스트 → 덱
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HostMsg {
    /// 접속 직후 1회 전송. caps는 이 호스트가 해당 덱에 부여한 권한(CAP_*), pair는 페어링 요청 여부
    Hello {
        proto: u32,
        name: String,
        os: String,
        caps: Vec<String>,
        #[serde(default)]
        pair: bool,
    },
    /// 페어링: 호스트 사용자가 검증 숫자가 일치함을 확인함
    PairConfirm,
    /// 미디어 재생 정보. 상태 변경 이벤트(곡 변경, 재생/일시정지/정지, 탐색, 플레이어 생성/종료)가 있을 때만 전체 전송한다.
    /// 재생 중 경과 시간은 보내지 않고 수신 측에서 계산한다. 활성 플레이어가 없으면 null
    Media { media: Option<Media> },
    /// 대용량 데이터 청크(현재는 앨범 아트 전용). 같은 id의 청크를 index 0부터 count-1까지 순서대로 보낸다.
    /// data는 base64 문자열이며, 마지막이 아닌 청크는 원본 바이트 길이가 반드시 3의 배수(BLOB_CHUNK)여야 한다.
    /// 그래야 수신 측이 디코딩 없이 base64 문자열을 그대로 이어 붙여도 유효한 전체 base64가 된다.
    /// 데이터 용도는 id로 식별하며, media의 art 필드가 이 id를 참조한다.
    Blob {
        id: String,
        /// MIME 타입 (예: image/jpeg)
        mime: String,
        index: u32,
        count: u32,
        data: String,
    },
    /// 덱을 절전 모드로 전환(`toss-connector sleep` 실행 시 전송). 덱은 sleeping 응답 후 곧바로 절전에 들어간다.
    /// 절전 중인 덱은 Wi-Fi 매직 패킷으로만 깨울 수 있다(DeckMsg::Hello의 wake 필드 참고).
    Sleep,
    /// 호스트에 등록된 명령 목록(덱의 명령 위젯에서 선택 가능한 항목). command.run 권한이 있을 때만 전송한다.
    /// 접속 직후와 목록이 변경될 때마다 전체를 보낸다. 실제 실행 커맨드는 제외하고 id, 이름, 종류 및
    /// 종류별 스펙(단계 목록, 범위)만 전달한다. 목록 전송 직후에는 현재 알고 있는 상태 값(command_value)도 모두 보낸다.
    Commands { commands: Vec<CommandInfo> },
    /// 상태가 있는 명령(switch, stepped, linear)의 현재 값. 값이 바뀔 때와 목록 전송 직후에만 보낸다.
    /// 상태 조회는 호스트가 담당하며, 등록된 상태 확인 커맨드를 주기적으로 실행해 변경 시 알린다.
    ///   switch   0 = 꺼짐, 1 = 켜짐
    ///   stepped  단계 인덱스(0부터 시작, CommandInfo.steps 기준)
    ///   linear   범위 내 수치
    /// 상태를 알 수 없으면 null(상태 확인 커맨드 실패 또는 아직 값을 설정하지 않은 경우)
    CommandValue { id: String, value: Option<f64> },
    /// 호스트에서 측정 가능한 리소스 목록(CPU, 메모리, GPU, 디스크, 네트워크, 온도 등). resource.info 권한이 있을 때만 전송한다.
    /// 접속 직후와 목록이 바뀔 때(디스크 마운트 변경 등) 전체를 보내며, 권한을 끄면 빈 목록을 보낸다.
    Metrics { metrics: Vec<MetricInfo> },
    /// 리소스 현재 값(이름 → 값). 덱이 구독(watch) 중인 항목 중 값이 바뀐 것만 약 1초마다 전송한다.
    /// 덱이 아무것도 보고 있지 않으면 호스트는 측정 자체를 하지 않는다. 단위와 최댓값은 해당 항목의 MetricInfo를 따른다.
    MetricValues { values: std::collections::BTreeMap<String, f64> },
    /// 이 덱에 부여된 권한 변경(호스트 사용자가 연결 중 수정함). hello의 caps를 통째로 교체한다.
    Caps { caps: Vec<String> },
    /// 덱이 요청한 액션의 처리 결과. 권한이 없으면 error에 ERROR_NOT_ALLOWED가 담긴다.
    Result { id: u64, ok: bool, error: String },
    Ping,
    #[serde(other)]
    Unknown,
}

/// 덱 → 호스트
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum DeckMsg {
    /// hello 응답. status는 STATUS_* 중 하나이며, addrs는 덱에 접속 가능한 주소 목록(호스트가 저장해 재접속 시 사용)
    Hello {
        proto: u32,
        name: String,
        status: String,
        #[serde(default)]
        addrs: Vec<String>,
        /// 절전 중인 덱을 WoWLAN 매직 패킷으로 깨울 때 사용할 Wi-Fi MAC 주소("aa:bb:cc:dd:ee:ff"). 없으면 빈 문자열.
        /// 호스트가 저장해 둔다(`toss-connector wake`). 이더넷으로는 깨울 수 없고 Wi-Fi 매직 패킷만 동작한다.
        #[serde(default)]
        wake: String,
    },
    /// sleep 응답. ok가 true면 곧 절전에 들어가고, false면 reason에 사유(SLEEP_*)가 담긴다.
    Sleeping { ok: bool, reason: String },
    /// 페어링 완료(덱이 호스트 키를 저장함)
    Paired,
    /// 연결 거부. 전송 후 덱이 연결을 닫는다. reason은 DENIED_* 중 하나
    Denied { reason: String },
    /// 덱이 현재 화면에 표시 중인 리소스 목록(해당 호스트를 가리키는 리소스 위젯에서 선택된 항목 전체).
    /// 호스트는 여기에 포함된 항목만 측정해 metric_values로 보낸다. 접속 직후와 레이아웃 변경 시 전체를 전송하며, 비어 있으면 측정하지 않는다.
    Watch { metrics: Vec<String> },
    /// 액션 요청. id는 result 응답과 매칭하는 시퀀스 번호, name은 아래 ACTION_* 중 하나
    Action {
        id: u64,
        name: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    Ping,
    #[serde(other)]
    Unknown,
}

/// 덱의 호스트 수락 상태 (DeckMsg::Hello의 status)
pub const STATUS_PAIRED: &str = "paired";
pub const STATUS_PAIRING: &str = "pairing";
pub const STATUS_DENIED: &str = "denied";

pub const DENIED_NOT_PAIRED: &str = "not_paired";
pub const DENIED_BUSY: &str = "busy";
pub const DENIED_REJECTED: &str = "rejected";
pub const DENIED_TIMEOUT: &str = "timeout";
pub const DENIED_FORGOTTEN: &str = "forgotten";

/// 절전 불가 사유 (DeckMsg::Sleeping의 reason)
pub const SLEEP_NO_WIFI: &str = "no_wifi";
pub const SLEEP_UNAVAILABLE: &str = "unavailable";

/// 덱이 요청할 수 있는 액션 이름
pub const ACTION_MEDIA_PLAY_PAUSE: &str = "media.play_pause";
pub const ACTION_MEDIA_NEXT: &str = "media.next";
pub const ACTION_MEDIA_PREV: &str = "media.prev";
/// 탐색(seek). args: {"offset_ms": -5000} (현재 위치 기준 오프셋, 음수면 뒤로)
pub const ACTION_MEDIA_SEEK: &str = "media.seek";
/// 셔플 켜기/끄기. args: {"on": true}
pub const ACTION_MEDIA_SHUFFLE: &str = "media.shuffle";
/// 반복 모드 설정. args: {"mode": "none" | "playlist" | "track"} (LOOP_*)
pub const ACTION_MEDIA_LOOP: &str = "media.loop";

/// 호스트에 등록된 명령 실행. args: {"id": "<명령 id>"} (HostMsg::Commands로 받은 것 중 하나)
/// 덱은 실행 커맨드를 보내지 않으며, 호스트에 등록되지 않은 id는 실행되지 않는다.
pub const ACTION_COMMAND_RUN: &str = "command.run";
/// 호스트에 등록된 스위치·단계·범위 명령의 값 설정. args: {"id": "<명령 id>", "value": 1}
/// value 의미는 command_value와 동일하다(스위치 0/1, 단계는 인덱스, 범위는 수치). 호스트는 명령 종류와 범위를 검증하고,
/// 범위 값은 범위 및 단위(step)에 맞춰 보정한 뒤 그 값에 해당하는 등록된 커맨드를 실행한다.
/// result는 요청을 받아 실행을 위임했다는 뜻이며, 실제 반영 여부는 뒤따르는 command_value로 확인한다.
pub const ACTION_COMMAND_SET: &str = "command.set";

/// 반복 모드 (Media.loop_mode, media.loop의 mode)
pub const LOOP_NONE: &str = "none";
pub const LOOP_PLAYLIST: &str = "playlist";
pub const LOOP_TRACK: &str = "track";

/// 호스트가 덱에 부여하는 권한 (HostMsg::Hello, HostMsg::Caps의 caps)
/// 권한 범위는 호스트 측에서 덱별로 설정하며, 덱은 이를 조회만 할 수 있고 변경할 수 없다.
/// 호스트는 허용되지 않은 작업을 실행하지 않는다. 각 액션은 action_cap에 정의된 권한이 필요하며, 없으면 거절된다.
pub const CAP_MEDIA_INFO: &str = "media.info";
pub const CAP_MEDIA_CONTROL: &str = "media.control";
pub const CAP_RESOURCE_INFO: &str = "resource.info";
pub const CAP_COMMAND_RUN: &str = "command.run";

/// 권한이 없는 액션을 요청했을 때 반환되는 result의 error
pub const ERROR_NOT_ALLOWED: &str = "not_allowed";

/// 해당 액션에 필요한 권한을 반환한다. 알 수 없는 액션이면 None(호스트가 실행하지 않음)
pub fn action_cap(action: &str) -> Option<&'static str> {
    match action {
        ACTION_MEDIA_PLAY_PAUSE | ACTION_MEDIA_NEXT | ACTION_MEDIA_PREV | ACTION_MEDIA_SEEK | ACTION_MEDIA_SHUFFLE | ACTION_MEDIA_LOOP => Some(CAP_MEDIA_CONTROL),
        ACTION_COMMAND_RUN | ACTION_COMMAND_SET => Some(CAP_COMMAND_RUN),
        _ => None,
    }
}

/// 측정 가능한 리소스 항목 (HostMsg::Metrics). 덱 자체 리소스도 동일한 구조체를 사용한다.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MetricInfo {
    /// 고유 식별자. metric_values와 watch에서 이 키로 참조한다. 예: "cpu", "mem", "gpu.temp", "fs.root"
    pub id: String,
    /// 미터 옆에 표시할 짧은 이름 (5자 이내). 예: "CPU", "VRAM"
    pub name: String,
    /// 선택 화면에 표시할 전체 이름. 예: "CPU usage"
    pub title: String,
    /// 단위. "%", "°C", "GHz", "W", "RPM"은 값 뒤에 그대로 붙이고,
    /// "B"와 "B/s"는 수신 측에서 크기에 맞게 KB·MB·GB로 변환해 표시한다. 빈 문자열이면 단위 없음(부하 등).
    pub unit: String,
    /// 최대 눈금 값(게이지가 100%가 되는 기준). 0이면 상한 없음(수신 측이 최근 관측된 최댓값에 맞춰 스케일링)
    #[serde(default)]
    pub max: f64,
    /// 표시할 소수점 자릿수
    #[serde(default)]
    pub decimals: u8,
}

/// 명령 종류 (CommandInfo.kind). 종류에 따라 덱에 표시되는 위젯이 달라진다.
pub const KIND_RUN: &str = "run";
pub const KIND_SWITCH: &str = "switch";
pub const KIND_STEPPED: &str = "stepped";
pub const KIND_LINEAR: &str = "linear";

/// 호스트에 등록된 명령 메타데이터(실제 실행 커맨드는 제외하고 덱에 전달)
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CommandInfo {
    /// 호스트가 부여한 고유 ID. 이름을 바꿔도 유지되므로 덱 위젯이 이 ID로 참조한다.
    pub id: String,
    /// 사용자 지정 이름
    pub name: String,
    /// 명령 종류 (KIND_*). 생략된 구버전 호스트 데이터는 run으로 처리
    #[serde(default = "default_kind")]
    pub kind: String,
    /// stepped: 단계 이름 목록 (순서대로, 값은 0-based 인덱스)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
    /// linear: 범위와 단위 (min부터 step 간격의 값만 허용)
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub step: f64,
}

fn default_kind() -> String {
    KIND_RUN.to_string()
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl CommandInfo {
    /// 단발성 실행 명령 (run)
    pub fn run(id: &str, name: &str) -> Self {
        Self { id: id.into(), name: name.into(), kind: default_kind(), steps: Vec::new(), min: 0.0, max: 0.0, step: 0.0 }
    }
}

/// 청크 하나에 담는 원본 데이터 크기(3의 배수). base64 인코딩 시 40,000자가 되어 단일 메시지에 여유롭게 들어간다.
pub const BLOB_CHUNK: usize = 30_000;
/// 청크들을 합친 전체 데이터 최대 크기(원본 기준). 초과하면 수신 측에서 폐기한다.
pub const BLOB_MAX: usize = 1024 * 1024;
/// 앨범 아트 최대 변 길이(px). 이를 초과하는 이미지는 호스트가 축소해 전송한다.
pub const ART_MAX_PX: u32 = 512;

/// 미디어 재생 정보
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Media {
    /// 플레이어 이름 (예: spotify)
    pub player: String,
    /// playing / paused / stopped
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// 트랙 길이 (ms). 알 수 없으면 0
    pub length_ms: u64,
    /// 메시지 전송 시점의 재생 위치(ms). 수신 측은 status가 playing인 동안 여기서부터 rate 배속으로 계산한다.
    pub position_ms: u64,
    /// 재생 속도(기본 1.0). 0이면 수신 측에서 시간을 진행하지 않는다(위치를 보고하지 않는 플레이어용).
    #[serde(default = "default_rate")]
    pub rate: f64,
    pub can_next: bool,
    pub can_prev: bool,
    /// 위치 탐색(media.seek) 가능 여부
    #[serde(default)]
    pub can_seek: bool,
    /// 셔플 활성화 여부. 플레이어가 지원하지 않으면 null (media.shuffle 요청 불가)
    #[serde(default)]
    pub shuffle: Option<bool>,
    /// 반복 모드 (LOOP_*). 플레이어가 지원하지 않으면 null (media.loop 요청 불가)
    #[serde(default)]
    pub loop_mode: Option<String>,
    /// 앨범 아트 ID(없으면 빈 문자열). 실제 이미지 데이터는 동일 ID의 blob 메시지로 별도 전송되며 먼저 또는 나중에 도착할 수 있다.
    #[serde(default)]
    pub art: String,
}

fn default_rate() -> f64 {
    1.0
}

impl Default for Media {
    fn default() -> Self {
        Self {
            player: String::new(),
            status: String::new(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            length_ms: 0,
            position_ms: 0,
            rate: 1.0,
            can_next: false,
            can_prev: false,
            can_seek: false,
            shuffle: None,
            loop_mode: None,
            art: String::new(),
        }
    }
}

impl Media {
    /// 전송 후 elapsed_ms가 경과했을 때의 예상 위치. 재생 중이면 rate 배속으로 증가하며 곡 길이를 넘지 않는다.
    pub fn position_after(&self, elapsed_ms: u64) -> u64 {
        if self.status != "playing" {
            return self.position_ms;
        }
        let now = self.position_ms as f64 + elapsed_ms as f64 * self.rate.max(0.0);
        let now = if self.length_ms > 0 { now.min(self.length_ms as f64) } else { now };
        now as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_messages_are_tolerated() {
        let m: DeckMsg = serde_json::from_str(r#"{"t":"from_the_future","x":1}"#).unwrap();
        assert_eq!(m, DeckMsg::Unknown);
        let m: HostMsg = serde_json::from_str(r#"{"t":"hello","proto":3,"name":"pc","os":"linux","caps":[]}"#).unwrap();
        assert!(matches!(m, HostMsg::Hello { pair: false, .. }));
    }

    #[test]
    fn action_args_are_optional() {
        let m: DeckMsg = serde_json::from_str(r#"{"t":"action","id":7,"name":"media.next"}"#).unwrap();
        assert_eq!(m, DeckMsg::Action { id: 7, name: ACTION_MEDIA_NEXT.into(), args: serde_json::Value::Null });
    }

    #[test]
    fn position_runs_only_while_playing_and_stops_at_the_end() {
        let m = Media { status: "playing".into(), position_ms: 10_000, length_ms: 60_000, ..Media::default() };
        assert_eq!(m.position_after(5_000), 15_000);
        assert_eq!(m.position_after(500_000), 60_000);
        assert_eq!(Media { rate: 1.5, ..m.clone() }.position_after(4_000), 16_000);
        assert_eq!(Media { rate: 0.0, ..m.clone() }.position_after(4_000), 10_000);
        assert_eq!(Media { status: "paused".into(), ..m.clone() }.position_after(4_000), 10_000);
        assert_eq!(Media { length_ms: 0, ..m }.position_after(90_000), 100_000);
    }

    #[test]
    fn media_from_before_rate_and_art_still_reads() {
        let m: Media = serde_json::from_str(
            r#"{"player":"p","status":"playing","title":"t","artist":"","album":"","length_ms":1,"position_ms":0,"can_next":true,"can_prev":false}"#,
        )
        .unwrap();
        assert_eq!((m.rate, m.art.as_str()), (1.0, ""));
        assert_eq!((m.can_seek, m.shuffle, m.loop_mode), (false, None, None));
    }

    #[test]
    fn commands_from_before_kinds_are_plain_run_commands() {
        let c: CommandInfo = serde_json::from_str(r#"{"id":"a1","name":"Terminal"}"#).unwrap();
        assert_eq!(c, CommandInfo::run("a1", "Terminal"));
        assert_eq!(serde_json::to_string(&c).unwrap(), r#"{"id":"a1","name":"Terminal","kind":"run"}"#);
        let knob = CommandInfo { kind: KIND_LINEAR.into(), max: 100.0, step: 5.0, ..CommandInfo::run("b2", "Volume") };
        assert_eq!(serde_json::from_str::<CommandInfo>(&serde_json::to_string(&knob).unwrap()).unwrap(), knob);
        let m: HostMsg = serde_json::from_str(r#"{"t":"command_value","id":"b2","value":null}"#).unwrap();
        assert_eq!(m, HostMsg::CommandValue { id: "b2".into(), value: None });
    }

    #[test]
    fn every_action_needs_a_cap_and_unknown_ones_have_none() {
        for a in [ACTION_MEDIA_PLAY_PAUSE, ACTION_MEDIA_NEXT, ACTION_MEDIA_PREV, ACTION_MEDIA_SEEK, ACTION_MEDIA_SHUFFLE, ACTION_MEDIA_LOOP] {
            assert_eq!(action_cap(a), Some(CAP_MEDIA_CONTROL));
        }
        assert_eq!(action_cap(ACTION_COMMAND_RUN), Some(CAP_COMMAND_RUN));
        assert_eq!(action_cap(ACTION_COMMAND_SET), Some(CAP_COMMAND_RUN));
        assert_eq!(action_cap("exec"), None);
        assert_eq!(action_cap(""), None);
    }
}
