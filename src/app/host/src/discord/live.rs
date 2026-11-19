//! Discord 상주 연결: 쓰는 곳(`hold`)이 있는 동안 Discord 앱과 IPC 연결을 하나 유지하고, 구독한 이벤트의 최신 값을 기억한다.
//!
//! 연결 절차: Discord 앱의 소켓이 있는지 먼저 본다(앱이 꺼져 있으면 덱에 묻지 않는다) → 덱에서 공개 값과 토큰을 빌린다
//! → `AUTHENTICATE` → `FEEDS`마다 현재 값을 읽어 두고 변경 이벤트를 구독한다.
//! 연결이 끊어지거나(Discord 재시작) 실패하면 `RETRY_MIN`부터 두 배씩 `RETRY_MAX`까지 늘리며 다시 시도한다.
//! 덱이 금고 사용을 거절했으면 덱이 다시 묻지 않는 동안(5분) 기다린다.
//!
//! 연결은 작업 스레드 하나가 소유한다. 다른 스레드의 명령(`call`)은 채널로 넘기고, 작업 스레드는 명령과 이벤트를 번갈아 처리한다.
//! 토큰은 연결할 때 한 번 빌려 인증에만 쓰고 보관하지 않는다.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use toss_proto::{SERVICE_DISCORD, VAULT_TOKEN};

use super::ipc::{self, Ipc};
use super::{begin, borrowed, REPLY_WAIT, VOICE};
use crate::vault;

/// 연결 직후 현재 값을 읽는 명령과, 그 값이 바뀔 때 오는 이벤트
pub const FEEDS: &[(&str, &str)] = &[("GET_VOICE_SETTINGS", VOICE)];

const RETRY_MIN: Duration = Duration::from_secs(3);
const RETRY_MAX: Duration = Duration::from_secs(60);
const DENIED_WAIT: Duration = Duration::from_secs(300);
/// 작업 스레드가 이벤트를 기다리는 한 번의 시간(명령 처리 지연의 상한)
const TICK: Duration = Duration::from_millis(50);

type Reply = Sender<Result<Value, String>>;
/// 연결 실패: (사유, 다음 시도까지 기다릴 시간. `None`이면 점점 늘리는 기본 간격)
type Retry = (String, Option<Duration>);

struct Shared {
    holders: usize,
    running: bool,
    up: bool,
    error: String,
    latest: BTreeMap<String, Value>,
    requests: Option<Sender<(String, Value, Reply)>>,
}

static LIVE: Mutex<Shared> = Mutex::new(Shared { holders: 0, running: false, up: false, error: String::new(), latest: BTreeMap::new(), requests: None });

/// 연결을 유지해 달라는 표시. 마지막 표시가 사라지면 연결을 닫는다.
pub struct Hold(());

impl Drop for Hold {
    fn drop(&mut self) {
        let mut s = LIVE.lock().unwrap();
        s.holders = s.holders.saturating_sub(1);
    }
}

/// 연결을 유지하도록 표시하고, 작업 스레드가 없으면 띄운다
pub fn hold() -> Hold {
    let mut s = LIVE.lock().unwrap();
    s.holders += 1;
    if !s.running {
        s.running = true;
        thread::spawn(work);
    }
    Hold(())
}

/// 연결 상태: (유지 중인지, 연결되었는지, 마지막 오류)
pub fn status() -> (bool, bool, String) {
    let s = LIVE.lock().unwrap();
    (s.holders > 0, s.up, s.error.clone())
}

/// 구독한 이벤트의 최신 데이터(연결되어 있지 않으면 `None`)
pub fn latest(evt: &str) -> Option<Value> {
    let s = LIVE.lock().unwrap();
    if s.up {
        s.latest.get(evt).cloned()
    } else {
        None
    }
}

/// 연결을 통해 명령을 보내고 응답을 기다린다
pub fn call(cmd: &str, args: Value) -> Result<Value, String> {
    let sender = {
        let s = LIVE.lock().unwrap();
        match (&s.requests, s.up) {
            (Some(tx), true) => tx.clone(),
            _ if s.holders == 0 => return Err("Discord 연결을 쓰고 있지 않습니다 (덱이 연결되어 있고 명령 실행이 허용되어 있어야 합니다)".into()),
            _ => return Err(if s.error.is_empty() { "Discord 앱에 연결하는 중입니다".into() } else { s.error.clone() }),
        }
    };
    let (tx, rx) = mpsc::channel();
    sender.send((cmd.to_string(), args, tx)).map_err(|_| "Discord 연결이 끊어졌습니다".to_string())?;
    rx.recv_timeout(REPLY_WAIT + Duration::from_secs(1)).map_err(|_| "Discord 앱이 응답하지 않습니다".to_string())?
}

fn wanted() -> bool {
    LIVE.lock().unwrap().holders > 0
}

fn work() {
    let mut retry = RETRY_MIN;
    loop {
        {
            let mut s = LIVE.lock().unwrap();
            if s.holders == 0 {
                s.running = false;
                s.up = false;
                s.error.clear();
                return;
            }
        }
        let wait = match open() {
            Ok((ipc, latest)) => {
                retry = RETRY_MIN;
                let (tx, rx) = mpsc::channel();
                {
                    let mut s = LIVE.lock().unwrap();
                    (s.up, s.latest, s.requests) = (true, latest, Some(tx));
                    s.error.clear();
                }
                eprintln!("Discord: 연결됨");
                let why = serve(ipc, &rx);
                let mut s = LIVE.lock().unwrap();
                (s.up, s.requests) = (false, None);
                s.latest.clear();
                if let Some(why) = why {
                    eprintln!("Discord: {why}");
                    s.error = why;
                }
                RETRY_MIN
            }
            Err((why, wait)) => {
                let mut s = LIVE.lock().unwrap();
                if s.error != why {
                    eprintln!("Discord: {why}");
                }
                s.error = why;
                let w = wait.unwrap_or(retry);
                retry = (retry * 2).min(RETRY_MAX);
                w
            }
        };
        let mut slept = Duration::ZERO;
        while slept < wait && wanted() {
            thread::sleep(Duration::from_millis(200));
            slept += Duration::from_millis(200);
        }
    }
}

/// 연결하고 인증한 뒤 구독까지 마친다. 실패하면 (사유, 다음 시도까지 기다릴 시간)을 돌려준다.
fn open() -> Result<(Ipc, BTreeMap<String, Value>), Retry> {
    let places: Vec<_> = ipc::sockets().into_iter().filter(|p| p.exists()).collect();
    if places.is_empty() {
        return Err(("Discord 앱이 실행 중이 아닙니다".into(), None));
    }
    let deck = |e: String| {
        let wait = e.contains("거절").then_some(DENIED_WAIT);
        (e, wait)
    };
    let (client_id, _) = begin().map_err(deck)?;
    let token = borrowed(&vault::ask(VAULT_TOKEN, SERVICE_DISCORD, Value::Null).map_err(deck)?).map_err(|e| (e, None))?;
    let mut ipc = Ipc::connect(&client_id, &places).map_err(|e| (e, None))?;
    ipc.command("AUTHENTICATE", json!({ "access_token": token }), REPLY_WAIT).map_err(|e| (e, None))?;
    let mut latest = BTreeMap::new();
    for (seed, evt) in FEEDS {
        latest.insert(evt.to_string(), ipc.command(seed, json!({}), REPLY_WAIT).map_err(|e| (e, None))?);
        ipc.subscribe(evt, json!({})).map_err(|e| (e, None))?;
    }
    Ok((ipc, latest))
}

/// 연결이 끊어지거나 쓰는 곳이 없어질 때까지 명령과 이벤트를 처리한다. 끊어졌으면 그 사유를 돌려준다.
fn serve(mut ipc: Ipc, requests: &Receiver<(String, Value, Reply)>) -> Option<String> {
    loop {
        if !wanted() {
            return None;
        }
        loop {
            match requests.try_recv() {
                Ok((cmd, args, reply)) => {
                    let _ = reply.send(ipc.command(&cmd, args, REPLY_WAIT));
                    if ipc.lost() {
                        return Some("Discord 앱과의 연결이 끊어졌습니다".into());
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return None,
            }
        }
        match ipc.event(TICK) {
            Ok(Some(ev)) => {
                if let Some(evt) = ev["evt"].as_str().filter(|e| FEEDS.iter().any(|(_, f)| f == e)) {
                    LIVE.lock().unwrap().latest.insert(evt.to_string(), ev["data"].clone());
                }
            }
            Ok(None) => {}
            Err(e) => return Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::*;

    fn frame(op: u32, v: &Value) -> Vec<u8> {
        let body = v.to_string();
        [op.to_le_bytes().as_slice(), (body.len() as u32).to_le_bytes().as_slice(), body.as_bytes()].concat()
    }

    fn read_frame(s: &mut UnixStream) -> Value {
        let mut head = [0u8; 8];
        s.read_exact(&mut head).unwrap();
        let mut body = vec![0u8; u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize];
        s.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    /// 가짜 Discord 앱: 명령에 답하기 전에 변경 이벤트를 먼저 보내고, 그 뒤에도 이벤트를 하나 더 반으로 나눠 보낸다.
    /// 작업 루프는 명령 응답을 돌려주고, 두 이벤트를 모두 최신 값에 반영하고, 쓰는 곳이 없어지면 끝나야 한다.
    #[test]
    fn the_loop_answers_calls_and_keeps_the_latest_event() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            read_frame(&mut s);
            s.write_all(&frame(1, &json!({ "cmd": "DISPATCH", "evt": "READY", "data": {} }))).unwrap();
            let req = read_frame(&mut s);
            assert_eq!((&req["cmd"], &req["args"]), (&json!("SET_VOICE_SETTINGS"), &json!({ "mute": true })));
            s.write_all(&frame(1, &json!({ "cmd": "DISPATCH", "evt": VOICE, "nonce": null, "data": { "mute": true, "deaf": false } }))).unwrap();
            s.write_all(&frame(1, &json!({ "cmd": "SET_VOICE_SETTINGS", "nonce": req["nonce"], "data": { "mute": true, "deaf": false } }))).unwrap();
            let late = frame(1, &json!({ "cmd": "DISPATCH", "evt": VOICE, "nonce": null, "data": { "mute": true, "deaf": true } }));
            s.write_all(&late[..5]).unwrap();
            thread::sleep(Duration::from_millis(120));
            s.write_all(&late[5..]).unwrap();
            thread::sleep(Duration::from_millis(300));
        });
        let ipc = Ipc::connect("42", &[path]).unwrap();
        let (tx, rx) = mpsc::channel();
        {
            let mut s = LIVE.lock().unwrap();
            (s.holders, s.up, s.requests) = (1, true, Some(tx));
            s.latest.insert(VOICE.into(), json!({ "mute": false, "deaf": false }));
        }
        let worker = thread::spawn(move || serve(ipc, &rx));
        assert_eq!(super::super::action("mute").unwrap().read(), Some(0.0));
        assert_eq!(call("SET_VOICE_SETTINGS", json!({ "mute": true })).unwrap()["mute"], true);
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while super::super::action("deaf").unwrap().read() != Some(1.0) && std::time::Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(super::super::action("mute").unwrap().read(), Some(1.0));
        assert_eq!(super::super::action("deaf").unwrap().read(), Some(1.0));
        LIVE.lock().unwrap().holders = 0;
        assert_eq!(worker.join().unwrap(), None);
        server.join().unwrap();
        {
            let mut s = LIVE.lock().unwrap();
            (s.up, s.requests) = (false, None);
            s.latest.clear();
        }
        assert_eq!(super::super::action("mute").unwrap().read(), None);
        assert!(call("SET_VOICE_SETTINGS", json!({})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
