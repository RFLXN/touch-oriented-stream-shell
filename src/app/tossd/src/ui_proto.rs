//! 화면(QML 클라이언트)과 주고받는 WebSocket JSON 프로토콜.
//!
//! 데몬 -> 클라이언트
//!   {"type":"hello","version":"0.1.0"}                       접속 직후 1회 전송
//!   {"type":"catalog","apps":[{"id":"media","title":..,"widgets":[{"kind":"media.player","w":4,"h":2,"settings":[..]}]}]}
//!                                                            추가 가능한 앱/위젯 카탈로그(접속 직후 1회. `layout`의 카탈로그)
//!   {"type":"layout","cols":4,"rows":6,"pages":[..]}         위젯 배치(아래 "배치" 참고). 접속 직후 및 배치 변경 시마다 전송
//!   {"type":"state","host":..,"ip":..,"links":{..},"net":{..}}   시스템 상태: 호스트명, IP, 인터페이스별 링크 및 상세 정보
//!                                                            (변경 시마다 전송. `sys.rs`의 `SysState`)
//!   {"type":"result","widget":3,"seq":12,"ok":true,"error":""}   위젯이 호스트에 요청한 액션 결과. `seq`는 결과마다 증가한다
//!                                                            (UI는 번호가 아니라 수신 시점을 보고 최근 실패만 1회 표시한다). 전송 자체를 못했을 때도 전달된다(`ok: false`).
//!   {"type":"device","brightness":200,"brightness_min":10,"brightness_max":255,"can_power":true,"ssh":true,"can_ssh":true}
//!                                                            기기 상태: 밝기와 허용 범위, 전원/절전 가능 여부, SSH 동작/제어 가능 여부(변경 시마다 전송. `device.rs`)
//!   {"type":"wifi","available":true,"phase":..,"networks":[..],...}   Wi-Fi 상태(변경 시마다 전송. `wifi.rs`의 `WifiState`)
//!   {"type":"hosts","hosts":[{"id":..,"name":..,"connected":true,"via":"usb","media":{..}},..],
//!    "pairing":{"active":false,"candidate":null}}            페어링된 호스트(PC) 목록과 상태, 페어링 진행 상태
//!                                                            (변경 시마다 전송. `hosts/mod.rs`의 `HostsState`, `media`는 `toss-proto`의 `Media`).
//!                                                            `media.position_ms`는 전송 시점 기준 위치이며, 재생 중이면 UI가 거기서부터 경과 시간을 계산한다.
//!                                                            호스트마다 `"art":{"id","mime","data"(base64)}`로 앨범 아트가 포함된다(없으면 `null`).
//!                                                            호스트마다 `"commands":[{"id","name","kind":"run|switch|stepped|linear","steps":[..],"min","max","step"}]`가 포함된다
//!                                                            (해당 호스트에 등록된 명령 목록. `toss-proto`의 `CommandInfo`).
//!   {"type":"values","hosts":{"<호스트 id>":{"<명령 id>":1}}} 호스트에 등록된 스위치/노브의 현재 값(변경 시 전체 전송).
//!                                                            스위치는 0/1, stepped는 인덱스(0부터), linear는 수치 값이다. 값을 모르는 항목은 생략된다.
//!   {"type":"meters","hosts":{"@deck":[{"id":"cpu","name":"CPU","title":"CPU usage","unit":"%","max":100,"decimals":0},..],"<호스트 id>":[..]}}
//!                                                            모니터링 가능한 리소스 목록. `"@deck"`은 덱 자신, 나머지는 리소스를 제공하는 호스트(PC)다
//!                                                            (변경 시 전체 전송. 형식은 `toss-proto`의 `MetricInfo`).
//!   {"type":"meter_values","hosts":{"@deck":{"cpu":12,"mem":301989888},"<호스트 id>":{..}}}
//!                                                            리소스 현재 값. **자원 미터 위젯이 현재 표시 중인 항목만** 포함된다(약 1초 간격, 변경 시 전송).
//!   {"type":"vault","services":[{"id":"discord","title":"Discord","registered":true,"linked":true,"hosts":["<호스트 id>"]}],
//!    "asking":{"host":"<호스트 id>","name":"pc","service":"discord","title":"Discord"}}
//!                                                            덱 금고 상태(키 값은 포함하지 않음. `hosts/vault.rs`의 `VaultState`). `registered`는 서비스 키 보관 여부,
//!                                                            `linked`는 토큰을 갱신할 수 있는 상태인지, `hosts`는 금고를 쓰도록 허용한 호스트다.
//!                                                            `asking`은 허용하지 않은 호스트가 금고를 쓰려 해서 화면에서 답을 기다리는 요청이다(없으면 `null`).
//!   {"type":"icons","icons":{"https://example.com/":"data:image/png;base64,.."}}
//!                                                            웹페이지 아이콘 버튼(`web.icon`)이 표시할 사이트 아이콘(주소 -> `data:` URI. 가져오지 못한 주소는 생략).
//!                                                            덱이 직접 가져와 보관한다(`icons.rs`). 변경 시 전체 전송
//! 클라이언트 -> 데몬
//!   {"type":"brightness","value":120,"keep":true}            화면 밝기 변경. `keep`이 true이면 영구 저장해 다음 부팅 시에도 복원한다
//!                                                            (드래그 중에는 `keep` 없이 보내고, 손을 뗄 때 `keep: true`로 보낸다).
//!   {"type":"power_off"}                                     덱 전원 끄기
//!   {"type":"ssh","on":false}                                SSH 끄기/켜기(`on`이 true면 켜기). 끄면 기존 세션도 종료되며
//!                                                            재부팅이나 재플래싱 후에도 비활성 상태가 유지된다. 결과는 `device.ssh`로 반영된다.
//!   {"type":"sleep"}                                         덱 절전 진입. Wi-Fi 연결 시에만 진입 가능하다(깨우는 수단이 Wi-Fi 매직 패킷뿐이기 때문).
//!   {"type":"wifi_scan"}                                     주변 AP 재스캔
//!   {"type":"wifi_connect","key":"686f6d65","psk":"..."}     AP 연결. `key`는 `wifi` 상태의 `networks[].key`.
//!                                                            `psk`를 생략하면 저장된 정보로 연결하거나 개방형 AP에 연결한다.
//!   {"type":"wifi_forget","key":"686f6d65"}                  해당 AP의 저장된 접속 정보 삭제
//!   {"type":"action","host":"","name":"media.next"}          호스트에 액션 전송. `host`는 호스트 ID(빈 문자열이면 연결된 첫 번째 호스트),
//!                                                            `name`은 `toss-proto`의 `ACTION_*`, `args`는 부가 인자(선택).
//!                                                            `widget`에 위젯 ID를 지정하면 해당 위젯으로 `result`가 반환된다.
//!                                                            스위치/노브 값 설정: `"name":"command.set","args":{"id":"<명령 id>","value":1}`
//!   {"type":"pair_start"} / {"type":"pair_stop"}             페어링 모드 시작 / 종료
//!   {"type":"pair_accept","code":"123 456"}                  페어링 요청 수락(검증 코드 일치). `code`는 화면에 표시된 숫자
//!                                                            (`hosts.pairing.candidate.code`)이며, 현재 후보와 다르면 데몬이 무시한다.
//!   {"type":"pair_reject"}                                   페어링 요청 거절
//!   {"type":"host_forget","id":".."}                         해당 호스트의 페어링 삭제
//!   {"type":"vault_answer","allow":true}                     `vault.asking`에 답한다(`allow`가 true면 그 호스트가 금고를 쓰도록 허용)
//!   {"type":"vault_revoke","service":"discord","host":".."}  호스트의 금고 사용 허용 해제
//!   {"type":"vault_forget","service":"discord"}              서비스 키와 토큰을 덱에서 삭제
//!   배치 편집(UI 편집 모드. `target`은 `"widget"`(페이지에 놓인 복합 위젯) 또는 `"rack"`). 데몬은 규칙에 맞는 요청만 수락해
//!   저장한 뒤 모든 클라이언트에 새 `layout`을 브로드캐스트한다. 규칙 위반이거나 필드가 잘못된(누락/타입 오류) `layout_*` 메시지는 무시하고
//!   현재 `layout`을 다시 내려 보내 UI 상태를 바로잡는다.
//!   {"type":"layout_move","target":"widget","id":2,"page":0,"x":2,"y":2}   이동. `page`가 현재 페이지 수와 같으면 새 페이지로 이동
//!   {"type":"layout_resize","id":1,"w":4,"h":1}              랙 크기 변경
//!   {"type":"layout_rename","id":1,"label":"TOOLS"}          랙 라벨 변경
//!   {"type":"layout_add","what":"media.player","page":0,"settings":{"host":".."}}   추가. `what`은 위젯 종류 또는 `"rack"`. 첫 빈자리에 배치
//!   {"type":"layout_put","rack":1,"x":0,"y":0,"what":"command.run","settings":{"host":"..","command":"..","label":"TERM","color":"green"}}
//!                                                            랙 내 특정 칸에 버튼 위젯 추가
//!   {"type":"layout_remove","target":"rack","id":1}          삭제. `target`이 `"button"`이면 랙 내부 버튼 위젯 삭제
//!   {"type":"layout_tidy"}                                   빈 페이지 정리(편집 모드 종료 시)
//!   {"type":"layout_reset"}                                  기본 배치로 초기화
//!
//! 용어(`docs/03-userland-guide.md` 3.16절 참고)
//!   타일       위젯을 배치하는 그리드 칸. 페이지마다 4x6으로 구성(`GRID_COLS` x `GRID_ROWS`)
//!   페이지     타일 그리드 한 판. 좌우 스와이프로 전환
//!   앱         기능 단위(예: Media). 앱마다 1종 이상의 위젯을 제공
//!   위젯 종류  앱이 제공하는 위젯 타입(예: `media.player`). 점유하는 타일 크기가 정해져 있음
//!   위젯       위젯 종류의 인스턴스. 개별 설정(예: 대상 PC)을 가짐
//!   랙         버튼 위젯을 담는 컨테이너(폴더/그룹 역할). 여러 타일을 차지함
//!
//! 배치 규칙
//!   - 버튼 위젯(`command.*`: 버튼, 스위치, 노브)은 반드시 랙 내부에 놓인다.
//!   - 복합 위젯(`media.player`, `monitor.*`)은 랙과 동등한 수준으로 페이지에 직접 배치되며, 내부에 다른 위젯을 담을 수 없다.
//!
//! 배치 형식: 페이지마다 랙과 복합 위젯 배열을 둔다.
//!   {"name":"Main",
//!    "racks":[{"id":1,"label":"STREAM","x":0,"y":2,"w":4,"h":2,
//!              "widgets":[{"id":5,"kind":"command.switch","x":0,"y":0,"w":1,"h":1,"label":"REC","color":"red","settings":{"host":"..","command":".."}}, ..]}],
//!    "widgets":[{"id":1,"kind":"media.player","x":0,"y":0,"w":4,"h":2,"label":"MEDIA","color":"","settings":{"host":".."}}, ..]}
//!   kind      위젯 종류: `command.run`(단발 실행 버튼: 호스트에 등록된 명령 실행, settings의 `host`와 `command`)
//!             / `command.switch`(스위치) / `command.stepped`(단계 노브) / `command.linear`(연속 노브): 호스트에 등록된
//!               상태형 명령을 표시하고 제어(settings는 `command.run`과 동일) — 이상은 랙 내부에 배치
//!             / `media.player`(미디어 제어) / `monitor.2x1` · `2x2` · `4x1` · `4x2`(자원 미터: settings의 `host`는 대상 기기 — `"@deck"`이면 덱 자신,
//!               아니면 호스트 ID — `metrics`는 표시할 리소스 ID 배열) — 이상은 복합 위젯
//!   settings  위젯 개별 설정. `media.player`는 `host`(대상 호스트 ID. 없거나 비어 있으면 연결된 아무 호스트)
//!   x, y      좌상단 타일 좌표(0부터 시작), `w`, `h`는 점유 타일 크기.
//!             랙과 복합 위젯은 페이지 기준 좌표이고, 랙 내부 위젯은 해당 랙의 좌상단이 `(0, 0)`이다.
//!   color     버튼 색상 이름(UI `Theme.qml`의 `buttonColors`). 버튼 위젯에만 사용
//!
//! 배치 타입과 규칙은 `layout`에, 시스템 상태는 `sys.rs`에, Wi-Fi는 `wifi.rs`에, 호스트 관리는 `hosts`에 있다.
//! (호스트와 주고받는 프로토콜은 별도 크레이트인 `toss-proto`에 정의되어 있다.)

use serde::{Deserialize, Serialize};

use crate::device::DeviceState;
use crate::hosts::{HostsState, Values, VaultState};
use crate::icons::IconMap;
use crate::layout::{AppInfo, Edit, Layout, Target};
use crate::state::{MeterInfos, MeterValues};
use crate::sys::SysState;
use crate::wifi::WifiState;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg<'a> {
    Hello { version: &'a str },
    Catalog { apps: &'a [AppInfo] },
    Layout(&'a Layout),
    State(&'a SysState),
    Result { widget: u32, seq: u64, ok: bool, error: &'a str },
    Wifi(&'a WifiState),
    Device(&'a DeviceState),
    Hosts(&'a HostsState),
    Values { hosts: &'a Values },
    Meters { hosts: &'a MeterInfos },
    MeterValues { hosts: &'a MeterValues },
    Vault(&'a VaultState),
    Icons { icons: &'a IconMap },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Brightness {
        value: u32,
        #[serde(default)]
        keep: bool,
    },
    PowerOff,
    Sleep,
    Ssh { on: bool },
    WifiScan,
    WifiConnect { key: String, psk: Option<String> },
    WifiForget { key: String },
    Action {
        #[serde(default)]
        host: String,
        name: String,
        #[serde(default)]
        args: serde_json::Value,
        /// 액션을 요청한 위젯(해당 위젯 앞으로 `result`가 반환된다)
        #[serde(default)]
        widget: Option<u32>,
    },
    PairStart,
    PairStop,
    PairAccept { code: String },
    PairReject,
    HostForget { id: String },
    VaultAnswer { allow: bool },
    VaultRevoke { service: String, host: String },
    VaultForget { service: String },
    LayoutMove { target: Target, id: u32, page: usize, x: u32, y: u32 },
    LayoutResize { id: u32, w: u32, h: u32 },
    LayoutRename { id: u32, label: String },
    LayoutAdd {
        what: String,
        #[serde(default)]
        page: usize,
        #[serde(default)]
        settings: serde_json::Map<String, serde_json::Value>,
    },
    LayoutPut {
        rack: u32,
        x: u32,
        y: u32,
        what: String,
        #[serde(default)]
        settings: serde_json::Map<String, serde_json::Value>,
    },
    LayoutRemove { target: Target, id: u32 },
    LayoutTidy,
    LayoutReset,
}

impl ClientMsg {
    /// 배치 편집 메시지(`layout_*`)면 대응하는 `Edit`으로 변환하고, 아니면 그대로 반환한다.
    pub fn into_edit(self) -> Result<Edit, Self> {
        Ok(match self {
            ClientMsg::LayoutMove { target, id, page, x, y } => Edit::Move { target, id, page, x, y },
            ClientMsg::LayoutResize { id, w, h } => Edit::Resize { id, w, h },
            ClientMsg::LayoutRename { id, label } => Edit::Rename { id, label },
            ClientMsg::LayoutAdd { what, page, settings } => Edit::Add { what, page, settings },
            ClientMsg::LayoutPut { rack, x, y, what, settings } => Edit::Put { rack, x, y, what, settings },
            ClientMsg::LayoutRemove { target, id } => Edit::Remove { target, id },
            ClientMsg::LayoutTidy => Edit::Tidy,
            ClientMsg::LayoutReset => Edit::Reset,
            other => return Err(other),
        })
    }
}
