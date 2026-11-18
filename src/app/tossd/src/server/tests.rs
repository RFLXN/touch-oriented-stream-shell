//! 데몬 전체를 띄워 놓고 UI 클라이언트(WebSocket)와 호스트(`toss-proto`) 양쪽에서 통신을 검증하는 통합 테스트.

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{json, Value};
use toss_proto::secure::{Conn, Identity};
use toss_proto::{DeckMsg, HostMsg, Media, PROTO_VERSION};
use tungstenite::stream::MaybeTlsStream;

use super::*;

type Ui = WebSocket<MaybeTlsStream<TcpStream>>;

/// 대기 중인 메시지가 이 시간 내에 오지 않으면 테스트가 무한 대기하지 않고 실패한다(다른 메시지가 계속 들어와도 동일).
const PATIENCE: Duration = Duration::from_secs(30);

/// 데몬을 띄우고 UI 접속 URL을 반환한다. `wpa_supplicant`는 없는 상태로 시작한다.
fn start(store: layout::Store, host: Option<(SocketAddr, PathBuf)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    thread::spawn(move || run(listener, store, Device::default(), "/nonexistent/wpa/wlan0".into(), host));
    url
}

/// 호스트 수신용 임시 주소와 덱 키/페어링 목록을 저장할 임시 디렉터리
fn host_side(name: &str) -> (SocketAddr, PathBuf) {
    let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!("tossd-test-{name}-{}-{}", std::process::id(), addr.port()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    (addr, dir)
}

/// UI 클라이언트로 접속
fn connect(url: &str) -> Ui {
    let (ui, _) = tungstenite::connect(url).unwrap();
    if let MaybeTlsStream::Plain(s) = ui.get_ref() {
        s.set_read_timeout(Some(PATIENCE)).unwrap();
    }
    ui
}

fn read(ui: &mut Ui) -> Value {
    loop {
        if let Message::Text(t) = ui.read().expect("대기하던 메시지가 도착하지 않음") {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

/// 지정한 `type`이면서 `pred`를 만족하는 메시지가 올 때까지 읽는다.
fn next_when(ui: &mut Ui, kind: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let until = Instant::now() + PATIENCE;
    loop {
        let v = read(ui);
        if v["type"] == kind && pred(&v) {
            return v;
        }
        assert!(Instant::now() < until, "대기하던 {kind} 메시지가 오지 않음");
    }
}

fn next(ui: &mut Ui, kind: &str) -> Value {
    next_when(ui, kind, |_| true)
}

fn say(ui: &mut Ui, msg: Value) {
    ui.send(Message::text(msg.to_string())).unwrap();
}

/// 호스트 역할로 덱에 연결된 클라이언트
struct Host {
    conn: Arc<Conn>,
    reader: TcpStream,
    /// 덱의 `hello` 응답
    hello: DeckMsg,
}

impl Host {
    /// 덱에 접속해 `hello`를 보내고 덱의 `hello`를 받는다. 이후 백그라운드로 주기적인 `ping`을 전송한다.
    fn attach(addr: SocketAddr, me: &Identity, hello: HostMsg) -> Host {
        let stream = loop {
            match TcpStream::connect(addr) {
                Ok(s) => break s,
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        };
        let (conn, mut reader) = Conn::establish(stream, me, true).unwrap();
        let conn = Arc::new(conn);
        conn.send(&hello).unwrap();
        let hello = conn.recv::<DeckMsg>(&mut reader).unwrap().expect("hello");
        let pinger = conn.clone();
        thread::spawn(move || {
            while pinger.send(&HostMsg::Ping).is_ok() {
                thread::sleep(Duration::from_millis(100));
            }
        });
        Host { conn, reader, hello }
    }

    fn status(&self) -> &str {
        match &self.hello {
            DeckMsg::Hello { status, .. } => status,
            _ => panic!("Hello 메시지가 아님"),
        }
    }

    fn send(&self, msg: HostMsg) {
        self.conn.send(&msg).unwrap();
    }

    /// 덱이 보낸 메시지 중 `want` 조건을 만족하는 첫 번째 메시지를 반환(나머지는 건너뜀)
    fn next_that<T>(&mut self, want: impl Fn(DeckMsg) -> Option<T>) -> T {
        let until = Instant::now() + PATIENCE;
        loop {
            if let Some(found) = self.conn.recv::<DeckMsg>(&mut self.reader).unwrap().and_then(&want) {
                return found;
            }
            assert!(Instant::now() < until, "덱이 기대한 메시지를 보내지 않음");
        }
    }

    /// 덱이 보낸 다음 메시지(`Ping`, `Watch`, `VaultState`는 건너뜀)
    fn next(&mut self) -> DeckMsg {
        self.next_that(|m| (!matches!(m, DeckMsg::Ping | DeckMsg::Watch { .. } | DeckMsg::VaultState { .. })).then_some(m))
    }

    fn next_watch(&mut self) -> Vec<String> {
        self.next_that(|m| match m {
            DeckMsg::Watch { metrics } => Some(metrics),
            _ => None,
        })
    }
}

fn hello(pair: bool) -> HostMsg {
    HostMsg::Hello { proto: PROTO_VERSION, name: "pc".into(), os: "linux".into(), caps: vec!["media.info".into(), "media.control".into()], pair }
}

/// 새로 접속한 UI 클라이언트는 전체 초기 상태를 받는다. 잘못된 메시지는 무시되고 연결은 유지된다.
#[test]
fn a_new_screen_gets_the_whole_state_and_nonsense_is_ignored() {
    let url = start(layout::Store::default(), None);
    let (mut a, mut b) = (connect(&url), connect(&url));
    let wanted = ["hello", "catalog", "state", "wifi", "layout", "device", "hosts", "values", "meters", "meter_values"];
    let mut got: BTreeMap<String, Value> = BTreeMap::new();
    let until = Instant::now() + PATIENCE;
    while !(wanted.iter().all(|k| got.contains_key(*k)) && got["state"]["net"]["eth"]["name"] == "eth0") {
        let v = read(&mut a);
        got.insert(v["type"].as_str().unwrap().to_string(), v);
        assert!(Instant::now() < until, "초기 상태 메시지가 모두 도착하지 않음: {:?}", got.keys());
    }
    assert_eq!(got.len(), wanted.len(), "{:?}", got.keys());
    assert_eq!(got["hello"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(got["catalog"]["apps"][0]["widgets"][0]["kind"], "media.player");
    assert_eq!((&got["layout"]["cols"], &got["layout"]["rows"], got["layout"]["pages"].as_array().unwrap().len()), (&Value::from(4), &Value::from(6), 1));
    assert_eq!(got["layout"]["pages"][0]["widgets"][0]["kind"], "media.player");
    let state = &got["state"];
    assert!(state["host"].is_string() && state["ip"].is_string() && state["links"]["usb"].is_boolean() && state["net"]["wlan"]["addr"].is_string());
    assert_eq!(state.as_object().unwrap().len(), 5, "불필요한 필드가 state에 포함됨: {state}");
    assert_eq!(got["wifi"]["available"], false);
    assert_eq!((&got["device"]["can_power"], &got["device"]["can_ssh"]), (&Value::from(false), &Value::from(false)));
    assert_eq!((got["hosts"]["hosts"].as_array().unwrap().len(), &got["hosts"]["pairing"]["active"]), (0, &Value::from(false)));

    next(&mut b, "layout");
    a.send(Message::text(r#"{"type":"nonsense"}"#)).unwrap();
    a.send(Message::text(r#"{"type":"press","id":7}"#)).unwrap();
    a.send(Message::text("not json")).unwrap();
    say(&mut a, json!({ "type": "layout_tidy" }));
    assert_eq!(next(&mut a, "layout")["pages"].as_array().unwrap().len(), 1);
    assert_eq!(next(&mut b, "layout")["pages"].as_array().unwrap().len(), 1);
}

/// 전체 페어링 및 연동 흐름: 미페어링 호스트는 거절되고, 페어링 모드에서 양쪽 확인 시 페어링이 완료되며,
/// 이후 호스트 상태가 UI에 반영되고 UI 액션이 호스트로 전달된다. 페어링 정보는 파일에 영구 저장된다.
#[test]
fn hosts_pair_then_exchange_state_and_actions() {
    let (host_addr, dir) = host_side("hosts");
    let url = start(layout::Store::default(), Some((host_addr, dir.clone())));
    let mut ui = connect(&url);
    let me = Identity::generate().unwrap();

    assert_eq!(next(&mut ui, "hosts")["hosts"].as_array().unwrap().len(), 0);

    let mut host = Host::attach(host_addr, &me, hello(true));
    assert_eq!(host.status(), "denied");
    assert_eq!(host.next(), DeckMsg::Denied { reason: "not_paired".into() });

    say(&mut ui, json!({ "type": "pair_start" }));
    next_when(&mut ui, "hosts", |v| v["pairing"]["active"] == true);
    let mut host = Host::attach(host_addr, &me, hello(true));
    assert_eq!(host.status(), "pairing");
    let v = next_when(&mut ui, "hosts", |v| v["pairing"]["candidate"].is_object());
    assert_eq!(v["pairing"]["candidate"]["code"], host.conn.code.as_str());
    assert_eq!(v["pairing"]["candidate"]["name"], "pc");

    host.send(HostMsg::PairConfirm);
    next_when(&mut ui, "hosts", |v| v["pairing"]["candidate"]["host_confirmed"] == true);
    assert!(!dir.join("hosts.json").exists());
    say(&mut ui, json!({ "type": "pair_accept", "code": host.conn.code }));
    assert_eq!(host.next(), DeckMsg::Paired);
    let v = next_when(&mut ui, "hosts", |v| v["hosts"][0]["connected"] == true);
    assert_eq!((&v["hosts"][0]["id"], &v["pairing"]["active"], &v["pairing"]["candidate"]), (&Value::from(me.id()), &Value::from(false), &Value::Null));
    assert!(std::fs::read_to_string(dir.join("hosts.json")).unwrap().contains(&me.id()));

    let media = Media { title: "곡".into(), status: "playing".into(), ..Media::default() };
    host.send(HostMsg::Media { media: Some(media) });
    next_when(&mut ui, "hosts", |v| v["hosts"][0]["media"]["title"] == "곡");

    let media = Media { title: "곡".into(), status: "playing".into(), position_ms: 10_000, length_ms: 300_000, art: "a1".into(), ..Media::default() };
    host.send(HostMsg::Media { media: Some(media) });
    let piece = "QUJD".repeat(toss_proto::BLOB_CHUNK / 3);
    host.send(HostMsg::Blob { id: "a1".into(), mime: "image/jpeg".into(), index: 0, count: 2, data: piece.clone() });
    host.send(HostMsg::Blob { id: "a1".into(), mime: "image/jpeg".into(), index: 1, count: 2, data: "QUI=".into() });
    let v = next_when(&mut ui, "hosts", |v| v["hosts"][0]["art"].is_object());
    assert_eq!((&v["hosts"][0]["art"]["id"], &v["hosts"][0]["art"]["mime"]), (&Value::from("a1"), &Value::from("image/jpeg")));
    assert_eq!(v["hosts"][0]["art"]["data"].as_str().unwrap(), format!("{piece}QUI="));
    thread::sleep(Duration::from_millis(1100));
    say(&mut ui, json!({ "type": "action", "name": "media.next" }));
    assert!(matches!(host.next(), DeckMsg::Action { name, .. } if name == "media.next"));

    let knob = toss_proto::CommandInfo { kind: toss_proto::KIND_LINEAR.into(), max: 100.0, step: 5.0, ..toss_proto::CommandInfo::run("b0b0b0b0", "Volume") };
    host.send(HostMsg::Commands { commands: vec![toss_proto::CommandInfo::run("c0ffee00", "Terminal"), knob] });
    let v = next_when(&mut ui, "hosts", |v| v["hosts"][0]["commands"][0].is_object());
    assert_eq!((&v["hosts"][0]["commands"][0]["name"], &v["hosts"][0]["commands"][0]["kind"]), (&Value::from("Terminal"), &Value::from("run")));
    assert_eq!((&v["hosts"][0]["commands"][1]["kind"], &v["hosts"][0]["commands"][1]["max"]), (&Value::from("linear"), &Value::from(100.0)));
    let values_are = |ui: &mut Ui, want: Value| next_when(ui, "values", |v| v["hosts"] == want);
    host.send(HostMsg::CommandValue { id: "b0b0b0b0".into(), value: Some(35.0) });
    values_are(&mut ui, json!({ me.id(): { "b0b0b0b0": 35.0 } }));
    host.send(HostMsg::CommandValue { id: "b0b0b0b0".into(), value: None });
    values_are(&mut ui, json!({}));
    host.send(HostMsg::CommandValue { id: "b0b0b0b0".into(), value: Some(40.0) });
    values_are(&mut ui, json!({ me.id(): { "b0b0b0b0": 40.0 } }));
    say(&mut ui, json!({ "type": "action", "name": "command.set", "args": { "id": "b0b0b0b0", "value": 45 } }));
    let DeckMsg::Action { name, args, .. } = host.next() else { panic!("Action 메시지가 아님") };
    assert_eq!((name.as_str(), &args["value"]), ("command.set", &Value::from(45)));
    host.send(HostMsg::Commands { commands: vec![toss_proto::CommandInfo::run("c0ffee00", "Terminal")] });
    values_are(&mut ui, json!({}));

    let metric = |id: &str, unit: &str, max: f64| toss_proto::MetricInfo { id: id.into(), name: id.to_uppercase(), title: id.into(), unit: unit.into(), max, decimals: 0 };
    host.send(HostMsg::Metrics { metrics: vec![metric("cpu", "%", 100.0), metric("mem", "B", 1e9)] });
    let v = next_when(&mut ui, "meters", |v| v["hosts"][me.id()].is_array());
    assert_eq!((&v["hosts"][me.id()][1]["id"], &v["hosts"][me.id()][1]["unit"]), (&Value::from("mem"), &Value::from("B")));
    assert!(v["hosts"]["@deck"].is_array(), "호스트 메트릭 수신 시 덱 자체 메트릭이 유실됨");
    say(&mut ui, json!({ "type": "layout_add", "what": "monitor.2x1", "page": 0, "settings": { "host": me.id(), "metrics": ["mem", "cpu", "mem", ""] } }));
    assert_eq!(host.next_watch(), ["mem", "cpu"]);
    host.send(HostMsg::MetricValues { values: [("cpu".to_string(), 37.0), ("ghost".to_string(), 1.0)].into() });
    let v = next_when(&mut ui, "meter_values", |v| v["hosts"][me.id()]["cpu"] == 37.0);
    assert_eq!(v["hosts"][me.id()]["ghost"], Value::Null);
    host.send(HostMsg::Metrics { metrics: Vec::new() });
    next_when(&mut ui, "meters", |v| v["hosts"][me.id()].is_null());

    say(&mut ui, json!({ "type": "action", "name": "command.run", "args": { "id": "c0ffee00" }, "widget": 41 }));
    let DeckMsg::Action { id, name, args } = host.next() else { panic!("Action 메시지가 아님") };
    assert_eq!((name.as_str(), &args["id"]), ("command.run", &Value::from("c0ffee00")));
    host.send(HostMsg::Sleep);
    assert_eq!(host.next(), DeckMsg::Sleeping { ok: false, reason: "no_wifi".into() });
    host.send(HostMsg::Result { id, ok: false, error: "not_allowed".into() });
    let result = next(&mut ui, "result");
    assert_eq!((&result["widget"], &result["ok"], &result["error"]), (&Value::from(41), &Value::from(false), &Value::from("not_allowed")));

    host.send(HostMsg::Caps { caps: vec!["media.info".into()] });
    let v = next_when(&mut ui, "hosts", |v| v["hosts"][0]["caps"] == json!(["media.info"]));
    let pos = v["hosts"][0]["media"]["position_ms"].as_u64().unwrap();
    assert!((11_000..14_000).contains(&pos), "재생 위치가 시간에 따라 진행되지 않음: {pos}");
    {
        let mut ui2 = connect(&url);
        let pos2 = next_when(&mut ui2, "hosts", |v| v["hosts"][0]["media"].is_object())["hosts"][0]["media"]["position_ms"].as_u64().unwrap();
        assert!(pos2 >= pos, "새 클라이언트가 이전 재생 위치를 받음: {pos2} < {pos}");
    }
    let media = Media { title: "다음 곡".into(), status: "paused".into(), art: "b2".into(), ..Media::default() };
    host.send(HostMsg::Media { media: Some(media) });
    assert_eq!(next_when(&mut ui, "hosts", |v| v["hosts"][0]["media"]["title"] == "다음 곡")["hosts"][0]["art"], Value::Null);

    host.conn.shutdown();
    assert_eq!(next_when(&mut ui, "hosts", |v| v["hosts"][0]["connected"] == false)["hosts"][0]["media"], Value::Null);
    let mut host = Host::attach(host_addr, &me, hello(false));
    assert_eq!(host.status(), "paired");
    next_when(&mut ui, "hosts", |v| v["hosts"][0]["connected"] == true);

    assert_eq!(Host::attach(host_addr, &Identity::generate().unwrap(), hello(false)).status(), "denied");

    say(&mut ui, json!({ "type": "host_forget", "id": me.id() }));
    assert_eq!(host.next(), DeckMsg::Denied { reason: "forgotten".into() });
    next_when(&mut ui, "hosts", |v| v["hosts"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 페어링 경계 사례:
///   - 거절된 호스트로 보내는 `hello` 응답에는 덱 주소와 WoWLAN MAC 주소가 비어 있다. 프로토콜 버전이 달라도 `hello`(덱 버전)는 반환된다.
///   - 호스트가 보낸 이름과 `caps`는 정제되어 UI로 전달된다.
///   - UI에 표시된 것과 다른 코드의 페어링 수락 요청은 무시된다.
///   - 호스트가 `paired`를 받자마자 연결을 닫아도(`toss-connector pair` 동작) 후보 슬롯이 비워져 다른 호스트가 이어서 페어링할 수 있다.
///   - 액션 결과는 해당 액션을 받은 호스트의 응답만 반영한다. 연결되지 않은 호스트로 보낸 액션은 즉시 실패 처리된다.
#[test]
fn pairing_frees_the_candidate_and_strangers_learn_nothing() {
    let (host_addr, dir) = host_side("pair");
    let url = start(layout::Store::default(), Some((host_addr, dir.clone())));
    let mut ui = connect(&url);
    let odd_hello = |proto: u32, pair: bool| HostMsg::Hello { proto, name: "pc\u{1b}[31m\n".to_string() + &"x".repeat(500), os: "linux".into(), caps: vec!["media.control".into(), "bad cap\n".into()], pair };
    let closed = |hello: &DeckMsg| matches!(hello, DeckMsg::Hello { proto: PROTO_VERSION, status, addrs, wake, .. } if status == "denied" && addrs.is_empty() && wake.is_empty());
    let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());

    let mut host = Host::attach(host_addr, &a, odd_hello(PROTO_VERSION, true));
    assert!(closed(&host.hello), "{:?}", host.hello);
    assert_eq!(host.next(), DeckMsg::Denied { reason: "not_paired".into() });
    let host = Host::attach(host_addr, &a, odd_hello(PROTO_VERSION + 1, false));
    assert!(closed(&host.hello), "{:?}", host.hello);

    for me in [&a, &b] {
        say(&mut ui, json!({ "type": "pair_start" }));
        next_when(&mut ui, "hosts", |v| v["pairing"]["active"] == true);
        let mut host = Host::attach(host_addr, me, odd_hello(PROTO_VERSION, true));
        assert_eq!(host.status(), "pairing");
        let v = next_when(&mut ui, "hosts", |v| v["pairing"]["candidate"].is_object());
        assert_eq!(v["pairing"]["candidate"]["name"].as_str().unwrap(), format!("pc[31m{}", "x".repeat(58)));
        say(&mut ui, json!({ "type": "pair_accept", "code": "000 00x" }));
        host.send(HostMsg::PairConfirm);
        let v = next_when(&mut ui, "hosts", |v| v["pairing"]["candidate"]["host_confirmed"] == true || v["pairing"]["candidate"].is_null());
        assert_eq!(v["pairing"]["candidate"]["deck_accepted"], false, "일치하지 않는 검증 코드 수락이 허용됨");
        say(&mut ui, json!({ "type": "pair_accept", "code": host.conn.code }));
        assert_eq!(host.next(), DeckMsg::Paired);
        host.conn.shutdown();
        next_when(&mut ui, "hosts", |v| v["pairing"]["candidate"].is_null() && v["pairing"]["active"] == false && v["hosts"].as_array().unwrap().iter().any(|h| h["id"] == me.id()));
    }
    assert!(std::fs::read_to_string(dir.join("hosts.json")).unwrap().contains(&b.id()));

    let mut host_a = Host::attach(host_addr, &a, odd_hello(PROTO_VERSION, false));
    let host_b = Host::attach(host_addr, &b, odd_hello(PROTO_VERSION, false));
    let v = next_when(&mut ui, "hosts", |v| v["hosts"].as_array().unwrap().iter().all(|h| h["connected"] == true));
    assert_eq!(v["hosts"][0]["caps"], json!(["media.control"]));
    say(&mut ui, json!({ "type": "action", "host": a.id(), "name": "media.next", "widget": 7 }));
    let DeckMsg::Action { id, .. } = host_a.next() else { panic!("Action 메시지가 아님") };
    host_b.send(HostMsg::Result { id, ok: true, error: String::new() });
    thread::sleep(Duration::from_millis(150));
    host_a.send(HostMsg::Result { id, ok: false, error: "busy".into() });
    let result = next(&mut ui, "result");
    assert_eq!((&result["widget"], &result["ok"], &result["error"]), (&Value::from(7), &Value::from(false), &Value::from("busy")));
    say(&mut ui, json!({ "type": "action", "host": "nobody", "name": "media.next", "widget": 8 }));
    assert_eq!(next_when(&mut ui, "result", |v| v["widget"] == 8)["error"], "no_host");
    let _ = std::fs::remove_dir_all(&dir);
}

/// UI에서 배치를 수정하면 규칙에 맞는 요청만 수락해 저장하고 모든 UI 클라이언트에 새 배치를 브로드캐스트한다.
/// 규칙에 어긋나는 요청은 거절하고 현재 배치를 다시 보낸다(파싱 불가 메시지도 동일). 재시작 시 저장된 배치로 복원된다.
#[test]
fn layout_edits_are_checked_saved_and_broadcast() {
    let dir = std::env::temp_dir().join(format!("tossd-test-edit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("layout.json");
    let url = start(layout::Store::new(file.clone()), None);
    let (mut a, mut b) = (connect(&url), connect(&url));
    let catalog = next(&mut a, "catalog");
    assert_eq!(catalog["apps"][0]["widgets"][0]["kind"], "media.player");
    assert_eq!(catalog["apps"][0]["widgets"][0]["settings"][0]["type"], "host");
    assert_eq!(next(&mut a, "layout")["pages"][0]["widgets"].as_array().unwrap().len(), 2);
    next(&mut b, "layout");

    say(&mut a, json!({ "type": "layout_move", "target": "widget", "id": 2, "page": 0, "x": 2, "y": 3 }));
    for ui in [&mut a, &mut b] {
        let l = next(ui, "layout");
        assert_eq!((&l["pages"][0]["widgets"][1]["x"], &l["pages"][0]["widgets"][1]["y"]), (&Value::from(2), &Value::from(3)));
    }
    say(&mut a, json!({ "type": "layout_move", "target": "widget", "id": 2, "page": 0, "x": 0, "y": 1 }));
    assert_eq!(next(&mut a, "layout")["pages"][0]["widgets"][1]["y"], 3);
    say(&mut a, json!({ "type": "layout_move", "target": "widget", "id": 2, "page": 0, "x": 0.5, "y": 4 }));
    assert_eq!(next(&mut a, "layout")["pages"][0]["widgets"][1]["y"], 3);
    say(&mut a, json!({ "type": "layout_add", "what": "rack", "page": 0 }));
    assert_eq!(next(&mut a, "layout")["pages"][0]["racks"][0]["label"], "RACK");
    say(&mut a, json!({ "type": "layout_rename", "id": 1, "label": "TOOLS" }));
    assert_eq!(next(&mut a, "layout")["pages"][0]["racks"][0]["label"], "TOOLS");
    say(&mut a, json!({ "type": "layout_add", "what": "media.player", "page": 0, "settings": { "host": "abcd" } }));
    let l = next(&mut a, "layout");
    assert_eq!(l["pages"][0]["widgets"][2]["settings"]["host"], "abcd");

    let url = start(layout::Store::new(file), None);
    let mut c = connect(&url);
    let l = next(&mut c, "layout");
    assert_eq!(l["pages"][0]["racks"][0]["label"], "TOOLS");
    assert_eq!(l["pages"][0]["widgets"].as_array().unwrap().len(), 3);
    say(&mut c, json!({ "type": "layout_reset" }));
    let l = next(&mut c, "layout");
    assert_eq!((l["pages"][0]["widgets"].as_array().unwrap().len(), l["pages"][0]["racks"].as_array().unwrap().len()), (2, 0));
    let _ = std::fs::remove_dir_all(&dir);
}

/// 핸드셰이크 요청이 다소 지연되는 클라이언트도 허용한다.
/// (UI 초기 기동 시 부하로 인해 요청이 100ms 이상 지연될 수 있는데, 폴링 주기와 같은 타임아웃을 적용했다가 첫 연결이 계속 실패했던 적이 있다.)
#[test]
fn slow_handshake_is_accepted() {
    let url = start(layout::Store::default(), None);
    let stream = TcpStream::connect(url.trim_start_matches("ws://")).unwrap();
    thread::sleep(POLL_INTERVAL * 4);
    let (mut ws, _) = tungstenite::client(url.as_str(), stream).unwrap();
    let Message::Text(t) = ws.read().unwrap() else { panic!("텍스트 메시지가 아님") };
    assert!(t.as_str().contains(r#""type":"hello""#));
}

/// 페어링된 호스트가 금고를 쓰려 하면 화면이 묻고, 허용·해제·삭제가 호스트와 화면 양쪽에 반영된다. 키 값은 어느 쪽에도 나가지 않는다.
#[test]
fn the_vault_asks_the_screen_and_tells_both_sides() {
    let (host_addr, dir) = host_side("vault");
    let url = start(layout::Store::default(), Some((host_addr, dir.clone())));
    let mut ui = connect(&url);
    let me = Identity::generate().unwrap();
    say(&mut ui, json!({ "type": "pair_start" }));
    next_when(&mut ui, "hosts", |v| v["pairing"]["active"] == true);
    let mut host = Host::attach(host_addr, &me, hello(true));
    host.send(HostMsg::PairConfirm);
    next_when(&mut ui, "hosts", |v| v["pairing"]["candidate"]["host_confirmed"] == true);
    say(&mut ui, json!({ "type": "pair_accept", "code": host.conn.code }));
    assert_eq!(host.next(), DeckMsg::Paired);
    let vault_state = |host: &mut Host| host.next_that(|m| match m {
        DeckMsg::VaultState { services } => Some(services),
        _ => None,
    });
    let reply = |host: &mut Host| host.next_that(|m| match m {
        DeckMsg::VaultReply { id, ok, error, data } => Some((id, ok, error, data)),
        _ => None,
    });

    assert!(!vault_state(&mut host)[0].registered);
    let put = json!({ "client_id": "123", "client_secret": "never-shown", "redirect_uri": "" });
    host.send(HostMsg::Vault { id: 1, op: "put".into(), service: "discord".into(), args: put });
    let v = next_when(&mut ui, "vault", |v| v["asking"].is_object());
    assert_eq!((&v["asking"]["host"], &v["asking"]["name"], &v["asking"]["title"]), (&Value::from(me.id()), &Value::from("pc"), &Value::from("Discord")));
    say(&mut ui, json!({ "type": "vault_answer", "allow": true }));
    assert_eq!(reply(&mut host), (1, true, String::new(), json!({ "ok": true })));
    let v = next_when(&mut ui, "vault", |v| v["services"][0]["registered"] == true && v["asking"].is_null());
    assert_eq!(v["services"][0]["hosts"], json!([me.id()]));
    assert!(!v.to_string().contains("never-shown"));
    let s = loop {
        let s = vault_state(&mut host);
        if s[0].registered && s[0].allowed {
            break s;
        }
    };
    assert!(!s[0].linked);

    host.send(HostMsg::Vault { id: 2, op: "begin".into(), service: "discord".into(), args: Value::Null });
    let (_, ok, _, data) = reply(&mut host);
    assert!(ok && data["client_id"] == "123" && !data.to_string().contains("never-shown"));
    host.send(HostMsg::Vault { id: 3, op: "token".into(), service: "discord".into(), args: Value::Null });
    assert_eq!(reply(&mut host), (3, false, "not_linked".into(), Value::Null));

    say(&mut ui, json!({ "type": "vault_revoke", "service": "discord", "host": me.id() }));
    while vault_state(&mut host)[0].allowed {}
    say(&mut ui, json!({ "type": "vault_forget", "service": "discord" }));
    next_when(&mut ui, "vault", |v| v["services"][0]["registered"] == false);
    host.send(HostMsg::Vault { id: 4, op: "begin".into(), service: "discord".into(), args: Value::Null });
    next_when(&mut ui, "vault", |v| v["asking"].is_object());
    host.conn.shutdown();
    next_when(&mut ui, "vault", |v| v["asking"].is_null());
    let _ = std::fs::remove_dir_all(&dir);
}
