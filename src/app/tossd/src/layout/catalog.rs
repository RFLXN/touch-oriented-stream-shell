//! 추가 가능한 위젯 카탈로그: 앱 목록, 앱별 위젯 종류, 위젯 추가 시 필요한 설정 스키마.
//! UI가 이를 받아(`catalog` 메시지) 추가 메뉴를 구성하고, 배치 편집(`edit.rs`)에서는 이 정의를 기준으로 입력 설정을 필터링한다.

use serde::Serialize;

use super::Kind;

/// 앱 정보와 해당 앱이 제공하는 위젯 종류 목록
#[derive(Serialize, Clone)]
pub struct AppInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub widgets: Vec<KindInfo>,
}

#[derive(Serialize, Clone)]
pub struct KindInfo {
    pub kind: Kind,
    pub title: &'static str,
    pub about: &'static str,
    /// 배치 형태: `"widget"`(페이지에 직접 배치되는 복합 위젯) 또는 `"button"`(랙 내부 칸에 놓이는 버튼 위젯, `Kind::is_composite`에서 파생)
    pub form: &'static str,
    /// 점유 타일 수(고정 크기. 크기가 다르면 별도 위젯 종류로 분리한다). 버튼 위젯은 랙 내 1칸이다.
    pub w: u32,
    pub h: u32,
    /// 새로 생성할 때 적용되는 기본 라벨
    pub label: &'static str,
    /// 위젯 추가 시 입력받는 설정 목록
    pub settings: Vec<SettingInfo>,
}

impl KindInfo {
    fn new(kind: Kind, title: &'static str, about: &'static str, (w, h): (u32, u32), label: &'static str, settings: Vec<SettingInfo>) -> Self {
        Self { kind, title, about, form: if kind.is_composite() { "widget" } else { "button" }, w, h, label, settings }
    }
}

#[derive(Serialize, Clone)]
pub struct SettingInfo {
    /// `Widget.settings`의 키 이름(버튼 위젯의 `label`과 `color`는 `Widget` 최상위 필드로 저장)
    pub key: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    /// 설정 값 타입:
    ///   host     페어링된 호스트(PC) ID. `required`가 false면 빈 문자열("연결된 첫 번째 호스트") 허용
    ///   command  선택한 호스트에 등록된 명령 ID(해당 호스트가 보고한 목록에서 선택. `"host"` 설정이 선행되어야 함).
    ///            `filter`가 지정되면 해당 종류의 명령만 선택 가능(`toss-proto`의 `KIND_*`)
    ///   metrics  선택한 호스트에서 측정 가능한 리소스 ID 배열(해당 호스트 목록에서 최대 `max`개까지 선택)
    ///   text     문자열(라벨)
    ///   url      웹페이지 주소(`http://` 또는 `https://`. `layout`의 `web_url`)
    ///   color    버튼 색상 이름(UI `Theme.buttonColors`)
    ///   light    인디케이터 색상 이름(UI `Theme.indicatorColors`). 비우면 버튼 색상에 맞춰 정한다(`Theme.indicatorFor`)
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// 필수 입력 여부
    pub required: bool,
    /// `type`이 `command`일 때 허용하는 명령 종류(`run` / `switch` / `stepped` / `linear`). 그 외에는 빈 문자열
    pub filter: &'static str,
    /// `type`이 `host`일 때 덱 자신(`SELF_HOST`)도 선택지에 포함할지 여부
    pub own: bool,
    /// `type`이 `host`일 때 덱이 직접 조회하는 Spotify 계정(`spotify::SOURCE`, `"@spotify"`)도 선택지에 포함할지 여부
    pub spotify: bool,
    /// `type`이 `metrics`일 때 최대 선택 가능 개수(1부터 `max`개)
    pub max: u32,
}

impl SettingInfo {
    fn new(key: &'static str, title: &'static str, about: &'static str, kind: &'static str, required: bool) -> Self {
        Self { key, title, about, kind, required, filter: "", own: false, spotify: false, max: 0 }
    }
}

/// 자원 미터 위젯 종류 생성 헬퍼(`monitor` 앱). 4가지 종류가 크기와 최대 미터 수만 다르다.
fn monitor_kind(kind: Kind, title: &'static str, about: &'static str, size: (u32, u32)) -> KindInfo {
    let settings = vec![
        SettingInfo { own: true, ..SettingInfo::new("host", "Source", "This deck or a paired PC", "host", true) },
        SettingInfo { max: kind.meters() as u32, ..SettingInfo::new("metrics", "Meters", "Shown top to bottom in the order picked", "metrics", true) },
    ];
    KindInfo::new(kind, title, about, size, "MONITOR", settings)
}

/// 명령 위젯 종류 생성 헬퍼(`command` 앱). 4가지 모두 랙 1칸 크기이며 설정 구조는 같고(`host`, `command`, `label`, `color`)
/// 필터링할 명령 종류(`filter`)만 다르다. 인디케이터가 있는 스위치만 인디케이터 색상(`light`)을 더 고른다.
fn command_kind(kind: Kind, title: &'static str, about: &'static str, filter: &'static str, pick: &'static str) -> KindInfo {
    let mut settings = vec![
        SettingInfo::new("host", "PC", "The PC that runs the command", "host", true),
        SettingInfo { filter, ..SettingInfo::new("command", "Command", pick, "command", true) },
        SettingInfo::new("label", "Label", "Printed under it", "text", false),
        SettingInfo::new("color", "Color", "", "color", false),
    ];
    if kind == Kind::CommandSwitch {
        settings.push(SettingInfo::new("light", "Light", "Indicator color when on. Auto follows the button color", "light", false));
    }
    KindInfo::new(kind, title, about, (1, 1), "", settings)
}

/// 웹페이지 버튼 종류 생성 헬퍼(`web` 앱). 두 종류 모두 랙 1칸이고 설정 구조가 같다(`host`, `url`, `label`, `color`).
fn web_kind(kind: Kind, title: &'static str, about: &'static str) -> KindInfo {
    let settings = vec![
        SettingInfo::new("host", "PC", "The PC whose browser opens the page", "host", true),
        SettingInfo::new("url", "Address", "http:// or https://", "url", true),
        SettingInfo::new("label", "Label", "Printed under it", "text", false),
        SettingInfo::new("color", "Color", "", "color", false),
    ];
    KindInfo::new(kind, title, about, (1, 1), "", settings)
}

pub fn catalog() -> Vec<AppInfo> {
    vec![
        AppInfo {
            id: "media",
            title: "Media",
            about: "What is playing on a PC or a Spotify account",
            widgets: vec![KindInfo::new(
                Kind::MediaPlayer,
                "Player",
                "Cover, track and playback keys",
                (4, 2),
                "MEDIA",
                vec![SettingInfo { spotify: true, ..SettingInfo::new("host", "Source", "A paired PC, or the Spotify account linked to this deck", "host", false) }],
            )],
        },
        AppInfo {
            id: "monitor",
            title: "Resource monitor",
            about: "CPU, memory, GPU, disk, network and temperatures of this deck or a PC",
            widgets: vec![
                monitor_kind(Kind::Monitor2x1, "Meters 2×1", "Up to 3 meters", (2, 1)),
                monitor_kind(Kind::Monitor2x2, "Meters 2×2", "Up to 6 meters", (2, 2)),
                monitor_kind(Kind::Monitor4x1, "Meters 4×1", "Up to 6 meters in two columns", (4, 1)),
                monitor_kind(Kind::Monitor4x2, "Meters 4×2", "Up to 12 meters in two columns", (4, 2)),
            ],
        },
        AppInfo {
            id: "command",
            title: "Command",
            about: "Commands, switches and knobs registered on a PC",
            widgets: vec![
                command_kind(Kind::CommandRun, "Run", "A button that runs one command", "run", "toss-connector command add"),
                command_kind(Kind::CommandSwitch, "Switch", "An on/off button that shows the PC's state", "switch", "toss-connector command add-switch"),
                command_kind(Kind::CommandStepped, "Stepped knob", "A knob that selects one of several states", "stepped", "toss-connector command add-stepped"),
                command_kind(Kind::CommandLinear, "Linear knob", "A knob that sets a value in a range", "linear", "toss-connector command add-linear"),
            ],
        },
        AppInfo {
            id: "web",
            title: "Web page",
            about: "Open a web page in a PC's browser",
            widgets: vec![
                web_kind(Kind::WebButton, "Simple", "A button that opens the page"),
                web_kind(Kind::WebIcon, "Icon", "A button that shows the site's icon"),
            ],
        },
    ]
}

/// 위젯 종류에 해당하는 카탈로그 메타데이터 조회
pub(super) fn kind_info(kind: Kind) -> Option<KindInfo> {
    catalog().into_iter().flat_map(|a| a.widgets).find(|k| k.kind == kind)
}
