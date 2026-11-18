//! Discord 연동: 이 PC에서 실행 중인 Discord 앱의 로컬 RPC(IPC 소켓)에 접속하고, 덱 금고에서 빌린 토큰으로 인증한다.
//!
//! 연결(승인) 절차 (`link`)
//!   1. 덱에서 공개 값(client_id, 권한 범위)을 받는다(`vault begin`). 이 PC가 처음이면 덱 화면이 허용 여부를 묻는다.
//!   2. Discord 앱에 `AUTHORIZE`를 보내면 앱이 승인 창을 띄운다. 사용자가 승인하면 승인 코드가 돌아온다(브라우저는 쓰지 않는다).
//!   3. 승인 코드를 덱에 넘기면 덱이 토큰을 발급받아 빌려준다(`vault token`의 code). 덱은 refresh 토큰을 보관해 이후 직접 갱신한다.
//!   4. 빌린 토큰으로 `AUTHENTICATE`해 확인한다.
//!
//! 확인 (`check`): 덱에서 토큰을 빌려 `AUTHENTICATE`만 한다(승인 창 없음). 토큰은 Discord 계정과 앱에 묶이므로
//! 같은 계정이면 PC가 바뀌어도 다시 승인할 필요가 없다.
//!
//! IPC 소켓: `$XDG_RUNTIME_DIR`(없으면 `$TMPDIR`, `$TMP`, `$TEMP`, `/tmp`)의 `discord-ipc-0`~`9`.
//! Flatpak(`app/com.discordapp.Discord/`)과 Snap(`snap.discord/`)으로 설치한 Discord의 위치도 찾는다.
//! 프레임: opcode(u32 리틀 엔디안) + 길이(u32 리틀 엔디안) + JSON. opcode는 0 HANDSHAKE, 1 FRAME, 2 CLOSE, 3 PING, 4 PONG.
//!
//! 음성 권한(`rpc.voice.*`)은 Discord가 승인한 앱이나 그 앱의 테스터(최대 50명)에게만 열려 있다.
//! 그래서 Discord 개발자 포털에서 직접 앱을 만들고 본인을 테스터로 넣어야 한다(`docs/04-host-link.md` 4.15절).
//! 빌린 토큰은 메모리에만 두고 파일이나 로그에 남기지 않는다.

use std::time::Duration;

use serde_json::{json, Value};
use toss_proto::{SERVICE_DISCORD, VAULT_BEGIN, VAULT_TOKEN};

use crate::vault;

/// Discord 앱의 승인 창에 사용자가 답하기를 기다리는 시간
const AUTHORIZE_WAIT: Duration = Duration::from_secs(300);
/// 그 밖의 IPC 응답을 기다리는 시간
const REPLY_WAIT: Duration = Duration::from_secs(10);
/// IPC 프레임 최대 크기
const FRAME_MAX: usize = 1024 * 1024;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

/// 인증된 Discord 사용자
#[derive(Clone, Debug, PartialEq)]
pub struct Who {
    pub name: String,
    /// 토큰 만료 시각(Discord가 알려 준 ISO 8601 문자열)
    pub expires: String,
    pub scopes: Vec<String>,
}

/// 진행 단계(웹 콘솔·CLI 표시용)
pub const STEP_DECK: &str = "deck";
pub const STEP_DISCORD: &str = "discord";

#[cfg(unix)]
mod ipc {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use serde_json::{json, Value};

    use super::{FRAME_MAX, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, OP_PING, OP_PONG, REPLY_WAIT};

    pub struct Ipc {
        s: UnixStream,
    }

    /// Discord 앱이 열어 둔 IPC 소켓 후보
    pub fn sockets() -> Vec<PathBuf> {
        let mut bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
        bases.push("/tmp".into());
        let mut out = Vec::new();
        for base in bases {
            for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
                for n in 0..10 {
                    out.push(base.join(sub).join(format!("discord-ipc-{n}")));
                }
            }
        }
        out
    }

    fn nonce() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        format!("{t:x}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst))
    }

    impl Ipc {
        /// 소켓 후보를 차례로 시도해 HANDSHAKE하고 READY를 받는다
        pub fn connect(client_id: &str, places: &[PathBuf]) -> Result<Self, String> {
            let mut last = "Discord 앱에 연결할 수 없습니다 (Discord 앱이 이 PC에서 실행 중인지 확인하세요)".to_string();
            for path in places {
                let Ok(s) = UnixStream::connect(path) else { continue };
                let mut ipc = Ipc { s };
                ipc.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id }))?;
                match ipc.recv(REPLY_WAIT) {
                    Ok((OP_FRAME, v)) if v["evt"] == "READY" => return Ok(ipc),
                    Ok((OP_CLOSE, v)) => last = format!("Discord 앱이 연결을 거절했습니다: {}", v["message"].as_str().unwrap_or("사유 없음")),
                    Ok(_) => last = "Discord 앱이 예상과 다르게 응답했습니다".into(),
                    Err(e) => last = e,
                }
            }
            Err(last)
        }

        fn send(&mut self, op: u32, v: &Value) -> Result<(), String> {
            let body = v.to_string();
            let mut frame = Vec::with_capacity(8 + body.len());
            frame.extend_from_slice(&op.to_le_bytes());
            frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
            frame.extend_from_slice(body.as_bytes());
            self.s.write_all(&frame).map_err(|e| format!("Discord 앱에 보낼 수 없습니다: {e}"))
        }

        fn recv(&mut self, wait: Duration) -> Result<(u32, Value), String> {
            self.s.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
            let lost = |e: std::io::Error| match e.kind() {
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => "Discord 앱이 제한 시간 안에 응답하지 않았습니다".to_string(),
                _ => format!("Discord 앱과의 연결이 끊어졌습니다: {e}"),
            };
            let mut head = [0u8; 8];
            self.s.read_exact(&mut head).map_err(lost)?;
            let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
            let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
            if len > FRAME_MAX {
                return Err("Discord 앱이 너무 큰 메시지를 보냈습니다".into());
            }
            let mut body = vec![0u8; len];
            self.s.read_exact(&mut body).map_err(lost)?;
            Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
        }

        /// 명령을 보내고 같은 nonce의 응답을 기다린다. 오류 응답이면 Discord가 알려 준 사유를 돌려준다.
        pub fn command(&mut self, cmd: &str, args: Value, wait: Duration) -> Result<Value, String> {
            let nonce = nonce();
            self.send(OP_FRAME, &json!({ "cmd": cmd, "args": args, "nonce": nonce }))?;
            loop {
                match self.recv(wait)? {
                    (OP_FRAME, v) if v["nonce"] == nonce.as_str() => {
                        return if v["evt"] == "ERROR" {
                            Err(format!("Discord: {} (코드 {})", v["data"]["message"].as_str().unwrap_or("알 수 없는 오류"), v["data"]["code"]))
                        } else {
                            Ok(v["data"].clone())
                        };
                    }
                    (OP_PING, v) => self.send(OP_PONG, &v)?,
                    (OP_CLOSE, v) => return Err(format!("Discord 앱이 연결을 닫았습니다: {}", v["message"].as_str().unwrap_or("사유 없음"))),
                    _ => {}
                }
            }
        }
    }
}

fn who(data: &Value) -> Result<Who, String> {
    let user = &data["user"];
    let name = user["global_name"].as_str().filter(|n| !n.is_empty()).or_else(|| user["username"].as_str()).ok_or("Discord 인증 응답에 사용자 정보가 없습니다")?;
    let scopes = data["scopes"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Ok(Who { name: name.to_string(), expires: data["expires"].as_str().unwrap_or_default().to_string(), scopes })
}

/// 덱에서 받은 공개 값(client_id, 권한 범위)
fn begin() -> Result<(String, Vec<String>), String> {
    let v = vault::ask(VAULT_BEGIN, SERVICE_DISCORD, Value::Null)?;
    let id = v["client_id"].as_str().ok_or("덱이 client_id를 알려 주지 않았습니다")?.to_string();
    let scopes = v["scopes"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
    Ok((id, scopes))
}

fn borrowed(v: &Value) -> Result<String, String> {
    v["access_token"].as_str().filter(|t| !t.is_empty()).map(str::to_string).ok_or_else(|| "덱이 토큰을 주지 않았습니다".to_string())
}

/// Discord 앱에서 승인을 받아 덱에 토큰을 발급받게 하고, 그 토큰으로 인증해 확인한다. `step`은 진행 단계가 바뀔 때마다 호출된다.
#[cfg(unix)]
pub fn link(step: impl Fn(&'static str)) -> Result<Who, String> {
    step(STEP_DECK);
    let (client_id, scopes) = begin()?;
    step(STEP_DISCORD);
    let mut ipc = ipc::Ipc::connect(&client_id, &ipc::sockets())?;
    let data = ipc.command("AUTHORIZE", json!({ "client_id": client_id, "scopes": scopes }), AUTHORIZE_WAIT)?;
    let code = data["code"].as_str().filter(|c| !c.is_empty()).ok_or("Discord 앱이 승인 코드를 주지 않았습니다")?.to_string();
    step(STEP_DECK);
    let token = borrowed(&vault::ask(VAULT_TOKEN, SERVICE_DISCORD, json!({ "code": code }))?)?;
    who(&ipc.command("AUTHENTICATE", json!({ "access_token": token }), REPLY_WAIT)?)
}

/// 덱에서 토큰을 빌려 Discord 앱에 인증해 본다(승인 창 없음)
#[cfg(unix)]
pub fn check(step: impl Fn(&'static str)) -> Result<Who, String> {
    step(STEP_DECK);
    let (client_id, _) = begin()?;
    let token = borrowed(&vault::ask(VAULT_TOKEN, SERVICE_DISCORD, Value::Null)?)?;
    step(STEP_DISCORD);
    let mut ipc = ipc::Ipc::connect(&client_id, &ipc::sockets())?;
    who(&ipc.command("AUTHENTICATE", json!({ "access_token": token }), REPLY_WAIT)?)
}

#[cfg(not(unix))]
pub fn link(_step: impl Fn(&'static str)) -> Result<Who, String> {
    Err("이 운영체제의 Discord 연결은 아직 지원하지 않습니다".into())
}

#[cfg(not(unix))]
pub fn check(_step: impl Fn(&'static str)) -> Result<Who, String> {
    Err("이 운영체제의 Discord 연결은 아직 지원하지 않습니다".into())
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;

    use super::*;

    fn frame(op: u32, v: &Value) -> Vec<u8> {
        let body = v.to_string();
        [op.to_le_bytes().as_slice(), (body.len() as u32).to_le_bytes().as_slice(), body.as_bytes()].concat()
    }

    fn read_frame(s: &mut std::os::unix::net::UnixStream) -> (u32, Value) {
        let mut head = [0u8; 8];
        s.read_exact(&mut head).unwrap();
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).unwrap();
        (u32::from_le_bytes([head[0], head[1], head[2], head[3]]), serde_json::from_slice(&body).unwrap())
    }

    /// 가짜 Discord 앱: HANDSHAKE에 READY로 답하고, 명령마다 ping 한 번과 다른 이벤트를 먼저 보낸 뒤 정해 둔 응답을 돌려준다
    #[test]
    fn the_client_shakes_hands_skips_noise_and_reads_errors() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-ipc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("discord-ipc-3");
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let (op, hello) = read_frame(&mut s);
            assert_eq!((op, &hello["client_id"]), (OP_HANDSHAKE, &json!("42")));
            s.write_all(&frame(OP_FRAME, &json!({ "cmd": "DISPATCH", "evt": "READY", "data": {} }))).unwrap();
            for reply in [json!({ "data": { "code": "C0" } }), json!({ "evt": "ERROR", "data": { "code": 4002, "message": "Invalid access token" } })] {
                let (_, req) = read_frame(&mut s);
                s.write_all(&frame(OP_PING, &json!({ "x": 1 }))).unwrap();
                assert_eq!(read_frame(&mut s), (OP_PONG, json!({ "x": 1 })));
                s.write_all(&frame(OP_FRAME, &json!({ "cmd": "DISPATCH", "evt": "VOICE_SETTINGS_UPDATE", "nonce": null }))).unwrap();
                let mut reply = reply;
                reply["nonce"] = req["nonce"].clone();
                reply["cmd"] = req["cmd"].clone();
                s.write_all(&frame(OP_FRAME, &reply)).unwrap();
            }
        });
        let places = [dir.join("discord-ipc-0"), path];
        let mut ipc = ipc::Ipc::connect("42", &places).unwrap();
        assert_eq!(ipc.command("AUTHORIZE", json!({ "client_id": "42" }), REPLY_WAIT).unwrap()["code"], "C0");
        let err = ipc.command("AUTHENTICATE", json!({ "access_token": "x" }), REPLY_WAIT).unwrap_err();
        assert!(err.contains("Invalid access token") && err.contains("4002"), "{err}");
        server.join().unwrap();
        assert!(ipc::Ipc::connect("42", &[dir.join("discord-ipc-9")]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_user_is_named_by_display_name_first() {
        let w = who(&json!({ "user": { "username": "u", "global_name": "Name" }, "scopes": ["rpc"], "expires": "2026-11-26T00:00:00Z" })).unwrap();
        assert_eq!(w, Who { name: "Name".into(), expires: "2026-11-26T00:00:00Z".into(), scopes: vec!["rpc".into()] });
        assert_eq!(who(&json!({ "user": { "username": "u", "global_name": null } })).unwrap().name, "u");
        assert!(who(&json!({})).is_err());
    }
}
