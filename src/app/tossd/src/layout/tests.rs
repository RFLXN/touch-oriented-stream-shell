//! 배치 검증, 편집, 카탈로그, 저장소 단위 테스트.

use std::fs;

use serde_json::{Map, Value};

use super::edit::LABEL_MAX;
use super::*;

/// 테스트용 샘플 배치(2페이지). 복합 위젯 2개와 다양한 크기의 랙에 명령 버튼/스위치를 배치한 구성(실제 기기 초기 배치는 아님).
fn sample_layout() -> Layout {
    fn widget(id: u32, kind: Kind, (x, y, w, h): (u32, u32, u32, u32), label: &str, color: &str) -> Widget {
        Widget { id, kind, x, y, w, h, label: label.into(), color: color.into(), settings: Map::new() }
    }
    fn button(id: u32, x: u32, y: u32, label: &str, color: &str) -> Widget {
        widget(id, Kind::CommandRun, (x, y, 1, 1), label, color)
    }
    fn switch(id: u32, x: u32, y: u32, label: &str, color: &str) -> Widget {
        widget(id, Kind::CommandSwitch, (x, y, 1, 1), label, color)
    }
    fn rack(id: u32, label: &str, (x, y, w, h): (u32, u32, u32, u32), widgets: Vec<Widget>) -> Rack {
        Rack { id, label: label.into(), x, y, w, h, widgets }
    }

    let main = Page {
        name: "Main".into(),
        widgets: vec![widget(1, Kind::MediaPlayer, (0, 0, 2, 2), "MEDIA", ""), widget(2, Kind::Monitor2x1, (2, 0, 2, 1), "MONITOR", "")],
        racks: vec![
            rack(1, "AUDIO", (2, 1, 2, 1), vec![switch(3, 0, 0, "MUTE", "graphite"), switch(4, 1, 0, "MIC", "graphite")]),
            rack(
                2,
                "STREAM",
                (0, 2, 4, 2),
                vec![
                    switch(5, 0, 0, "녹화", "red"),
                    switch(6, 1, 0, "配信", "orange"),
                    button(7, 2, 0, "SCENE 1", "bone"),
                    button(8, 3, 0, "SCENE 2", "bone"),
                    button(9, 0, 1, "화면 전환", "bone"),
                    button(10, 1, 1, "スクショ", "bone"),
                    switch(11, 2, 1, "静音", "bone"),
                    button(12, 3, 1, "CLIP", "yellow"),
                ],
            ),
            rack(3, "DESKTOP", (0, 4, 4, 1), vec![button(13, 0, 0, "PREV", "bone"), button(14, 1, 0, "NEXT", "bone"), button(15, 2, 0, "LOCK", "blue"), button(16, 3, 0, "SLEEP", "graphite")]),
            rack(4, "APPS", (0, 5, 2, 1), vec![button(17, 0, 0, "TERM", "green"), button(18, 1, 0, "WEB", "bone")]),
        ],
    };
    let second = Page {
        name: "Function".into(),
        widgets: vec![],
        racks: vec![rack(5, "FUNCTION", (0, 0, 4, 2), (0..8u32).map(|i| button(101 + i, i % 4, i / 4, &format!("F{}", 13 + i), if i % 4 == 3 { "graphite" } else { "bone" })).collect())],
    };

    Layout { cols: GRID_COLS, rows: GRID_ROWS, pages: vec![main, second] }
}

fn widget_of(layout: &Layout, id: u32) -> Option<&Widget> {
    layout.pages.iter().flat_map(|p| p.widgets.iter().chain(p.racks.iter().flat_map(|r| &r.widgets))).find(|w| w.id == id)
}

/// `(page, kind_tag, id, (x, y, w, h))`
type Spot<'a> = (usize, &'a str, u32, (u32, u32, u32, u32));

fn spots(l: &Layout) -> Vec<Spot<'_>> {
    let mut out = Vec::new();
    for (i, p) in l.pages.iter().enumerate() {
        out.extend(p.widgets.iter().map(|w| (i, "widget", w.id, (w.x, w.y, w.w, w.h))));
        out.extend(p.racks.iter().map(|r| (i, "rack", r.id, (r.x, r.y, r.w, r.h))));
    }
    out
}

#[test]
fn the_layouts_follow_the_rules() {
    for layout in [default_layout(), sample_layout()] {
        assert_eq!((layout.cols, layout.rows), (4, 6));
        layout.check().unwrap();
    }
}

#[test]
fn the_device_starts_with_one_page_of_widgets_that_need_no_setup() {
    let layout = default_layout();
    assert_eq!(layout.pages.len(), 1);
    assert!(layout.pages[0].racks.is_empty());
    let media = widget_of(&layout, 1).unwrap();
    assert!(media.kind == Kind::MediaPlayer && (media.x, media.y, media.w, media.h) == (0, 0, 4, 2));
    assert!(layout.pages[0].widgets.iter().all(|w| w.kind.is_composite()));
    assert_eq!(layout.pages[0].widgets.len(), 2);
}

/// 위젯 종류 이름은 정의된 값만 허용한다. 초기 테스트용 종류(`button`, `switch`)는 제거되었으므로 포함된 저장 파일은 백업 처리된다.
#[test]
fn kinds_are_known_by_name() {
    assert_eq!(Kind::from_name("media.player"), Some(Kind::MediaPlayer));
    assert_eq!(Kind::from_name("command.linear"), Some(Kind::CommandLinear));
    for gone in ["button", "switch", "", "Media.Player"] {
        assert_eq!(Kind::from_name(gone), None, "{gone}");
    }
}

/// 규칙 위반 배치는 모두 거절된다.
#[test]
fn broken_layouts_are_rejected() {
    fn widget(id: u32, kind: Kind, x: u32, y: u32, w: u32, h: u32) -> Widget {
        Widget { id, kind, x, y, w, h, label: String::new(), color: String::new(), settings: Map::new() }
    }
    fn layout(racks: Vec<Rack>, widgets: Vec<Widget>) -> Layout {
        Layout { cols: GRID_COLS, rows: GRID_ROWS, pages: vec![Page { name: "p".into(), racks, widgets }] }
    }
    fn rack(id: u32, (x, y, w, h): (u32, u32, u32, u32), widgets: Vec<Widget>) -> Rack {
        Rack { id, label: String::new(), x, y, w, h, widgets }
    }
    let fails = |l: Layout, why: &str| assert!(l.check().is_err(), "거절되어야 함: {why}");

    layout(vec![rack(1, (0, 0, 2, 1), vec![widget(1, Kind::CommandRun, 0, 0, 1, 1)])], vec![widget(2, Kind::MediaPlayer, 2, 0, 2, 2)]).check().unwrap();
    fails(layout(vec![], vec![widget(1, Kind::CommandRun, 0, 0, 1, 1)]), "버튼 위젯이 랙 밖에 있음");
    fails(layout(vec![rack(1, (0, 0, 2, 2), vec![widget(1, Kind::Monitor2x1, 0, 0, 2, 1)])], vec![]), "복합 위젯이 랙 내부에 있음");
    fails(layout(vec![rack(1, (0, 0, 2, 1), vec![])], vec![widget(1, Kind::MediaPlayer, 1, 0, 2, 2)]), "랙과 복합 위젯이 겹침");
    fails(layout(vec![rack(1, (3, 0, 2, 1), vec![])], vec![]), "랙이 페이지 그리드를 벗어남");
    fails(layout(vec![rack(1, (0, 0, 2, 1), vec![widget(1, Kind::CommandRun, 2, 0, 1, 1)])], vec![]), "위젯이 랙 크기를 벗어남");
    fails(layout(vec![rack(1, (0, 0, 2, 1), vec![widget(1, Kind::CommandRun, 0, 0, 1, 1), widget(2, Kind::CommandSwitch, 0, 0, 1, 1)])], vec![]), "랙 내부 위젯끼리 겹침");
    fails(layout(vec![rack(1, (0, 0, 2, 1), vec![widget(1, Kind::CommandRun, 0, 0, 1, 1), widget(1, Kind::CommandRun, 1, 0, 1, 1)])], vec![]), "위젯 ID 중복");
}

#[test]
fn moving_goes_only_where_there_is_room() {
    let mut l = default_layout();
    l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 0, x: 2, y: 2 }).unwrap();
    assert_eq!(widget_of(&l, 2).map(|w| (w.x, w.y)), Some((2, 2)));
    let before = l.clone();
    assert!(l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 0, x: 0, y: 1 }).is_err());
    assert!(l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 0, x: 3, y: 2 }).is_err());
    assert!(l.edit(&Edit::Move { target: Target::Widget, id: 99, page: 0, x: 0, y: 3 }).is_err());
    assert!(l.edit(&Edit::Move { target: Target::Rack, id: 2, page: 0, x: 0, y: 3 }).is_err());
    assert_eq!(l, before);
    l.edit(&Edit::Move { target: Target::Widget, id: 1, page: 0, x: 0, y: 3 }).unwrap();
    assert_eq!(l.pages[0].widgets.iter().map(|w| w.id).collect::<Vec<_>>(), [1, 2], "같은 페이지 내 이동 시 위젯 순서가 바뀜");
}

/// 좌표나 크기가 `u32` 상한 부근이라 덧셈 시 오버플로되는 경우도 범위 초과로 거절한다(이동, 랙 크기, 랙 칸, 저장 파일).
#[test]
fn positions_at_the_end_of_the_number_range_are_out_of_bounds() {
    let mut l = default_layout();
    l.edit(&Edit::Add { what: "rack".into(), page: 0, settings: Map::new() }).unwrap();
    let before = l.clone();
    for (x, y) in [(u32::MAX, 0), (0, u32::MAX), (u32::MAX - 1, 3), (3, u32::MAX)] {
        assert!(l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 0, x, y }).is_err(), "위젯 이동 ({x}, {y})");
        assert!(l.edit(&Edit::Move { target: Target::Rack, id: 1, page: 0, x, y }).is_err(), "랙 이동 ({x}, {y})");
        let settings = serde_json::from_str(r#"{"host":"pc1","command":"c1"}"#).unwrap();
        assert!(l.edit(&Edit::Put { rack: 1, x, y, what: "command.run".into(), settings }).is_err(), "랙 칸 배치 ({x}, {y})");
    }
    assert_eq!(l.pages[0].racks[0].x, 2);
    assert!(l.edit(&Edit::Resize { id: 1, w: u32::MAX - 1, h: 1 }).is_err());
    assert!(l.edit(&Edit::Resize { id: 1, w: 1, h: u32::MAX }).is_err());
    assert_eq!(l, before);
    l.pages[0].widgets[1].x = u32::MAX;
    assert!(l.check().is_err());
    l.pages[0].widgets[1].x = 0;
    l.pages[0].widgets[1].id = u32::MAX;
    l.check().unwrap();
    assert!(l.edit(&Edit::Add { what: "media.player".into(), page: 0, settings: Map::new() }).is_err());
}

#[test]
fn moving_to_the_page_after_the_last_makes_that_page_and_tidy_drops_empty_ones() {
    let mut l = default_layout();
    l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 1, x: 0, y: 0 }).unwrap();
    assert_eq!(l.pages.len(), 2);
    assert_eq!(l.pages[1].name, "Page 2");
    assert_eq!(spots(&l), [(0, "widget", 1, (0, 0, 4, 2)), (1, "widget", 2, (0, 0, 2, 1))]);
    assert!(l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 3, x: 0, y: 0 }).is_err());
    l.edit(&Edit::Move { target: Target::Widget, id: 1, page: 1, x: 0, y: 1 }).unwrap();
    l.edit(&Edit::Tidy).unwrap();
    assert_eq!(l.pages.len(), 1);
    assert_eq!(l.pages[0].widgets.len(), 2);
    l.edit(&Edit::Remove { target: Target::Widget, id: 1 }).unwrap();
    l.edit(&Edit::Remove { target: Target::Widget, id: 2 }).unwrap();
    l.edit(&Edit::Tidy).unwrap();
    assert_eq!((l.pages.len(), l.pages[0].is_empty()), (1, true));
}

#[test]
fn adding_takes_the_first_free_spot_and_spills_to_a_new_page() {
    let mut l = default_layout();
    let settings: Map<String, Value> = serde_json::from_str(r#"{"host":"abcd","junk":"x","n":3}"#).unwrap();
    l.edit(&Edit::Add { what: "media.player".into(), page: 0, settings }).unwrap();
    let added = widget_of(&l, 3).unwrap();
    assert_eq!((added.x, added.y, added.w, added.h, added.label.as_str()), (0, 3, 4, 2, "MEDIA"));
    assert_eq!(serde_json::to_string(&added.settings).unwrap(), r#"{"host":"abcd"}"#);
    l.edit(&Edit::Add { what: "rack".into(), page: 0, settings: Map::new() }).unwrap();
    assert_eq!(spots(&l).last(), Some(&(0, "rack", 1, (2, 2, 2, 1))));
    let settings: Map<String, Value> = serde_json::from_str(r#"{"host":"ab\ncd"}"#).unwrap();
    l.edit(&Edit::Add { what: "media.player".into(), page: 0, settings }).unwrap();
    assert_eq!((l.pages.len(), l.pages[1].widgets[0].id, l.pages[1].widgets[0].settings.len()), (2, 4, 0));
    assert!(l.edit(&Edit::Add { what: "nope".into(), page: 0, settings: Map::new() }).is_err());
    assert!(l.edit(&Edit::Add { what: "button".into(), page: 0, settings: Map::new() }).is_err());
}

#[test]
fn racks_resize_and_rename_within_the_rules() {
    let mut l = sample_layout();
    assert!(l.edit(&Edit::Resize { id: 1, w: 1, h: 1 }).is_err());
    assert!(l.edit(&Edit::Resize { id: 1, w: 2, h: 2 }).is_err());
    l.edit(&Edit::Resize { id: 4, w: 4, h: 1 }).unwrap();
    l.edit(&Edit::Rename { id: 4, label: "  Tools ".into() }).unwrap();
    let rack = l.pages[0].racks.iter().find(|r| r.id == 4).unwrap();
    assert_eq!((rack.w, rack.label.as_str()), (4, "Tools"));
    assert!(l.edit(&Edit::Rename { id: 4, label: "x".repeat(LABEL_MAX + 1) }).is_err());
    assert!(l.edit(&Edit::Rename { id: 77, label: "x".into() }).is_err());
    l.edit(&Edit::Move { target: Target::Rack, id: 5, page: 1, x: 0, y: 3 }).unwrap();
    assert_eq!(l.pages[1].racks[0].widgets.len(), 8);
    l.edit(&Edit::Remove { target: Target::Rack, id: 5 }).unwrap();
    assert!(widget_of(&l, 101).is_none());
    l.edit(&Edit::Reset).unwrap();
    assert_eq!(l, default_layout());
}

#[test]
fn the_catalog_says_where_each_kind_goes() {
    for app in catalog() {
        assert!(!app.widgets.is_empty(), "{}", app.id);
        for k in app.widgets {
            assert_eq!(k.form, if k.kind.is_composite() { "widget" } else { "button" });
            assert!(k.w <= GRID_COLS && k.h <= GRID_ROWS && (k.kind.is_composite() || (k.w, k.h) == (1, 1)));
            assert!(serde_json::to_string(&k.kind).unwrap().starts_with(&format!("\"{}.", app.id)));
        }
    }
}

/// `command` 앱의 각 위젯 종류는 대응하는 명령 타입만 선택 가능하도록 필터(`filter`)를 가지며, 모두 랙 내부 칸에 배치된다.
#[test]
fn each_command_kind_picks_only_its_own_kind_of_command() {
    let app = catalog().into_iter().find(|a| a.id == "command").unwrap();
    let filters: Vec<(String, &str)> = app.widgets.iter().map(|k| (serde_json::to_string(&k.kind).unwrap(), k.settings.iter().find(|s| s.kind == "command").unwrap().filter)).collect();
    assert_eq!(filters, [("\"command.run\"".to_string(), "run"), ("\"command.switch\"".into(), "switch"), ("\"command.stepped\"".into(), "stepped"), ("\"command.linear\"".into(), "linear")]);
    let mut l = default_layout();
    l.edit(&Edit::Add { what: "rack".into(), page: 0, settings: Map::new() }).unwrap();
    l.edit(&Edit::Resize { id: 1, w: 2, h: 2 }).unwrap();
    for (i, what) in ["command.switch", "command.stepped", "command.linear"].into_iter().enumerate() {
        let settings = serde_json::from_str(r#"{"host":"pc1","command":"c0ffee00","label":"VOL"}"#).unwrap();
        l.edit(&Edit::Put { rack: 1, x: i as u32 % 2, y: i as u32 / 2, what: what.into(), settings }).unwrap();
        assert!(l.edit(&Edit::Add { what: what.into(), page: 0, settings: Map::new() }).is_err());
    }
    assert_eq!(l.pages[0].racks[0].widgets.iter().map(|w| w.kind).collect::<Vec<_>>(), [Kind::CommandSwitch, Kind::CommandStepped, Kind::CommandLinear]);
}

#[test]
fn command_buttons_go_into_rack_cells() {
    let mut l = default_layout();
    l.edit(&Edit::Add { what: "rack".into(), page: 0, settings: Map::new() }).unwrap();
    let put = |x: u32, y: u32, json: &str| Edit::Put { rack: 1, x, y, what: "command.run".into(), settings: serde_json::from_str(json).unwrap() };
    l.edit(&put(0, 0, r#"{"host":"pc1","command":"c0ffee00","label":" Term ","color":"green","junk":"x"}"#)).unwrap();
    let b = widget_of(&l, 3).unwrap();
    assert_eq!((b.kind, b.x, b.y, b.label.as_str(), b.color.as_str()), (Kind::CommandRun, 0, 0, "Term", "green"));
    assert_eq!(serde_json::to_string(&b.settings).unwrap(), r#"{"command":"c0ffee00","host":"pc1"}"#);
    assert!(l.edit(&put(0, 0, r#"{"host":"pc1","command":"c2"}"#)).is_err());
    assert!(l.edit(&put(2, 0, r#"{"host":"pc1","command":"c2"}"#)).is_err());
    assert!(l.edit(&Edit::Put { rack: 9, x: 0, y: 0, what: "command.run".into(), settings: Map::new() }).is_err());
    assert!(l.edit(&put(1, 0, r#"{"host":"pc1"}"#)).is_err());
    assert!(l.edit(&put(1, 0, r#"{"command":"c2"}"#)).is_err());
    assert!(l.edit(&Edit::Put { rack: 1, x: 1, y: 0, what: "media.player".into(), settings: Map::new() }).is_err());
    assert!(l.edit(&Edit::Add { what: "command.run".into(), page: 0, settings: Map::new() }).is_err());
    l.edit(&put(1, 0, &format!(r#"{{"host":"pc1","command":"c2","label":"{}","color":7}}"#, "x".repeat(LABEL_MAX + 1)))).unwrap();
    let b = widget_of(&l, 4).unwrap();
    assert_eq!((b.label.as_str(), b.color.as_str(), b.settings.len()), ("", "", 2));
    assert!(l.edit(&Edit::Resize { id: 1, w: 1, h: 1 }).is_err());
    l.edit(&Edit::Remove { target: Target::Button, id: 4 }).unwrap();
    l.edit(&Edit::Resize { id: 1, w: 1, h: 1 }).unwrap();
    assert!(l.edit(&Edit::Remove { target: Target::Button, id: 4 }).is_err());
    assert!(widget_of(&l, 3).is_some());
}

/// 자원 미터: 크기에 따라 최대 미터 수가 다르며 대상 호스트(`@deck` 또는 PC)와 메트릭 목록이 필수다.
/// 현재 배치에서 모니터링 중인 항목(`watched`)을 추출할 수 있다(모니터링 중인 항목만 측정).
#[test]
fn monitors_take_a_source_and_a_limited_number_of_meters() {
    let mut l = default_layout();
    assert_eq!(l.watched(), [(SELF_HOST.to_string(), vec!["cpu.temp".to_string(), "mem".into(), "load".into()])].into());
    let add = |what: &str, json: &str| Edit::Add { what: what.into(), page: 0, settings: serde_json::from_str(json).unwrap() };
    assert!(l.edit(&add("monitor.2x2", r#"{}"#)).is_err());
    assert!(l.edit(&add("monitor.2x2", r#"{"host":"@deck","metrics":[]}"#)).is_err());
    assert!(l.edit(&add("monitor.2x2", r#"{"metrics":["cpu"]}"#)).is_err());
    l.edit(&add("monitor.2x2", r#"{"host":"pc1","metrics":["cpu","mem","cpu","","gpu","gpu.temp","net.down","net.up","disk.read","extra"]}"#)).unwrap();
    let w = widget_of(&l, 3).unwrap();
    assert_eq!((w.kind, w.w, w.h), (Kind::Monitor2x2, 2, 2));
    assert_eq!(serde_json::to_string(&w.settings["metrics"]).unwrap(), r#"["cpu","mem","gpu","gpu.temp","net.down","net.up"]"#);
    for (what, size, most) in [("monitor.2x1", (2, 1), 3), ("monitor.4x1", (4, 1), 6), ("monitor.4x2", (4, 2), 12)] {
        let info = catalog().into_iter().flat_map(|a| a.widgets).find(|k| serde_json::to_string(&k.kind).unwrap() == format!("\"{what}\"")).unwrap();
        assert_eq!(((info.w, info.h), info.settings.iter().find(|s| s.kind == "metrics").unwrap().max), (size, most));
        assert!(info.settings.iter().any(|s| s.kind == "host" && s.own && s.required));
    }
    l.edit(&add("monitor.2x1", r#"{"host":"pc1","metrics":["mem","swap"]}"#)).unwrap();
    let watched = l.watched();
    assert_eq!(watched["pc1"], ["cpu", "mem", "gpu", "gpu.temp", "net.down", "net.up", "swap"]);
    assert_eq!(watched.len(), 2);
}

#[test]
fn the_layout_survives_a_restart_and_a_broken_file_is_set_aside() {
    let dir = std::env::temp_dir().join(format!("tossd-test-layout-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let store = Store::new(dir.join("layout.json"));
    assert_eq!(store.load(), default_layout());
    let mut l = default_layout();
    l.edit(&Edit::Add { what: "rack".into(), page: 0, settings: Map::new() }).unwrap();
    l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 1, x: 1, y: 1 }).unwrap();
    store.save(&l);
    assert_eq!(store.load(), l);
    l.edit(&Edit::Move { target: Target::Widget, id: 2, page: 0, x: 0, y: 3 }).unwrap();
    assert_eq!(l.pages.len(), 2);
    store.save(&l);
    assert_eq!(store.load().pages.len(), 1);
    l.pages[0].widgets[1].y = 0;
    store.save(&l);
    assert_eq!(store.load(), default_layout());
    assert!(dir.join("layout.json.bad").exists() && !dir.join("layout.json").exists());
    fs::write(dir.join("layout.json"), "{ not json").unwrap();
    assert_eq!(store.load(), default_layout());
    fs::write(dir.join("layout.json"), serde_json::to_string(&default_layout()).unwrap().replace("media.player", "button")).unwrap();
    assert_eq!(store.load(), default_layout());
    assert!(!dir.join("layout.json").exists());
    let _ = fs::remove_dir_all(&dir);
}
