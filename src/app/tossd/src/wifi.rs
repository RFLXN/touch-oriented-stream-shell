//! Wi-Fi 관리: 주변 AP 스캔, 선택한 AP 연결, 저장된 AP 삭제.
//!
//! 실제 동작은 `wpa_supplicant`가 처리하며, 데몬은 제어 소켓(`/run/wpa_supplicant/wlan0`)을 통해 명령을 주고받는다.
//! 소켓 권한이 `tossd` 그룹에 열려 있으므로(`src/os/rootfs/etc/local.d/30-wifi.start`) 데몬이 root 권한 없이도 제어할 수 있다.
//! IP 할당(DHCP)은 `dhcpcd`가 담당한다(데몬은 관여하지 않음).
//! 저장된 AP와 비밀번호는 `wpa_supplicant`가 자체 설정 파일에 기록한다(기기에서는 영구 파티션 `/data/wifi/wpa_supplicant.conf`).
//!
//! 전용 스레드가 1초마다 상태를 조회해 `WifiState`를 생성하고, 변경 시 콜백으로 알린다. UI의 요청(`WifiCmd`)도 이 스레드에서 처리한다.
//!
//! 연결 시도 시 `wpa_supplicant`는 해당 AP만 활성화하고 나머지 저장된 AP를 비활성화하므로, 시도가 끝나면(`finish`) 모두 다시 활성화한 뒤
//! 설정 파일에 저장한다. 시도가 끝나기 전에 다른 AP를 선택하거나 해당 AP를 삭제하면 진행 중이던 시도를 먼저 철회해(`abandon`)
//! 검증되지 않은 비밀번호나 임시 비활성화 상태가 설정 파일에 남지 않도록 한다.
//!
//! 비밀번호는 로그에 남기지 않는다.

use std::collections::HashMap;
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use toss_proto::secure::{from_hex, to_hex};

pub const CTRL_PATH: &str = "/run/wpa_supplicant/wlan0";

const TICK: Duration = Duration::from_secs(1);
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);
/// 스캔 요청 후 "스캔 중" 상태를 유지하는 시간. 2.4GHz/5GHz 양 대역을 스캔하는 데 보통 3~4초 걸린다.
const SCAN_TIME: Duration = Duration::from_secs(5);
/// 연결 시도 후 이 시간 내에 완료되지 않으면 실패로 처리한다.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// 연결된 동안 신호 세기를 다시 조회하는 간격. 매초 읽으면 값이 미세하게 흔들려 불필요한 상태 푸시가 반복된다.
const SIGNAL_INTERVAL: Duration = Duration::from_secs(5);

/// UI로 전달하는 Wi-Fi 상태
#[derive(Serialize, Clone, PartialEq, Default, Debug)]
pub struct WifiState {
    /// `wpa_supplicant` 제어 소켓 연결 가능 여부. `false`면 나머지 필드는 의미가 없다.
    pub available: bool,
    /// `idle`(미연결) / `connecting`(연결 중) / `connected`(연결됨) / `failed`(방금 연결 실패)
    pub phase: &'static str,
    /// 현재 연결되었거나 연결 시도 중인 AP의 SSID와 키(없으면 빈 문자열)
    pub ssid: String,
    pub key: String,
    /// 할당받은 IP 주소(아직 없으면 빈 문자열)
    pub ip: String,
    /// 연결된 AP의 주파수(MHz), 보안 방식(예: WPA2-PSK), 신호 세기(dBm), 링크 속도(Mb/s). 미연결 시 0 / 빈 문자열
    pub freq: u32,
    pub security: String,
    pub signal: i32,
    pub link_speed: u32,
    pub scanning: bool,
    /// 마지막 연결 실패 사유: `""`(없음) / `"failed"`(연결 실패, 주로 비밀번호 불일치) / `"bad_psk"`(비밀번호 길이·형식 오류)
    /// / `"unsupported"`(지원하지 않는 보안 방식)
    pub error: &'static str,
    /// 스캔된 AP 목록. 신호 세기 내림차순이며 동일 SSID는 가장 신호가 강한 항목만 남긴다.
    pub networks: Vec<Network>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct Network {
    /// UI에 표시할 SSID
    pub ssid: String,
    /// AP 식별 키: SSID 원시 바이트의 hex 인코딩. 연결/삭제 요청 시 이 키를 돌려준다
    /// (UTF-8이 아닌 SSID도 안전하게 다루기 위함).
    pub key: String,
    /// 신호 세기(dBm, 0에 가까울수록 강함)
    pub signal: i32,
    /// `open`(비밀번호 없음) / `psk`(비밀번호 방식) / `other`(미지원 방식: Enterprise 인증, WPA3 전용, WEP 등)
    pub kind: &'static str,
    /// 저장된 네트워크 여부
    pub saved: bool,
}

/// UI에서 전달받는 Wi-Fi 명령
pub enum WifiCmd {
    Scan,
    /// `psk`가 `None`이면 저장된 설정(이미 저장된 경우)이나 개방형 AP로 연결한다.
    Connect { key: String, psk: Option<String> },
    Forget { key: String },
}

/// `wpa_supplicant` 제어 소켓. 명령 한 줄을 보내고 응답 하나를 받는다(Unix datagram).
struct WpaCtrl {
    sock: UnixDatagram,
}

impl WpaCtrl {
    fn open(path: &Path) -> io::Result<Self> {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let name = format!("tossd-wpa-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::SeqCst));
        let sock = UnixDatagram::bind_addr(&SocketAddr::from_abstract_name(name.as_bytes())?)?;
        sock.connect(path)?;
        sock.set_read_timeout(Some(REPLY_TIMEOUT))?;
        Ok(Self { sock })
    }

    fn request(&self, cmd: &str) -> io::Result<String> {
        self.sock.send(cmd.as_bytes())?;
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            let n = self.sock.recv(&mut buf)?;
            if buf[..n].first() == Some(&b'<') {
                continue;
            }
            return Ok(String::from_utf8_lossy(&buf[..n]).into_owned());
        }
    }

    /// `"OK"` 응답이 기대되는 명령
    fn ok(&self, cmd: &str) -> io::Result<bool> {
        Ok(self.request(cmd)?.trim() == "OK")
    }
}

/// `wpa_supplicant`의 SSID 이스케이프 표기를 원래 바이트열로 디코딩한다.
/// 출력 불가능한 바이트는 `\xNN`, 그 외에는 `\"`, `\\`, `\e`, `\n`, `\r`, `\t`가 쓰인다.
fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            match b[i] {
                b'e' => out.push(0x1b),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'x' if i + 2 < b.len() => {
                    let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                    match hex {
                        Some(v) => {
                            out.push(v);
                            i += 2;
                        }
                        None => out.push(b'x'),
                    }
                }
                other => out.push(other),
            }
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    out
}

/// `STATUS` 응답 파싱(`키=값` 라인)
fn parse_status(text: &str) -> HashMap<&str, &str> {
    text.lines().filter_map(|l| l.split_once('=')).collect()
}

/// 보안 플래그(예: `"[WPA2-PSK-CCMP][ESS]"`)에서 인증 방식을 분류한다.
fn kind_of(flags: &str) -> &'static str {
    if flags.contains("EAP") || flags.contains("WEP") || flags.contains("OWE") {
        "other"
    } else if flags.contains("PSK") {
        "psk"
    } else if flags.contains("SAE") {
        "other"
    } else {
        "open"
    }
}

struct Seen {
    ssid: Vec<u8>,
    signal: i32,
    kind: &'static str,
}

/// `SCAN_RESULTS` 응답 파싱: 첫 줄은 헤더, 이후 `"bssid \t 주파수 \t 신호 \t 플래그 \t ssid"`
fn parse_scan(text: &str) -> Vec<Seen> {
    let mut best: Vec<Seen> = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let ssid = unescape(f[4]);
        if ssid.is_empty() || ssid.iter().all(|&b| b == 0) {
            continue;
        }
        let seen = Seen { ssid, signal: f[2].parse().unwrap_or(-100), kind: kind_of(f[3]) };
        match best.iter_mut().find(|s| s.ssid == seen.ssid) {
            Some(s) if s.signal >= seen.signal => {}
            Some(s) => *s = seen,
            None => best.push(seen),
        }
    }
    best.sort_by_key(|s| std::cmp::Reverse(s.signal));
    best
}

struct Saved {
    id: String,
    ssid: Vec<u8>,
    flags: String,
}

/// `LIST_NETWORKS` 응답 파싱: 첫 줄은 헤더, 이후 `"id \t ssid \t bssid \t 플래그"`
fn parse_saved(text: &str) -> Vec<Saved> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() >= 2 && f[0].parse::<u32>().is_ok()).then(|| Saved {
                id: f[0].to_string(),
                ssid: unescape(f[1]),
                flags: f.get(3).copied().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// WPA-PSK 비밀번호 유효성 검사: 8~63자, 출력 가능한 ASCII 문자만 허용
fn psk_ok(psk: &str) -> bool {
    (8..=63).contains(&psk.len()) && psk.bytes().all(|b| (32..=126).contains(&b))
}

/// 연결을 요청하고 결과를 대기 중인 상태
struct Pending {
    id: String,
    ssid: Vec<u8>,
    since: Instant,
    /// 이번 시도에 새로 생성한 네트워크 엔트리 여부. true면 실패 시 삭제해 잘못된 비밀번호가 남지 않게 한다.
    fresh: bool,
}

struct Worker {
    path: PathBuf,
    ctrl: Option<WpaCtrl>,
    scan_until: Option<Instant>,
    pending: Option<Pending>,
    error: &'static str,
    /// 마지막 스캔 결과(보안 방식 확인용)
    seen: Vec<Seen>,
    /// 마지막으로 읽은 신호 세기(dBm)와 링크 속도(Mb/s), 측정 시각
    signal: (i32, u32),
    signal_at: Option<Instant>,
}

impl Worker {
    fn ctrl(&mut self) -> io::Result<&WpaCtrl> {
        if self.ctrl.is_none() {
            self.ctrl = Some(WpaCtrl::open(&self.path)?);
        }
        Ok(self.ctrl.as_ref().unwrap())
    }

    fn handle(&mut self, cmd: WifiCmd) -> io::Result<()> {
        match cmd {
            WifiCmd::Scan => {
                self.ctrl()?.request("SCAN")?;
                self.scan_until = Some(Instant::now() + SCAN_TIME);
            }
            WifiCmd::Connect { key, psk } => {
                let Some(ssid) = from_hex(&key) else { return Ok(()) };
                self.error = "";
                self.abandon()?;
                match self.connect(&ssid, psk.as_deref())? {
                    Ok(pending) => {
                        eprintln!("wifi: 연결 시도: {}", String::from_utf8_lossy(&ssid));
                        self.pending = Some(pending);
                    }
                    Err(why) => {
                        eprintln!("wifi: 연결 시작 실패({why}): {}", String::from_utf8_lossy(&ssid));
                        self.error = why;
                    }
                }
            }
            WifiCmd::Forget { key } => {
                let Some(ssid) = from_hex(&key) else { return Ok(()) };
                if self.pending.as_ref().is_some_and(|p| p.ssid == ssid) {
                    self.abandon()?;
                }
                let settled = self.pending.is_none();
                let ctrl = self.ctrl()?;
                for n in parse_saved(&ctrl.request("LIST_NETWORKS")?).iter().filter(|n| n.ssid == ssid) {
                    ctrl.ok(&format!("REMOVE_NETWORK {}", n.id))?;
                }
                if settled {
                    ctrl.ok("SAVE_CONFIG")?;
                }
                eprintln!("wifi: 저장된 네트워크 삭제: {}", String::from_utf8_lossy(&ssid));
                self.error = "";
            }
        }
        Ok(())
    }

    /// 진행 중인 연결 시도를 철회한다(다른 AP 선택 또는 해당 AP 삭제 시): 새로 추가한 엔트리는 삭제하고,
    /// 비활성화됐던 저장 네트워크들을 다시 모두 활성화한다.
    /// 여기서는 설정 파일에 저장하지 않고 후속 작업이 완료될 때 저장한다(다른 AP를 삭제하는 중에 진행 중인 시도가 있다면 그 시도가 끝날 때(`finish`) 저장).
    fn abandon(&mut self) -> io::Result<()> {
        let Some(p) = self.pending.take() else { return Ok(()) };
        let ctrl = self.ctrl()?;
        if p.fresh {
            ctrl.ok(&format!("REMOVE_NETWORK {}", p.id))?;
        }
        ctrl.ok("ENABLE_NETWORK all")?;
        Ok(())
    }

    /// 연결을 시작한다. 바깥 `Result`는 소켓 통신 오류, 안쪽 `Err`는 시작 거부 사유(`WifiState.error` 값)다.
    fn connect(&mut self, ssid: &[u8], psk: Option<&str>) -> io::Result<Result<Pending, &'static str>> {
        let kind = self.seen.iter().find(|s| s.ssid == ssid).map(|s| s.kind);
        let ctrl = self.ctrl()?;
        let saved = parse_saved(&ctrl.request("LIST_NETWORKS")?);
        let existing = saved.iter().find(|n| n.ssid == ssid);

        let (id, fresh) = match (existing, psk) {
            (Some(n), None) => (n.id.clone(), false),
            (existing, psk) => {
                if kind == Some("other") {
                    return Ok(Err("unsupported"));
                }
                if psk.is_some_and(|p| !psk_ok(p)) || (psk.is_none() && kind == Some("psk")) {
                    return Ok(Err("bad_psk"));
                }
                if let Some(n) = existing {
                    ctrl.ok(&format!("REMOVE_NETWORK {}", n.id))?;
                }
                let id = ctrl.request("ADD_NETWORK")?.trim().to_string();
                if id.parse::<u32>().is_err() {
                    return Ok(Err("failed"));
                }
                let mut ok = ctrl.ok(&format!("SET_NETWORK {id} ssid {}", to_hex(ssid)))?;
                ok = ok
                    && match psk {
                        Some(p) => ctrl.ok(&format!("SET_NETWORK {id} psk \"{p}\""))?,
                        None => ctrl.ok(&format!("SET_NETWORK {id} key_mgmt NONE"))?,
                    };
                if !ok {
                    ctrl.ok(&format!("REMOVE_NETWORK {id}"))?;
                    return Ok(Err("failed"));
                }
                (id, true)
            }
        };
        if !ctrl.ok(&format!("SELECT_NETWORK {id}"))? {
            if fresh {
                ctrl.ok(&format!("REMOVE_NETWORK {id}"))?;
            }
            return Ok(Err("failed"));
        }
        Ok(Ok(Pending { id, ssid: ssid.to_vec(), since: Instant::now(), fresh }))
    }

    /// 연결 시도를 마무리한다: 실패했으면 새로 추가한 엔트리를 삭제하고, 모든 저장 네트워크를 다시 활성화한 뒤 설정 파일에 저장한다.
    fn finish(&mut self, connected: bool) -> io::Result<()> {
        let Some(p) = self.pending.take() else { return Ok(()) };
        let name = String::from_utf8_lossy(&p.ssid).into_owned();
        let ctrl = self.ctrl()?;
        if !connected && p.fresh {
            ctrl.ok(&format!("REMOVE_NETWORK {}", p.id))?;
        }
        ctrl.ok("ENABLE_NETWORK all")?;
        if !ctrl.ok("SAVE_CONFIG")? {
            eprintln!("wifi: 설정 저장 실패(update_config=1 누락 가능성)");
        }
        if connected {
            eprintln!("wifi: 연결됨: {name}");
        } else {
            eprintln!("wifi: 연결 실패: {name}");
            self.error = "failed";
        }
        Ok(())
    }

    /// 현재 상태를 조회해 UI 전달용 `WifiState`를 구성한다.
    fn refresh(&mut self) -> io::Result<WifiState> {
        let ctrl = self.ctrl()?;
        let status_text = ctrl.request("STATUS")?;
        let saved = parse_saved(&ctrl.request("LIST_NETWORKS")?);
        let seen = parse_scan(&ctrl.request("SCAN_RESULTS")?);

        let status = parse_status(&status_text);
        let wpa_state = status.get("wpa_state").copied().unwrap_or("");
        let cur_ssid = status.get("ssid").map(|s| unescape(s)).unwrap_or_default();
        let completed = wpa_state == "COMPLETED";

        let outcome = self.pending.as_ref().and_then(|p| {
            let disabled = saved.iter().any(|n| n.id == p.id && n.flags.contains("TEMP-DISABLED"));
            if completed && cur_ssid == p.ssid {
                Some(true)
            } else if disabled || p.since.elapsed() > CONNECT_TIMEOUT {
                Some(false)
            } else {
                None
            }
        });
        if let Some(connected) = outcome {
            self.finish(connected)?;
        }
        if self.scan_until.is_some_and(|t| Instant::now() >= t) {
            self.scan_until = None;
        }
        if !completed {
            self.signal = (0, 0);
            self.signal_at = None;
        } else if self.signal_at.is_none_or(|t| t.elapsed() >= SIGNAL_INTERVAL) {
            let poll_text = self.ctrl()?.request("SIGNAL_POLL")?;
            let poll = parse_status(&poll_text);
            let num = |key: &str| poll.get(key).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            self.signal = (num("RSSI"), num("LINKSPEED").max(0) as u32);
            self.signal_at = Some(Instant::now());
        }

        let associating = matches!(wpa_state, "AUTHENTICATING" | "ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE" | "GROUP_HANDSHAKE");
        let (phase, ssid) = if let Some(p) = &self.pending {
            ("connecting", p.ssid.clone())
        } else if completed {
            ("connected", cur_ssid)
        } else if !self.error.is_empty() {
            ("failed", Vec::new())
        } else if associating {
            ("connecting", cur_ssid)
        } else {
            ("idle", Vec::new())
        };

        let networks = seen
            .iter()
            .map(|s| Network {
                ssid: String::from_utf8_lossy(&s.ssid).into_owned(),
                key: to_hex(&s.ssid),
                signal: s.signal,
                kind: s.kind,
                saved: saved.iter().any(|n| n.ssid == s.ssid),
            })
            .collect();
        self.seen = seen;

        Ok(WifiState {
            available: true,
            phase,
            key: to_hex(&ssid),
            ssid: String::from_utf8_lossy(&ssid).into_owned(),
            ip: if phase == "connected" { status.get("ip_address").copied().unwrap_or("").to_string() } else { String::new() },
            freq: if phase == "connected" { status.get("freq").and_then(|f| f.parse().ok()).unwrap_or(0) } else { 0 },
            security: if phase == "connected" { status.get("key_mgmt").copied().unwrap_or("").to_string() } else { String::new() },
            signal: if phase == "connected" { self.signal.0 } else { 0 },
            link_speed: if phase == "connected" { self.signal.1 } else { 0 },
            scanning: self.scan_until.is_some(),
            error: self.error,
            networks,
        })
    }

    fn run(mut self, rx: Receiver<WifiCmd>, publish: impl Fn(WifiState)) {
        let mut last: Option<WifiState> = None;
        loop {
            let cmd = match rx.recv_timeout(TICK) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let state = cmd.map_or(Ok(()), |c| self.handle(c)).and_then(|()| self.refresh()).unwrap_or_else(|e| {
                if self.ctrl.take().is_some() || last.is_none() {
                    eprintln!("wifi: wpa_supplicant에 연결할 수 없습니다: {e}");
                }
                self.pending = None;
                self.scan_until = None;
                WifiState { phase: "idle", ..WifiState::default() }
            });
            if last.as_ref() != Some(&state) {
                if last.as_ref().map(|l| l.available) != Some(state.available) && state.available {
                    eprintln!("wifi: wpa_supplicant에 연결됨");
                }
                publish(state.clone());
                last = Some(state);
            }
        }
    }
}

/// Wi-Fi 워커 스레드를 시작한다. 상태 변경 시 `publish`를 호출하고, 반환된 `Sender`로 명령을 전달한다.
pub fn spawn(ctrl_path: PathBuf, publish: impl Fn(WifiState) + Send + 'static) -> Sender<WifiCmd> {
    let (tx, rx) = mpsc::channel();
    let worker = Worker {
        path: ctrl_path,
        ctrl: None,
        scan_until: None,
        pending: None,
        error: "",
        seen: Vec::new(),
        signal: (0, 0),
        signal_at: None,
    };
    thread::spawn(move || worker.run(rx, publish));
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn names_are_decoded() {
        assert_eq!(unescape("plain"), b"plain");
        assert_eq!(unescape(r#"a\"b\\c"#), br#"a"b\c"#);
        assert_eq!(unescape(r"\xec\xa7\x91"), "집".as_bytes());
        assert_eq!(unescape(r"tab\tend"), b"tab\tend");
        assert_eq!(from_hex(&to_hex("집 5G".as_bytes())).unwrap(), "집 5G".as_bytes());
        assert_eq!(from_hex("zz"), None);
        assert_eq!(from_hex("abc"), None);
    }

    #[test]
    fn scan_results_are_read() {
        let text = "bssid / frequency / signal level / flags / ssid\n\
            aa:aa:aa:aa:aa:01\t2412\t-70\t[WPA2-PSK-CCMP][ESS]\thome\n\
            aa:aa:aa:aa:aa:02\t5745\t-40\t[WPA2-PSK-CCMP][ESS]\thome\n\
            aa:aa:aa:aa:aa:03\t2437\t-55\t[ESS]\tcafe\n\
            aa:aa:aa:aa:aa:04\t2437\t-30\t[WPA2-EAP-CCMP][ESS]\toffice\n\
            aa:aa:aa:aa:aa:05\t2437\t-20\t[WPA2-PSK-CCMP][ESS]\t\n\
            aa:aa:aa:aa:aa:06\t2437\t-60\t[WPA2-SAE-CCMP][ESS]\twpa3\n";
        let seen = parse_scan(text);
        let got: Vec<(&[u8], i32, &str)> = seen.iter().map(|s| (s.ssid.as_slice(), s.signal, s.kind)).collect();
        assert_eq!(
            got,
            vec![
                (&b"office"[..], -30, "other"),
                (&b"home"[..], -40, "psk"),
                (&b"cafe"[..], -55, "open"),
                (&b"wpa3"[..], -60, "other"),
            ]
        );
    }

    #[test]
    fn passwords_are_checked() {
        assert!(psk_ok("12345678"));
        assert!(psk_ok(r#"with "quotes" and spaces"#));
        assert!(!psk_ok("1234567"));
        assert!(!psk_ok(&"x".repeat(64)));
        assert!(!psk_ok("줄바꿈\n들어감"));
    }

    /// 모의 `wpa_supplicant`: 제어 소켓으로 들어온 명령을 기록하고 미리 정의된 응답을 반환한다.
    struct Fake {
        log: Arc<Mutex<Vec<String>>>,
        /// 연결 성공으로 응답할지 여부
        completed: Arc<Mutex<bool>>,
        path: PathBuf,
    }

    fn fake() -> Fake {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!("tossd-fake-wpa-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_file(&path);
        let sock = UnixDatagram::bind(&path).unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let completed = Arc::new(Mutex::new(false));
        let (log2, completed2) = (log.clone(), completed.clone());
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut saved: Vec<(u32, String)> = Vec::new();
            let mut next_id = 0;
            while let Ok((n, from)) = sock.recv_from(&mut buf) {
                let cmd = String::from_utf8_lossy(&buf[..n]).into_owned();
                let reply = match cmd.split(' ').collect::<Vec<_>>().as_slice() {
                    ["STATUS"] if *completed2.lock().unwrap() => {
                        "wpa_state=COMPLETED\nssid=home\nip_address=10.0.0.7\nfreq=5745\nkey_mgmt=WPA2-PSK\n".to_string()
                    }
                    ["SIGNAL_POLL"] => "RSSI=-48\nLINKSPEED=433\nNOISE=9999\nFREQUENCY=5745\n".to_string(),
                    ["STATUS"] => "wpa_state=SCANNING\n".to_string(),
                    ["SCAN_RESULTS"] => "bssid / frequency / signal level / flags / ssid\n\
                        aa:aa:aa:aa:aa:01\t2412\t-50\t[WPA2-PSK-CCMP][ESS]\thome\n\
                        aa:aa:aa:aa:aa:02\t2412\t-60\t[ESS]\tcafe\n"
                        .to_string(),
                    ["LIST_NETWORKS"] => {
                        let mut t = "network id / ssid / bssid / flags\n".to_string();
                        for (id, ssid) in &saved {
                            t += &format!("{id}\t{ssid}\tany\t\n");
                        }
                        t
                    }
                    ["ADD_NETWORK"] => {
                        saved.push((next_id, String::new()));
                        next_id += 1;
                        format!("{}\n", next_id - 1)
                    }
                    ["SET_NETWORK", id, "ssid", hex] => {
                        let id: u32 = id.parse().unwrap();
                        saved.iter_mut().find(|s| s.0 == id).unwrap().1 = String::from_utf8(from_hex(hex).unwrap()).unwrap();
                        "OK\n".to_string()
                    }
                    ["REMOVE_NETWORK", id] => {
                        let id: u32 = id.parse().unwrap();
                        saved.retain(|s| s.0 != id);
                        "OK\n".to_string()
                    }
                    _ => "OK\n".to_string(),
                };
                log2.lock().unwrap().push(cmd);
                let _ = sock.send_to_addr(reply.as_bytes(), &from);
            }
        });
        Fake { log, completed, path }
    }

    /// 조건을 만족하는 상태가 수신될 때까지 대기한다.
    fn wait(rx: &Receiver<WifiState>, what: &str, pred: impl Fn(&WifiState) -> bool) -> WifiState {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("기대하던 상태가 오지 않음: {what}"));
            match rx.recv_timeout(left) {
                Ok(s) if pred(&s) => return s,
                Ok(_) => {}
                Err(_) => panic!("기대하던 상태가 오지 않음: {what}"),
            }
        }
    }

    /// 스캔 결과가 전달되고 비밀번호 연결 시 `wpa_supplicant`에 올바른 명령이 전달되며, 연결 완료 후 저장되는지 검증한다.
    #[test]
    fn connect_flow() {
        let fake = fake();
        let (tx_state, rx_state) = mpsc::channel();
        let tx = spawn(fake.path.clone(), move |s| {
            let _ = tx_state.send(s);
        });

        let s = wait(&rx_state, "검색 결과", |s| s.available && s.networks.len() == 2);
        assert_eq!(s.phase, "idle");
        assert_eq!((s.networks[0].ssid.as_str(), s.networks[0].kind, s.networks[0].saved), ("home", "psk", false));
        assert_eq!((s.networks[1].ssid.as_str(), s.networks[1].kind), ("cafe", "open"));
        let home = s.networks[0].key.clone();

        tx.send(WifiCmd::Connect { key: home.clone(), psk: Some("short".into()) }).unwrap();
        let s = wait(&rx_state, "거절", |s| s.phase == "failed");
        assert_eq!(s.error, "bad_psk");
        assert!(!fake.log.lock().unwrap().iter().any(|c| c.starts_with("ADD_NETWORK")));

        tx.send(WifiCmd::Connect { key: home.clone(), psk: Some("pass word\"1".into()) }).unwrap();
        let s = wait(&rx_state, "접속 중", |s| s.phase == "connecting");
        assert_eq!(s.ssid, "home");
        {
            let log = fake.log.lock().unwrap();
            assert!(log.contains(&format!("SET_NETWORK 0 ssid {home}")));
            assert!(log.contains(&"SET_NETWORK 0 psk \"pass word\"1\"".to_string()));
            assert!(log.contains(&"SELECT_NETWORK 0".to_string()));
            assert!(!log.contains(&"SAVE_CONFIG".to_string()));
        }

        *fake.completed.lock().unwrap() = true;
        let s = wait(&rx_state, "접속됨", |s| s.phase == "connected");
        assert_eq!((s.ssid.as_str(), s.ip.as_str(), s.error), ("home", "10.0.0.7", ""));
        assert_eq!((s.freq, s.security.as_str(), s.signal, s.link_speed), (5745, "WPA2-PSK", -48, 433));
        assert!(s.networks[0].saved);
        {
            let log = fake.log.lock().unwrap();
            assert!(log.contains(&"ENABLE_NETWORK all".to_string()));
            assert!(log.contains(&"SAVE_CONFIG".to_string()));
        }

        tx.send(WifiCmd::Forget { key: home }).unwrap();
        wait(&rx_state, "지워짐", |s| !s.networks[0].saved);
        assert!(fake.log.lock().unwrap().contains(&"REMOVE_NETWORK 0".to_string()));
        let _ = std::fs::remove_file(&fake.path);
    }

    /// 연결 시도 중에 다른 AP를 선택하거나 해당 AP를 삭제하면 검증되지 않은 연결 정보를 삭제하고 기존 AP를 다시 활성화한 뒤에 설정을 저장한다.
    #[test]
    fn an_unfinished_attempt_is_undone_before_anything_is_saved() {
        let fake = fake();
        let (tx_state, rx_state) = mpsc::channel();
        let tx = spawn(fake.path.clone(), move |s| {
            let _ = tx_state.send(s);
        });
        let s = wait(&rx_state, "스캔 결과", |s| s.available && s.networks.len() == 2);
        let (home, cafe) = (s.networks[0].key.clone(), s.networks[1].key.clone());
        let at = |cmd: &str, from: usize| fake.log.lock().unwrap().iter().skip(from).position(|c| c == cmd).map(|i| i + from);

        tx.send(WifiCmd::Connect { key: home, psk: Some("not the password".into()) }).unwrap();
        wait(&rx_state, "home 연결 중", |s| s.phase == "connecting" && s.ssid == "home");
        tx.send(WifiCmd::Connect { key: cafe.clone(), psk: None }).unwrap();
        wait(&rx_state, "cafe 연결 중", |s| s.phase == "connecting" && s.ssid == "cafe");
        let first = at("ADD_NETWORK", 0).unwrap();
        let undone = at("REMOVE_NETWORK 0", first).expect("실패한 비밀번호가 남아 있음");
        let enabled = at("ENABLE_NETWORK all", undone).expect("비활성화된 AP를 다시 켜지 않음");
        let second = at("ADD_NETWORK", first + 1).unwrap();
        assert!(undone < enabled && enabled < second);
        assert_eq!(at("SAVE_CONFIG", 0), None);

        tx.send(WifiCmd::Forget { key: cafe }).unwrap();
        wait(&rx_state, "삭제됨", |s| s.phase == "idle" && s.networks.iter().all(|n| !n.saved));
        let undone = at("REMOVE_NETWORK 1", second).unwrap();
        let enabled = at("ENABLE_NETWORK all", undone).expect("비활성화된 AP를 켜지 않고 설정을 저장함");
        assert!(enabled < at("SAVE_CONFIG", 0).unwrap());
        let _ = std::fs::remove_file(&fake.path);
    }

    /// `wpa_supplicant`가 없으면 미사용 상태(`available: false`)로 보고한다.
    #[test]
    fn unavailable_without_supplicant() {
        let (tx_state, rx_state) = mpsc::channel();
        let _tx = spawn(PathBuf::from("/nonexistent/wpa/wlan0"), move |s| {
            let _ = tx_state.send(s);
        });
        let s = wait(&rx_state, "사용 불가", |_| true);
        assert!(!s.available);
        assert!(s.networks.is_empty());
    }
}
