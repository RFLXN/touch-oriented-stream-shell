//! 위젯 배치 관리: 페이지, 랙, 위젯 정의 및 유효성 검증.
//!
//! 앱과 위젯 종류
//!   앱         기능 묶음(예: Media). 위젯을 추가할 때 사용자가 먼저 앱을 선택한다.
//!   위젯 종류  앱이 제공하는 위젯 타입(`Kind`, 예: `media.player`). 점유 크기와 설정 스키마를 가진다.
//!              한 앱이 여러 위젯 종류를 제공할 수 있다(크기나 레이아웃이 다른 변형).
//!   위젯       위젯 종류의 인스턴스. 개별 설정(`settings`, 예: 대상 PC)을 가진다.
//!
//! 모듈 구성
//!   mod.rs      배치 모델(`Kind`, `Widget`, `Rack`, `Page`, `Layout`), 유효성 검증(`check`), 기본 배치
//!   catalog.rs  추가 가능한 앱과 위젯 종류 목록(`catalog`). UI가 이를 받아 "추가" 메뉴를 그린다.
//!   edit.rs     배치 편집 연산(`Edit`). UI 편집 모드에서 요청한다.
//!   store.rs    배치 파일 영구 저장소(`Store`)
//!
//! 새 위젯 종류를 추가할 때는 `Kind`에 배리언트를 넣고(복합 위젯이면 `is_composite`에도 반영), `catalog.rs`의 `catalog()`에 등록한다
//! (UI 측은 `app/ui/shell/WidgetSlot.qml`의 `sources`와 `app/ui/widgets/` 아래 QML 컴포넌트 추가).
//!
//! 배치는 사용자가 기기 UI 편집 모드에서 직접 수정한다. 수정할 때마다 `Store`가 파일에 저장하며 재부팅 시 이를 불러온다.
//! 저장된 파일이 없으면 `default_layout()`으로 시작한다.

mod catalog;
mod edit;
mod store;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use catalog::{catalog, AppInfo};
pub use edit::{Edit, Target};
pub use store::Store;

/// 페이지당 타일 그리드 크기. 2026-11-16에 4x6으로 확정(800x1280 화면 기준 타일 1칸이 약 178px = 2.1cm).
/// UI 측은 `layout` 메시지의 `cols`, `rows`를 그대로 따른다.
pub const GRID_COLS: u32 = 4;
pub const GRID_ROWS: u32 = 6;
/// 최대 페이지 수
pub const PAGES_MAX: usize = 12;
/// `settings.host`에서 "덱 자신"을 가리키는 특수 값(페어링된 PC는 해당 호스트 ID)
pub const SELF_HOST: &str = "@deck";
/// 메트릭 ID 최대 길이
const METRIC_ID_MAX: usize = 64;

/// 위젯 종류. 이름은 `"<앱>.<종류>"` 형식이다.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// 미디어 플레이어(앨범 아트, 곡 정보, 재생 제어 키 7개)
    #[serde(rename = "media.player")]
    MediaPlayer,
    /// 자원 미터: 선택한 호스트(페어링된 PC 또는 덱 자신)의 시스템 자원을 표시한다. 4가지 크기가 있으며 최대 미터 수가 다르다
    /// (2x1은 3개, 2x2와 4x1은 6개, 4x2는 12개).
    #[serde(rename = "monitor.2x1")]
    Monitor2x1,
    #[serde(rename = "monitor.2x2")]
    Monitor2x2,
    #[serde(rename = "monitor.4x1")]
    Monitor4x1,
    #[serde(rename = "monitor.4x2")]
    Monitor4x2,
    /// 명령 실행 버튼: 누르면 호스트에 등록된 명령 1건을 실행한다(랙 내부 배치).
    #[serde(rename = "command.run")]
    CommandRun,
    /// 명령 스위치: 호스트에 등록된 스위치를 켜거나 끈다. 현재 ON/OFF 상태는 호스트가 보고한다(랙 내부 배치).
    #[serde(rename = "command.switch")]
    CommandSwitch,
    /// 단계 노브: 호스트에 등록된 단계 중 하나를 선택한다(랙 내부 배치).
    #[serde(rename = "command.stepped")]
    CommandStepped,
    /// 연속 노브: 호스트에 등록된 수치 범위 내 값을 조절한다(랙 내부 배치).
    #[serde(rename = "command.linear")]
    CommandLinear,
}

impl Kind {
    /// 이름(`"media.player"`)으로 `Kind`를 찾는다. 알 수 없는 이름이면 `None`.
    pub fn from_name(name: &str) -> Option<Self> {
        serde_json::from_value(Value::String(name.to_string())).ok()
    }

    /// 복합 위젯 여부(true면 랙과 동등하게 페이지에 직접 배치, false면 랙 내부 칸에만 배치 가능한 버튼 위젯).
    pub fn is_composite(self) -> bool {
        matches!(self, Kind::MediaPlayer) || self.meters() > 0
    }

    /// 자원 미터 위젯이면 최대 미터 수를, 아니면 0을 반환한다.
    pub fn meters(self) -> usize {
        match self {
            Kind::Monitor2x1 => 3,
            Kind::Monitor2x2 | Kind::Monitor4x1 => 6,
            Kind::Monitor4x2 => 12,
            _ => 0,
        }
    }
}

/// 위젯: 위젯 종류를 타일에 올려놓은 것
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Widget {
    pub id: u32,
    pub kind: Kind,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub label: String,
    #[serde(default)]
    pub color: String,
    /// 위젯 개별 설정. 허용 키는 위젯 종류에 따라 결정된다(`catalog`의 `settings`).
    #[serde(default)]
    pub settings: Map<String, Value>,
}

/// 랙: 버튼 위젯을 담는 컨테이너. 내부 위젯 좌표는 랙 좌상단을 `(0, 0)`으로 계산한다.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Rack {
    pub id: u32,
    pub label: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Page {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub racks: Vec<Rack>,
    /// 복합 위젯(페이지에 직접 배치)
    #[serde(default)]
    pub widgets: Vec<Widget>,
}

impl Page {
    fn is_empty(&self) -> bool {
        self.racks.is_empty() && self.widgets.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Layout {
    pub cols: u32,
    pub rows: u32,
    pub pages: Vec<Page>,
}

impl Default for Layout {
    fn default() -> Self {
        default_layout()
    }
}

impl Layout {
    /// 자원 미터 위젯들이 모니터링 중인 항목: `호스트(SELF_HOST 또는 PC ID) -> 메트릭 ID 목록(중복 제거)`.
    /// 덱 자체 항목은 데몬이 측정하고, PC 항목은 해당 호스트에 `Watch`로 전달해 보고 있지 않은 항목은 측정하지 않도록 한다.
    pub fn watched(&self) -> BTreeMap<String, Vec<String>> {
        let mut out = BTreeMap::<String, Vec<String>>::new();
        for w in self.pages.iter().flat_map(|p| &p.widgets).filter(|w| w.kind.meters() > 0) {
            let host = w.settings.get("host").and_then(Value::as_str).filter(|h| !h.is_empty()).unwrap_or(SELF_HOST);
            let list = out.entry(host.to_string()).or_default();
            for id in w.settings.get("metrics").map(|m| metric_ids(m, w.kind.meters())).unwrap_or_default() {
                if !list.contains(&id) {
                    list.push(id);
                }
            }
        }
        out
    }

    /// 배치가 모든 규칙을 만족하는지 검증한다. 위반 사항이 있으면 사유를 반환한다.
    ///   - 랙과 복합 위젯은 페이지 그리드 범위 내에 있으며 서로 겹치지 않는다.
    ///   - 페이지에 직접 배치된 위젯은 복합 위젯이어야 한다.
    ///   - 랙 내부 위젯은 복합 위젯이 아니어야 하며, 랙 크기 안에 있고 서로 겹치지 않는다.
    ///   - 위젯 ID와 랙 ID는 전체 배치에서 각각 유일해야 한다.
    pub fn check(&self) -> Result<(), String> {
        /// 영역이 판(`cols` x `rows`) 안에 있는지 확인하고 점유 타일을 기록한다.
        /// 정수 오버플로를 피하기 위해 좌표와 크기를 더하지 않고 뺄셈으로 경계를 비교한다.
        fn claim(used: &mut HashSet<(u32, u32)>, (x, y, w, h): (u32, u32, u32, u32), (cols, rows): (u32, u32), what: &str) -> Result<(), String> {
            if w < 1 || h < 1 || x >= cols || w > cols - x || y >= rows || h > rows - y {
                return Err(format!("{what}: 그리드 범위 초과"));
            }
            for cx in x..x + w {
                for cy in y..y + h {
                    if !used.insert((cx, cy)) {
                        return Err(format!("{what}: 타일 ({cx}, {cy}) 중복 점유"));
                    }
                }
            }
            Ok(())
        }
        let (mut widget_ids, mut rack_ids) = (HashSet::new(), HashSet::new());
        if (self.cols, self.rows) != (GRID_COLS, GRID_ROWS) {
            return Err(format!("그리드 크기 불일치: {}x{} ({GRID_COLS}x{GRID_ROWS}이어야 함)", self.cols, self.rows));
        }
        if self.pages.is_empty() || self.pages.len() > PAGES_MAX {
            return Err(format!("페이지 수 범위 초과: {}개(허용: 1~{PAGES_MAX})", self.pages.len()));
        }
        let grid = (self.cols, self.rows);
        for page in &self.pages {
            let mut used = HashSet::new();
            for w in &page.widgets {
                if !w.kind.is_composite() {
                    return Err(format!("위젯 {}: 복합 위젯만 페이지에 직접 배치할 수 있음", w.id));
                }
                if !widget_ids.insert(w.id) {
                    return Err(format!("위젯 ID {} 중복", w.id));
                }
                claim(&mut used, (w.x, w.y, w.w, w.h), grid, &format!("위젯 {}", w.id))?;
            }
            for r in &page.racks {
                if !rack_ids.insert(r.id) {
                    return Err(format!("랙 ID {} 중복", r.id));
                }
                claim(&mut used, (r.x, r.y, r.w, r.h), grid, &format!("랙 {}", r.id))?;
                let mut inside = HashSet::new();
                for w in &r.widgets {
                    if w.kind.is_composite() {
                        return Err(format!("위젯 {}: 복합 위젯은 랙 {} 내부에 넣을 수 없음", w.id, r.id));
                    }
                    if !widget_ids.insert(w.id) {
                        return Err(format!("위젯 ID {} 중복", w.id));
                    }
                    claim(&mut inside, (w.x, w.y, w.w, w.h), (r.w, r.h), &format!("랙 {}의 위젯 {}", r.id, w.id))?;
                }
            }
        }
        Ok(())
    }
}

/// 설정 JSON 값에서 메트릭 ID 배열을 추출한다(빈 값, 길이 초과, 제어 문자, 중복을 제외하고 최대 `max`개).
fn metric_ids(value: &Value, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in value.as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !id.is_empty() && id.len() <= METRIC_ID_MAX && !id.chars().any(char::is_control) && !out.iter().any(|o| o == id) && out.len() < max {
            out.push(id.to_string());
        }
    }
    out
}

/// 기본 자원 미터가 표시할 덱 자체 메트릭(온도, 메모리, 부하)
fn default_metrics() -> Value {
    serde_json::json!(["cpu.temp", "mem", "load"])
}

/// 기본 배치(저장된 배치가 없거나 "기본 배치로 초기화" 시 사용): 한 페이지에 페어링된 PC 없이도 바로 동작하는 위젯만 둔다.
///   미디어 플레이어   상단 전체 가로(4x2)
///   자원 미터         그 아래 왼쪽(2x1): 덱 자체 온도, 메모리, 부하
/// 랙과 명령 위젯은 사용자가 대상 PC와 명령을 직접 선택해야 의미가 있으므로 편집 모드에서 추가한다.
pub fn default_layout() -> Layout {
    let widget = |id: u32, kind: Kind, (x, y, w, h): (u32, u32, u32, u32), label: &str| Widget { id, kind, x, y, w, h, label: label.into(), color: String::new(), settings: Map::new() };
    let mut monitor = widget(2, Kind::Monitor2x1, (0, 2, 2, 1), "MONITOR");
    monitor.settings.insert("host".into(), Value::String(SELF_HOST.into()));
    monitor.settings.insert("metrics".into(), default_metrics());
    let main = Page {
        name: "Main".into(),
        widgets: vec![widget(1, Kind::MediaPlayer, (0, 0, 4, 2), "MEDIA"), monitor],
        racks: vec![],
    };
    Layout { cols: GRID_COLS, rows: GRID_ROWS, pages: vec![main] }
}
