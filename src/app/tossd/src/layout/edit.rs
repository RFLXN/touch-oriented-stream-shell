//! 배치 편집 연산. UI 편집 모드에서 호출한다(`ui_proto.rs`의 `layout_*` 메시지).
//! 편집 결과는 `Layout::check`를 통과해야만 반영된다 — 이 모듈에서는 요청된 변경을 적용하고 최종 정합성 검증은 `check`에 맡긴다.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::catalog::{kind_info, KindInfo};
use super::{default_layout, metric_ids, Kind, Layout, Page, Rack, Widget, GRID_COLS, GRID_ROWS};

/// 라벨 최대 길이(문자 수)
pub const LABEL_MAX: usize = 24;
/// 버튼 색상 이름 최대 길이(문자 수) 및 기타 설정 값(호스트 ID, 명령 ID) 최대 길이(바이트)
const COLOR_MAX: usize = 16;
const SETTING_MAX: usize = 128;

/// 편집 대상: 페이지에 직접 배치된 복합 위젯 또는 랙
#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Widget,
    Rack,
    /// 랙 내부 버튼 위젯(삭제 시에만 사용)
    Button,
}

/// 배치 편집 요청
#[derive(Clone, PartialEq, Debug)]
pub enum Edit {
    /// 위치나 페이지 이동. `page`가 현재 페이지 수와 같으면 새 페이지를 생성해 배치한다.
    Move { target: Target, id: u32, page: usize, x: u32, y: u32 },
    /// 랙 크기 변경
    Resize { id: u32, w: u32, h: u32 },
    /// 랙 라벨 변경
    Rename { id: u32, label: String },
    /// 새 요소 추가. `what`은 복합 위젯 종류 이름(`"media.player"` 등) 또는 `"rack"`.
    /// 지정한 `page`의 첫 빈자리에 배치하고, 자리가 없으면 이후 페이지나 새 페이지에 배치한다.
    Add { what: String, page: usize, settings: Map<String, Value> },
    /// 랙의 `(x, y)` 칸에 버튼 위젯 추가. `what`은 버튼 위젯 종류 이름(`"command.run"`, `"command.linear"` 등).
    Put { rack: u32, x: u32, y: u32, what: String, settings: Map<String, Value> },
    Remove { target: Target, id: u32 },
    /// 빈 페이지 정리(편집 모드 종료 시). 최소 1페이지는 유지한다.
    Tidy,
    /// 기본 배치로 초기화
    Reset,
}

impl Page {
    /// 페이지에서 `w x h` 크기가 들어갈 첫 빈자리 검색(위에서 아래, 왼쪽에서 오른쪽 순)
    fn free_spot(&self, w: u32, h: u32) -> Option<(u32, u32)> {
        let taken = |x: u32, y: u32| {
            let wide = u64::from;
            let hit = |ix: u32, iy: u32, iw: u32, ih: u32| wide(x) < wide(ix) + wide(iw) && wide(ix) < wide(x) + wide(w) && wide(y) < wide(iy) + wide(ih) && wide(iy) < wide(y) + wide(h);
            self.racks.iter().any(|r| hit(r.x, r.y, r.w, r.h)) || self.widgets.iter().any(|i| hit(i.x, i.y, i.w, i.h))
        };
        if w > GRID_COLS || h > GRID_ROWS {
            return None;
        }
        (0..=GRID_ROWS - h).flat_map(|y| (0..=GRID_COLS - w).map(move |x| (x, y))).find(|&(x, y)| !taken(x, y))
    }
}

fn widgets_of(page: &mut Page) -> &mut Vec<Widget> {
    &mut page.widgets
}

fn racks_of(page: &mut Page) -> &mut Vec<Rack> {
    &mut page.racks
}

/// 페이지에 배치된 항목 중 `found`와 일치하는 요소를 찾아 `page`로 옮기고 `place`로 좌표를 갱신한다.
/// 찾지 못하면 `false`. 같은 페이지 내 이동이면 기존 슬라이스 순서를 유지한다.
fn relocate<T>(pages: &mut [Page], items: fn(&mut Page) -> &mut Vec<T>, found: impl Fn(&T) -> bool, page: usize, place: impl FnOnce(&mut T)) -> bool {
    let Some((from, at)) = pages.iter_mut().enumerate().find_map(|(i, p)| items(p).iter().position(&found).map(|at| (i, at))) else {
        return false;
    };
    let mut item = items(&mut pages[from]).remove(at);
    place(&mut item);
    if from == page {
        items(&mut pages[from]).insert(at, item);
    } else {
        items(&mut pages[page]).push(item);
    }
    true
}

/// 입력된 설정에서 해당 위젯 종류가 정의한 키만 추출하고 정제한다(`catalog`의 `settings` 기준). 알 수 없는 키나 잘못된 값은 버린다.
///   metrics      메트릭 ID 목록(`metric_ids`: 빈 값, 길이 초과, 중복 제외 후 최대 개수까지)
///   text, color  앞뒤 공백 제거 후 길이 초과 또는 제어 문자가 있으면 빈 문자열로 처리
///   그 외        문자열 그대로 유지하되 길이 초과 또는 제어 문자가 있으면 버림
/// 필수 항목이 빠졌거나 비어 있으면 오류를 반환한다.
fn pick_settings(info: &KindInfo, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let clean = |text: &str| !text.chars().any(char::is_control);
    let mut picked = Map::new();
    for s in &info.settings {
        let Some(value) = given.get(s.key) else { continue };
        let value = match s.kind {
            "metrics" => Some(Value::Array(metric_ids(value, s.max as usize).into_iter().map(Value::String).collect())),
            "text" | "color" => {
                let max = if s.kind == "text" { LABEL_MAX } else { COLOR_MAX };
                let text = value.as_str().map(str::trim).filter(|t| t.chars().count() <= max && clean(t)).unwrap_or_default();
                Some(Value::String(text.to_string()))
            }
            _ => value.as_str().filter(|t| t.len() <= SETTING_MAX && clean(t)).map(|t| Value::String(t.to_string())),
        };
        if let Some(value) = value {
            picked.insert(s.key.to_string(), value);
        }
    }
    let empty = |key: &str| match picked.get(key) {
        Some(Value::String(v)) => v.is_empty(),
        Some(Value::Array(v)) => v.is_empty(),
        _ => true,
    };
    if info.settings.iter().any(|s| s.required && empty(s.key)) {
        return Err("필수 설정 값이 비어 있습니다".into());
    }
    Ok(picked)
}

impl Layout {
    /// 배치를 수정한다. 수정 결과가 규칙을 만족할 때만 적용하며, 실패하면 기존 배치를 유지하고 사유를 반환한다.
    pub fn edit(&mut self, edit: &Edit) -> Result<(), String> {
        let mut next = self.clone();
        next.apply(edit)?;
        next.check()?;
        *self = next;
        Ok(())
    }

    fn next_widget_id(&self) -> Result<u32, String> {
        let last = self.pages.iter().flat_map(|p| p.widgets.iter().chain(p.racks.iter().flat_map(|r| &r.widgets))).map(|w| w.id).max().unwrap_or(0);
        last.checked_add(1).ok_or_else(|| "위젯 ID를 더 이상 할당할 수 없습니다".to_string())
    }

    fn next_rack_id(&self) -> Result<u32, String> {
        let last = self.pages.iter().flat_map(|p| &p.racks).map(|r| r.id).max().unwrap_or(0);
        last.checked_add(1).ok_or_else(|| "랙 ID를 더 이상 할당할 수 없습니다".to_string())
    }

    /// 마지막에 새 빈 페이지를 추가하고 해당 인덱스를 반환한다.
    fn push_page(&mut self) -> usize {
        self.pages.push(Page { name: format!("Page {}", self.pages.len() + 1), ..Page::default() });
        self.pages.len() - 1
    }

    /// `w x h` 영역을 배치할 위치를 찾는다: `page`부터 순차 탐색해 첫 빈자리를 쓰고, 없으면 새 페이지를 추가한다.
    fn place(&mut self, page: usize, w: u32, h: u32) -> Result<(usize, u32, u32), String> {
        if w < 1 || h < 1 || w > self.cols || h > self.rows {
            return Err("크기가 그리드 범위를 벗어납니다".into());
        }
        let start = page.min(self.pages.len());
        let spot = self.pages.iter().enumerate().skip(start).find_map(|(i, p)| p.free_spot(w, h).map(|(x, y)| (i, x, y)));
        Ok(spot.unwrap_or_else(|| (self.push_page(), 0, 0)))
    }

    /// 지정한 위젯 종류(`what`)로 새 위젯을 생성한다(초기 좌표는 `(0, 0)`). `composite`가 true면 페이지, false면 랙 배치용이다.
    /// 해당 컨테이너에 놓을 수 없는 종류면 오류를 반환한다. 설정 중 `label`과 `color`는 위젯 필드로 분리되고 나머지가 `settings`에 담긴다.
    fn new_widget(&self, what: &str, composite: bool, given: &Map<String, Value>) -> Result<Widget, String> {
        let kind = Kind::from_name(what).ok_or_else(|| format!("알 수 없는 위젯 종류: {what}"))?;
        let info = kind_info(kind).filter(|_| kind.is_composite() == composite).ok_or_else(|| format!("{}에 배치할 수 없는 위젯 종류: {what}", if composite { "페이지" } else { "랙" }))?;
        let mut settings = pick_settings(&info, given)?;
        let mut text = |key: &str| match settings.remove(key) {
            Some(Value::String(text)) => Some(text),
            _ => None,
        };
        let label = text("label").unwrap_or_else(|| info.label.to_string());
        let color = text("color").unwrap_or_default();
        Ok(Widget { id: self.next_widget_id()?, kind, x: 0, y: 0, w: info.w, h: info.h, label, color, settings })
    }

    fn rack_mut(&mut self, id: u32) -> Result<&mut Rack, String> {
        self.pages.iter_mut().flat_map(|p| &mut p.racks).find(|r| r.id == id).ok_or_else(|| format!("랙 {id}를 찾을 수 없습니다"))
    }

    fn apply(&mut self, edit: &Edit) -> Result<(), String> {
        match edit {
            Edit::Move { target, id, page, x, y } => {
                if *page > self.pages.len() {
                    return Err(format!("페이지 {page}가 존재하지 않습니다"));
                }
                if *page == self.pages.len() {
                    self.push_page();
                }
                let moved = match target {
                    Target::Widget => relocate(&mut self.pages, widgets_of, |w| w.id == *id, *page, |w| (w.x, w.y) = (*x, *y)),
                    Target::Rack => relocate(&mut self.pages, racks_of, |r| r.id == *id, *page, |r| (r.x, r.y) = (*x, *y)),
                    Target::Button => return Err("랙 내부 버튼은 직접 이동할 수 없습니다(삭제 후 다시 추가)".into()),
                };
                if !moved {
                    return Err(format!("ID {id}를 찾을 수 없습니다"));
                }
            }
            Edit::Resize { id, w, h } => {
                let rack = self.rack_mut(*id)?;
                (rack.w, rack.h) = (*w, *h);
            }
            Edit::Rename { id, label } => {
                let label = label.trim();
                if label.chars().count() > LABEL_MAX || label.chars().any(char::is_control) {
                    return Err("라벨이 너무 길거나 허용되지 않는 문자가 포함되어 있습니다".into());
                }
                self.rack_mut(*id)?.label = label.to_string();
            }
            Edit::Add { what, page, settings } => {
                if what == "rack" {
                    let id = self.next_rack_id()?;
                    let (page, x, y) = self.place(*page, 2, 1)?;
                    self.pages[page].racks.push(Rack { id, label: "RACK".into(), x, y, w: 2, h: 1, widgets: vec![] });
                } else {
                    let mut widget = self.new_widget(what, true, settings)?;
                    let (page, x, y) = self.place(*page, widget.w, widget.h)?;
                    (widget.x, widget.y) = (x, y);
                    self.pages[page].widgets.push(widget);
                }
            }
            Edit::Put { rack, x, y, what, settings } => {
                let mut widget = self.new_widget(what, false, settings)?;
                (widget.x, widget.y) = (*x, *y);
                self.rack_mut(*rack)?.widgets.push(widget);
            }
            Edit::Remove { target, id } => {
                let count = |l: &Layout| l.pages.iter().map(|p| p.widgets.len() + p.racks.len() + p.racks.iter().map(|r| r.widgets.len()).sum::<usize>()).sum::<usize>();
                let before = count(self);
                for p in &mut self.pages {
                    match target {
                        Target::Widget => p.widgets.retain(|w| w.id != *id),
                        Target::Rack => p.racks.retain(|r| r.id != *id),
                        Target::Button => p.racks.iter_mut().for_each(|r| r.widgets.retain(|w| w.id != *id)),
                    }
                }
                if before == count(self) {
                    return Err(format!("ID {id}를 찾을 수 없습니다"));
                }
            }
            Edit::Tidy => {
                self.pages.retain(|p| !p.is_empty());
                if self.pages.is_empty() {
                    self.pages.push(Page { name: "Main".into(), ..Page::default() });
                }
            }
            Edit::Reset => *self = default_layout(),
        }
        Ok(())
    }
}
