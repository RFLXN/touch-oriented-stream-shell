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
//! Windows에서는 명명된 파이프 `\\.\pipe\discord-ipc-0`~`9`를 쓴다(프레임 형식은 같다).
//! 프레임: opcode(u32 리틀 엔디안) + 길이(u32 리틀 엔디안) + JSON. opcode는 0 HANDSHAKE, 1 FRAME, 2 CLOSE, 3 PING, 4 PONG.
//!
//! 음성 권한(`rpc.voice.*`)은 Discord가 승인한 앱이나 그 앱의 테스터(최대 50명)에게만 열려 있다.
//! 그래서 Discord 개발자 포털에서 직접 앱을 만들고 본인을 테스터로 넣어야 한다(`docs/04-host-link.md` 4.15절).
//! 빌린 토큰은 메모리에만 두고 파일이나 로그에 남기지 않는다.

use std::time::Duration;

use serde_json::{json, Value};
use toss_proto::{KIND_SWITCH, SERVICE_DISCORD, VAULT_BEGIN, VAULT_TOKEN};

use crate::vault;

#[cfg(any(unix, windows))]
mod live;
pub use live::{hold, status, Hold};

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

/// IPC 전송 계층(유닉스 소켓). 읽기는 소켓의 읽기 제한 시간을 쓴다.
#[cfg(unix)]
mod stream {
    use std::io::{self, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    pub struct Stream(UnixStream);

    impl Stream {
        pub fn connect(path: &Path) -> io::Result<Self> {
            UnixStream::connect(path).map(Self)
        }

        /// `wait` 안에 도착한 데이터를 읽는다. 오지 않으면 WouldBlock 또는 TimedOut 오류, 상대가 닫았으면 0
        pub fn read_within(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            self.0.set_read_timeout(Some(wait))?;
            self.0.read(buf)
        }

        pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.write_all(bytes)
        }
    }

    /// Discord 앱이 열어 둘 수 있는 IPC 소켓 후보
    pub fn places() -> Vec<PathBuf> {
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

    /// 지금 열려 있는 IPC 소켓
    pub fn present() -> Vec<PathBuf> {
        places().into_iter().filter(|p| p.exists()).collect()
    }
}

/// IPC 전송 계층(Windows 명명된 파이프). 동기 파이프는 읽기 제한 시간이 없으므로
/// `PeekNamedPipe`로 도착한 바이트가 있는지 확인한 뒤 그만큼만 읽는다(읽기가 막혀 쓰기까지 멈추는 일이 없다).
#[cfg(windows)]
mod stream {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::PeekNamedPipe;

    /// 데이터 도착 여부를 확인하는 간격
    const PEEK_EVERY: Duration = Duration::from_millis(10);
    const PIPE_DIR: &str = r"\\.\pipe\";

    pub struct Stream(File);

    impl Stream {
        pub fn connect(path: &Path) -> io::Result<Self> {
            OpenOptions::new().read(true).write(true).open(path).map(Self)
        }

        /// `wait` 안에 도착한 데이터를 읽는다. 오지 않으면 TimedOut 오류, 파이프가 끊겼으면 0
        pub fn read_within(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize> {
            let until = Instant::now() + wait;
            loop {
                let mut avail = 0u32;
                let handle = HANDLE(self.0.as_raw_handle());
                if unsafe { PeekNamedPipe(handle, None, 0, None, Some(&mut avail), None) }.is_err() {
                    return Ok(0);
                }
                if avail > 0 {
                    let n = buf.len().min(avail as usize);
                    return self.0.read(&mut buf[..n]);
                }
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                std::thread::sleep(left.min(PEEK_EVERY));
            }
        }

        pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            self.0.write_all(bytes)
        }
    }

    /// Discord 앱이 열어 둘 수 있는 IPC 파이프 후보
    pub fn places() -> Vec<PathBuf> {
        (0..10).map(|n| PathBuf::from(format!(r"{PIPE_DIR}discord-ipc-{n}"))).collect()
    }

    /// 지금 열려 있는 IPC 파이프. 파이프에 `exists()`(파일 열기)를 쓰면 Discord에 빈 연결이 하나 생기므로 파이프 목록에서 찾는다
    pub fn present() -> Vec<PathBuf> {
        let open: Vec<String> = std::fs::read_dir(PIPE_DIR)
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_ascii_lowercase()).collect())
            .unwrap_or_default();
        places().into_iter().filter(|p| p.file_name().is_some_and(|n| open.contains(&n.to_string_lossy().to_ascii_lowercase()))).collect()
    }
}

#[cfg(any(unix, windows))]
mod ipc {
    use std::collections::VecDeque;
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};

    use super::stream::Stream;
    pub use super::stream::present;
    use super::{FRAME_MAX, OP_CLOSE, OP_FRAME, OP_HANDSHAKE, OP_PING, OP_PONG, REPLY_WAIT};

    /// Discord 앱과의 IPC 연결 하나. 명령의 응답을 기다리는 동안 도착한 이벤트(`DISPATCH`)는 버리지 않고 쌓아 두었다가 `event`로 돌려준다.
    pub struct Ipc {
        s: Stream,
        buf: Vec<u8>,
        events: VecDeque<Value>,
        lost: bool,
    }

    /// Discord 앱이 열어 둘 수 있는 IPC 소켓(Windows는 파이프) 후보
    pub fn sockets() -> Vec<PathBuf> {
        super::stream::places()
    }

    fn nonce() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        format!("{t:x}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst))
    }

    fn late() -> String {
        "Discord 앱이 제한 시간 안에 응답하지 않았습니다".to_string()
    }

    fn is_event(v: &Value) -> bool {
        v["cmd"] == "DISPATCH" && v["nonce"].is_null()
    }

    impl Ipc {
        /// 소켓 후보를 차례로 시도해 HANDSHAKE하고 READY를 받는다
        pub fn connect(client_id: &str, places: &[PathBuf]) -> Result<Self, String> {
            let mut last = "Discord 앱에 연결할 수 없습니다 (Discord 앱이 이 PC에서 실행 중인지 확인하세요)".to_string();
            for path in places {
                let Ok(s) = Stream::connect(path) else { continue };
                let mut ipc = Ipc { s, buf: Vec::new(), events: VecDeque::new(), lost: false };
                ipc.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id }))?;
                match ipc.recv(REPLY_WAIT) {
                    Ok(Some((OP_FRAME, v))) if v["evt"] == "READY" => return Ok(ipc),
                    Ok(Some((OP_CLOSE, v))) => last = format!("Discord 앱이 연결을 거절했습니다: {}", v["message"].as_str().unwrap_or("사유 없음")),
                    Ok(Some(_)) => last = "Discord 앱이 예상과 다르게 응답했습니다".into(),
                    Ok(None) => last = late(),
                    Err(e) => last = e,
                }
            }
            Err(last)
        }

        /// 연결이 끊어졌는지 여부(Discord가 돌려준 오류 응답은 해당하지 않는다)
        pub fn lost(&self) -> bool {
            self.lost
        }

        fn send(&mut self, op: u32, v: &Value) -> Result<(), String> {
            let body = v.to_string();
            let mut frame = Vec::with_capacity(8 + body.len());
            frame.extend_from_slice(&op.to_le_bytes());
            frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
            frame.extend_from_slice(body.as_bytes());
            self.s.write_all(&frame).map_err(|e| {
                self.lost = true;
                format!("Discord 앱에 보낼 수 없습니다: {e}")
            })
        }

        /// 프레임 하나를 받는다. `wait` 안에 온전한 프레임이 오지 않으면 `None`(받던 조각은 다음 호출에 이어 붙인다)
        fn recv(&mut self, wait: Duration) -> Result<Option<(u32, Value)>, String> {
            let until = Instant::now() + wait;
            loop {
                if self.buf.len() >= 8 {
                    let op = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]);
                    let len = u32::from_le_bytes([self.buf[4], self.buf[5], self.buf[6], self.buf[7]]) as usize;
                    if len > FRAME_MAX {
                        self.lost = true;
                        return Err("Discord 앱이 너무 큰 메시지를 보냈습니다".into());
                    }
                    if self.buf.len() >= 8 + len {
                        let v = serde_json::from_slice(&self.buf[8..8 + len]).unwrap_or(Value::Null);
                        self.buf.drain(..8 + len);
                        return Ok(Some((op, v)));
                    }
                }
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Ok(None);
                }
                let mut chunk = [0u8; 8192];
                match self.s.read_within(&mut chunk, left) {
                    Ok(0) => {
                        self.lost = true;
                        return Err("Discord 앱과의 연결이 끊어졌습니다".into());
                    }
                    Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                    Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(None),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => {
                        self.lost = true;
                        return Err(format!("Discord 앱과의 연결이 끊어졌습니다: {e}"));
                    }
                }
            }
        }

        /// 요청 프레임을 보내고 같은 nonce의 응답을 기다린다. 오류 응답이면 Discord가 알려 준 사유를 돌려준다.
        fn request(&mut self, mut frame: Value, wait: Duration) -> Result<Value, String> {
            let nonce = nonce();
            frame["nonce"] = json!(nonce);
            self.send(OP_FRAME, &frame)?;
            let until = Instant::now() + wait;
            loop {
                match self.recv(until.saturating_duration_since(Instant::now()))? {
                    None => return Err(late()),
                    Some((OP_FRAME, v)) if v["nonce"] == nonce.as_str() => {
                        return if v["evt"] == "ERROR" {
                            Err(format!("Discord: {} (코드 {})", v["data"]["message"].as_str().unwrap_or("알 수 없는 오류"), v["data"]["code"]))
                        } else {
                            Ok(v["data"].clone())
                        };
                    }
                    Some((OP_FRAME, v)) if is_event(&v) => self.events.push_back(v),
                    Some((OP_PING, v)) => self.send(OP_PONG, &v)?,
                    Some((OP_CLOSE, v)) => {
                        self.lost = true;
                        return Err(format!("Discord 앱이 연결을 닫았습니다: {}", v["message"].as_str().unwrap_or("사유 없음")));
                    }
                    _ => {}
                }
            }
        }

        /// 명령을 보내고 응답을 기다린다
        pub fn command(&mut self, cmd: &str, args: Value, wait: Duration) -> Result<Value, String> {
            self.request(json!({ "cmd": cmd, "args": args }), wait)
        }

        /// 이벤트를 구독한다(이후 그 이벤트가 `event`로 들어온다)
        pub fn subscribe(&mut self, evt: &str, args: Value) -> Result<Value, String> {
            self.request(json!({ "cmd": "SUBSCRIBE", "evt": evt, "args": args }), REPLY_WAIT)
        }

        /// 구독한 이벤트를 하나 꺼낸다. `wait` 안에 오지 않으면 `None`
        pub fn event(&mut self, wait: Duration) -> Result<Option<Value>, String> {
            if let Some(v) = self.events.pop_front() {
                return Ok(Some(v));
            }
            match self.recv(wait)? {
                Some((OP_FRAME, v)) if is_event(&v) => Ok(Some(v)),
                Some((OP_PING, v)) => self.send(OP_PONG, &v).map(|()| None),
                Some((OP_CLOSE, v)) => {
                    self.lost = true;
                    Err(format!("Discord 앱이 연결을 닫았습니다: {}", v["message"].as_str().unwrap_or("사유 없음")))
                }
                _ => Ok(None),
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
#[cfg(any(unix, windows))]
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
#[cfg(any(unix, windows))]
pub fn check(step: impl Fn(&'static str)) -> Result<Who, String> {
    step(STEP_DECK);
    let (client_id, _) = begin()?;
    let token = borrowed(&vault::ask(VAULT_TOKEN, SERVICE_DISCORD, Value::Null)?)?;
    step(STEP_DISCORD);
    let mut ipc = ipc::Ipc::connect(&client_id, &ipc::sockets())?;
    who(&ipc.command("AUTHENTICATE", json!({ "access_token": token }), REPLY_WAIT)?)
}

/// 덱 명령으로 쓸 수 있는 Discord 동작. 명령에 `discord`로 지정하면 셸 커맨드 대신 이 동작을 실행하고,
/// 상태는 구독한 이벤트(`feed`)의 최신 값에서 읽는다(상태 확인 커맨드를 주기적으로 실행하지 않는다).
pub struct Action {
    pub id: &'static str,
    pub title: &'static str,
    /// 명령 종류(toss-proto KIND_*). 덱에서 이 종류의 위젯으로 쓴다
    pub kind: &'static str,
    feed: &'static str,
    read: fn(&Value) -> Option<f64>,
    write: fn(f64) -> (&'static str, Value),
}

/// Discord 음성 설정 변경 이벤트
pub const VOICE: &str = "VOICE_SETTINGS_UPDATE";

/// `mute`는 헤드셋 음소거(`deaf`) 중에도 켜짐으로 읽는다. Discord는 헤드셋 음소거 중에 마이크도 막지만 `mute` 값은 그대로 두기 때문이다.
/// 헤드셋 음소거 중에 마이크 음소거를 끄면 Discord가 헤드셋 음소거도 함께 푼다.
pub const ACTIONS: &[Action] = &[
    Action { id: "mute", title: "마이크 음소거", kind: KIND_SWITCH, feed: VOICE, read: |v| Some(f64::from(v["mute"].as_bool()? || v["deaf"].as_bool().unwrap_or(false))), write: |x| ("SET_VOICE_SETTINGS", json!({ "mute": x != 0.0 })) },
    Action { id: "deaf", title: "헤드셋 음소거", kind: KIND_SWITCH, feed: VOICE, read: |v| v["deaf"].as_bool().map(f64::from), write: |x| ("SET_VOICE_SETTINGS", json!({ "deaf": x != 0.0 })) },
];

/// ID로 Discord 동작을 찾는다
pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

impl Action {
    /// 현재 값(Discord에 연결되어 있지 않으면 `None`)
    pub fn read(&self) -> Option<f64> {
        live::latest(self.feed).and_then(|v| (self.read)(&v))
    }

    /// 값을 적용한다(정규화된 값: 스위치는 0 또는 1)
    pub fn write(&self, value: f64) -> Result<(), String> {
        let (cmd, args) = (self.write)(value);
        live::call(cmd, args).map(|_| ())
    }
}

#[cfg(not(any(unix, windows)))]
mod live {
    use serde_json::Value;

    pub struct Hold(());

    pub fn hold() -> Hold {
        Hold(())
    }

    pub fn status() -> (bool, bool, String) {
        (false, false, "이 운영체제의 Discord 연결은 아직 지원하지 않습니다".into())
    }

    pub fn latest(_evt: &str) -> Option<Value> {
        None
    }

    pub fn call(_cmd: &str, _args: Value) -> Result<Value, String> {
        Err("이 운영체제의 Discord 연결은 아직 지원하지 않습니다".into())
    }
}

#[cfg(not(any(unix, windows)))]
pub fn link(_step: impl Fn(&'static str)) -> Result<Who, String> {
    Err("이 운영체제의 Discord 연결은 아직 지원하지 않습니다".into())
}

#[cfg(not(any(unix, windows)))]
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
    fn the_mic_reads_as_muted_while_deafened() {
        let (mute, deaf) = (action("mute").unwrap(), action("deaf").unwrap());
        let read = |a: &Action, v: Value| (a.read)(&v);
        assert_eq!(read(mute, json!({ "mute": false, "deaf": true })), Some(1.0));
        assert_eq!(read(mute, json!({ "mute": true, "deaf": false })), Some(1.0));
        assert_eq!(read(mute, json!({ "mute": false, "deaf": false })), Some(0.0));
        assert_eq!(read(mute, json!({})), None);
        assert_eq!(read(deaf, json!({ "mute": true, "deaf": false })), Some(0.0));
        assert_eq!((mute.write)(0.0), ("SET_VOICE_SETTINGS", json!({ "mute": false })));
    }

    #[test]
    fn the_user_is_named_by_display_name_first() {
        let w = who(&json!({ "user": { "username": "u", "global_name": "Name" }, "scopes": ["rpc"], "expires": "2026-11-26T00:00:00Z" })).unwrap();
        assert_eq!(w, Who { name: "Name".into(), expires: "2026-11-26T00:00:00Z".into(), scopes: vec!["rpc".into()] });
        assert_eq!(who(&json!({ "user": { "username": "u", "global_name": null } })).unwrap().name, "u");
        assert!(who(&json!({})).is_err());
    }
}

/// 이 PC에서 실행 중인 Discord 앱과의 파이프 왕복 확인: `cargo test discord_pipe -- --ignored --nocapture`
/// 존재하지 않는 앱 ID로 HANDSHAKE하므로 Discord가 거절(CLOSE)로 답해야 한다(승인 창은 뜨지 않는다).
#[cfg(all(test, windows))]
mod windows_tests {
    use super::ipc;

    #[test]
    #[ignore]
    fn discord_pipe_round_trip() {
        let places = ipc::present();
        println!("열린 Discord 파이프: {places:?}");
        assert!(!places.is_empty(), "Discord 앱이 실행 중이어야 함");
        let err = ipc::Ipc::connect("0", &places).err().expect("잘못된 앱 ID는 거절되어야 함");
        println!("Discord 응답: {err}");
        assert!(err.contains("거절"), "{err}");
    }
}
