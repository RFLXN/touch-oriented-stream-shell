//! 덱 접속 및 초기 핸드셰이크(attach)와 주소·메타데이터 정리 유틸리티.
//!
//! 덱은 공개키로 식별한다. TCP 연결 후 Noise 키 교환을 수행하며, 등록된 덱이 아니면 아무 정보도 보내지 않고
//! 즉시 연결을 닫는다(호스트 이름, OS, 권한 설정이 미인증 피어에 노출되는 것을 방지).
//!
//! 연결 실패는 AttachError로 구분한다: 연결 불가(Unreachable: 전원 꺼짐, 해당 네트워크 미연결 등 일상적 상황으로
//! 조용히 다음 주소로 넘어감)와 프로토콜 오류(Failed: 버전 불일치 등 사용자가 확인해야 하는 문제).
//!
//! 덱이 전달하는 정보(이름, 주소 목록, WoWLAN MAC)는 여기서 검증·정규화해,
//! 덱이 타인에게 넘어가더라도 호스트가 엉뚱한 곳에 접속하거나 비정상 데이터를 저장하지 못하게 막는다.

use std::io;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use toss_proto::secure::{Conn, Identity};
use toss_proto::{
    DeckMsg, HostMsg, DEAD_AFTER, DEFAULT_PORT, DENIED_BUSY, DENIED_FORGOTTEN, DENIED_NOT_PAIRED, DENIED_REJECTED, DENIED_TIMEOUT, PING_EVERY, PROTO_VERSION, USB_DECK_ADDR,
};

use crate::store::Deck;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// 덱당 저장할 최대 주소 개수 및 주소 문자열 최대 길이
const ADDRS_MAX: usize = 8;
const ADDR_MAX: usize = 255;
/// 덱 이름 최대 길이 (문자 수)
const NAME_MAX: usize = 64;

/// "주소" 또는 "주소:포트"를 SocketAddr로 해석(포트 생략 시 기본 포트 사용)
pub fn resolve(addr: &str) -> io::Result<SocketAddr> {
    let with_port = if addr.parse::<IpAddr>().is_ok() || !addr.contains(':') { format!("{addr}:{DEFAULT_PORT}") } else { addr.to_string() };
    with_port.to_socket_addrs()?.next().ok_or_else(|| io::Error::other("주소를 해석할 수 없습니다"))
}

/// 덱에 시도할 접속 주소 목록을 구성한다(USB 주소 우선, 이후 저장된 주소 순, 중복 제거).
/// 페어링된 덱이 없으면 빈 목록을 반환해 불필요한 연결 시도를 하지 않는다.
pub fn dial_list<'a>(decks: impl IntoIterator<Item = &'a Deck>) -> Vec<String> {
    let mut decks = decks.into_iter().peekable();
    if decks.peek().is_none() {
        return Vec::new();
    }
    let mut addrs = vec![USB_DECK_ADDR.to_string()];
    for a in decks.flat_map(|d| d.addrs.iter()) {
        if !addrs.contains(a) {
            addrs.push(a.clone());
        }
    }
    addrs
}

/// 덱에 저장할 주소 목록을 병합한다: 방금 연결된 주소(worked)를 맨 앞에 두고, 덱이 보고한 주소(reported),
/// 기존 저장 주소(stored) 순으로 중복 없이 최대 ADDRS_MAX개까지 유지한다.
/// 덱이 보고한 목록에서는 순수 IP 주소만 수용하고(호스트명·포트 포함 값 무시),
/// worked와 stored는 호스트 사용자 입력이나 기존 검증을 거친 값이므로 그대로 유지한다.
pub fn merged_addrs(worked: &str, reported: &[String], stored: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let worked = worked.to_string();
    let candidates = std::iter::once(&worked).chain(reported.iter().filter(|a| a.parse::<IpAddr>().is_ok())).chain(stored.iter());
    for a in candidates {
        if !a.is_empty() && a.len() <= ADDR_MAX && !out.contains(a) && out.len() < ADDRS_MAX {
            out.push(a.clone());
        }
    }
    out
}

/// 덱 이름 정규화: 제어 문자를 제거하고 NAME_MAX 길이로 자른다. 비어 있으면 "deck" 반환
fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    let name = name.trim();
    if name.is_empty() { "deck".to_string() } else { name.to_string() }
}

/// WoWLAN MAC 주소 검증: "aa:bb:cc:dd:ee:ff" 형식이 아니면 빈 문자열 반환
fn clean_wake(wake: &str) -> String {
    let ok = wake.len() == 17 && wake.split(':').count() == 6 && wake.split(':').all(|h| h.len() == 2 && u8::from_str_radix(h, 16).is_ok());
    if ok { wake.to_ascii_lowercase() } else { String::new() }
}

pub struct Attached {
    pub conn: Arc<Conn>,
    pub reader: TcpStream,
    /// 정규화된 덱 이름과 IP 주소 목록
    pub name: String,
    pub addrs: Vec<String>,
    /// 덱의 수락 상태 (toss-proto의 STATUS_*)
    pub status: String,
    /// hello에 실어 보낸 활성 권한 목록
    pub caps: Vec<String>,
    /// 절전 깨우기용 Wi-Fi MAC 주소 (없으면 빈 문자열)
    pub wake: String,
}

/// 덱 연결 실패 원인
#[derive(Debug)]
pub enum AttachError {
    /// 연결 불가(전원 꺼짐, 네트워크 단절 등). 일시적/일상적 상황이므로 다음 주소나 다음 주기로 재시도한다.
    Unreachable(io::Error),
    /// 연결은 되었으나 핸드셰이크/프로토콜 단계에서 실패(키 교환 실패, hello 무응답, 버전 불일치 등). 사용자에게 알려야 한다.
    Failed(String),
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachError::Unreachable(e) => write!(f, "{}", why_lost(e)),
            AttachError::Failed(why) => f.write_str(why),
        }
    }
}

/// 덱에 접속해 Noise 키 교환 후 hello 메시지를 주고받는다.
/// 키 교환으로 상대방 공개키를 확인한 뒤 caps_for(상대 키)를 호출해 권한 목록을 조회한다.
/// None이 반환되면(미등록 피어) 아무것도 보내지 않고 연결을 닫은 뒤 Ok(None)을 반환한다.
/// 프로토콜 버전이 다르면(덱이 자신의 버전을 담아 denied hello를 보냄) 버전 불일치 에러를 반환한다.
pub fn attach(addr: &str, me: &Identity, pair: bool, caps_for: impl FnOnce(&str) -> Option<Vec<String>>) -> Result<Option<Attached>, AttachError> {
    use AttachError::{Failed, Unreachable};
    let stream = TcpStream::connect_timeout(&resolve(addr).map_err(Unreachable)?, CONNECT_TIMEOUT).map_err(Unreachable)?;
    stream.set_nodelay(true).map_err(Unreachable)?;
    stream.set_read_timeout(Some(DEAD_AFTER)).map_err(Unreachable)?;
    stream.set_write_timeout(Some(DEAD_AFTER)).map_err(Unreachable)?;
    let (conn, mut reader) = Conn::establish(stream, me, true).map_err(|e| Failed(format!("덱과 키 교환 실패: {}", why_lost(&e))))?;
    let Some(caps) = caps_for(&conn.remote) else {
        conn.shutdown();
        return Ok(None);
    };
    conn.send(&HostMsg::Hello { proto: PROTO_VERSION, name: host_name(), os: std::env::consts::OS.to_string(), caps: caps.clone(), pair })
        .map_err(|e| Failed(format!("덱에 hello 전송 실패: {}", why_lost(&e))))?;
    let hello = conn.recv(&mut reader).map_err(|e| Failed(format!("덱의 hello 응답 수신 실패: {}", why_lost(&e))))?;
    let Some(DeckMsg::Hello { proto, name, status, addrs, wake }) = hello else {
        return Err(Failed("덱이 hello로 응답하지 않았습니다".to_string()));
    };
    if proto != PROTO_VERSION {
        return Err(Failed(format!("프로토콜 버전이 다릅니다 (덱 {proto}, 호스트 {PROTO_VERSION}). 양쪽 버전을 맞춰 주세요")));
    }
    Ok(Some(Attached { conn: Arc::new(conn), reader, name: clean_name(&name), addrs: merged_addrs("", &addrs, &[]), status, caps, wake: clean_wake(&wake) }))
}

/// 덱이 연결을 거부했을 때 이어서 전송되는 상세 사유를 읽는다.
pub fn denied_reason(a: &mut Attached) -> String {
    for _ in 0..8 {
        match a.conn.recv::<DeckMsg>(&mut a.reader) {
            Ok(Some(DeckMsg::Denied { reason })) => return reason,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    String::new()
}

pub fn explain(reason: &str) -> &'static str {
    match reason {
        DENIED_NOT_PAIRED => "덱이 페어링 모드가 아닙니다. 덱 설정 → Paired devices → PAIR NEW DEVICE를 누른 뒤 다시 시도하세요",
        DENIED_BUSY => "덱이 다른 PC와 페어링 중입니다. 잠시 후 다시 시도하세요",
        DENIED_REJECTED => "덱에서 페어링을 거절했습니다",
        DENIED_TIMEOUT => "덱의 페어링 모드가 만료되었습니다. 덱에서 다시 켠 뒤 시도하세요",
        DENIED_FORGOTTEN => "덱에서 이 PC의 페어링이 삭제되었습니다",
        _ => "덱이 연결을 거부했습니다",
    }
}

/// I/O 에러를 사람이 읽기 쉬운 연결 종료 사유로 변환
pub fn why_lost(e: &io::Error) -> String {
    use io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, TimedOut, UnexpectedEof, WouldBlock};
    match e.kind() {
        WouldBlock | TimedOut => "덱이 응답하지 않습니다".to_string(),
        UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe => "덱이 연결을 닫았습니다".to_string(),
        _ => e.to_string(),
    }
}

/// Keep-alive ping 전송 스레드 시작. 반환된 플래그를 false로 설정하면 종료된다.
/// 메인 세션 스레드가 느린 플레이어나 커맨드에 블로킹되더라도 ping은 독립적으로 전송되어 연결이 유지된다.
pub fn start_pings(conn: &Arc<Conn>) -> Arc<AtomicBool> {
    let alive = Arc::new(AtomicBool::new(true));
    let (alive2, conn) = (alive.clone(), conn.clone());
    thread::spawn(move || {
        while alive2.load(Ordering::SeqCst) {
            if conn.send(&HostMsg::Ping).is_err() {
                break;
            }
            thread::sleep(PING_EVERY);
        }
    });
    alive
}

pub fn host_name() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "host".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::Choices;
    use std::net::TcpListener;

    fn deck(key: &str, addrs: &[&str]) -> Deck {
        Deck { key: key.into(), name: "deck".into(), addrs: addrs.iter().map(|a| a.to_string()).collect(), wake: String::new(), perms: Choices::new() }
    }

    /// 페어링된 덱이 없으면 연결을 시도하지 않으며, 있으면 USB 주소를 우선하고 중복 주소는 제거한다.
    #[test]
    fn nothing_is_dialled_before_the_first_pairing() {
        assert!(dial_list(&[] as &[Deck]).is_empty());
        assert_eq!(dial_list(&[deck("a", &[])]), [USB_DECK_ADDR]);
        assert_eq!(dial_list(&[deck("a", &["10.0.0.7", USB_DECK_ADDR]), deck("b", &["10.0.0.8", "10.0.0.7"])]), [USB_DECK_ADDR, "10.0.0.7", "10.0.0.8"]);
    }

    /// 저장 주소 병합: 최근 연결 성공 주소를 맨 앞에 두고, 덱 보고 주소는 IP만 허용하며, 개수·길이 제한을 지킨다.
    #[test]
    fn remembered_addresses_put_the_one_that_worked_first_and_take_only_ip_addresses_from_the_deck() {
        let s = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        let reported = s(&["10.0.0.7", "evil.example", "10.0.0.9:22", "fe80::1", "10.0.0.7", ""]);
        assert_eq!(merged_addrs("192.168.0.5", &reported, &s(&["myhost:7879", "10.0.0.7"])), ["192.168.0.5", "10.0.0.7", "fe80::1", "myhost:7879"]);
        assert_eq!(merged_addrs("10.0.0.7", &reported, &[])[0], "10.0.0.7");
        let many: Vec<String> = (1..=30).map(|n| format!("10.0.0.{n}")).collect();
        let kept = merged_addrs("172.16.42.1", &many, &[]);
        assert_eq!((kept.len(), kept[0].as_str()), (ADDRS_MAX, "172.16.42.1"));
        assert!(merged_addrs(&"x".repeat(300), &[], &[]).is_empty());
    }

    #[test]
    fn what_the_deck_says_about_itself_is_trimmed() {
        assert_eq!(clean_name("  my\u{7}deck\n"), "mydeck");
        assert_eq!(clean_name(&"가".repeat(200)).chars().count(), NAME_MAX);
        assert_eq!(clean_name("\n"), "deck");
        assert_eq!(clean_wake("AA:bb:cc:dd:ee:ff"), "aa:bb:cc:dd:ee:ff");
        for bad in ["", "aa:bb:cc:dd:ee", "aa:bb:cc:dd:ee:zz", "aabbccddeeff", "aa:bb:cc:dd:ee:ff:00"] {
            assert_eq!(clean_wake(bad), "", "{bad}");
        }
    }

    /// 프로토콜 버전 불일치: Unreachable과 구분해 버전 불일치 실패로 보고한다.
    #[test]
    fn a_deck_of_another_protocol_version_is_reported_as_such() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let deck = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let (conn, mut reader) = Conn::establish(stream, &Identity::generate().unwrap(), false).unwrap();
            let _ = conn.recv::<HostMsg>(&mut reader);
            conn.send(&DeckMsg::Hello { proto: PROTO_VERSION + 1, name: "deck".into(), status: toss_proto::STATUS_DENIED.into(), addrs: Vec::new(), wake: String::new() }).unwrap();
        });
        let got = attach(&addr, &Identity::generate().unwrap(), false, |_| Some(Vec::new()));
        deck.join().unwrap();
        let Err(AttachError::Failed(why)) = got else { panic!("버전이 다른데 연결되었거나 Unreachable로 처리됨") };
        assert!(why.contains("프로토콜 버전이 다릅니다") && why.contains(&format!("덱 {}", PROTO_VERSION + 1)), "{why}");

        let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        assert!(matches!(attach(&closed, &Identity::generate().unwrap(), false, |_| Some(Vec::new())), Err(AttachError::Unreachable(_))));
    }

    #[test]
    fn a_silent_or_closed_connection_is_named_in_plain_words() {
        assert_eq!(why_lost(&io::Error::from(io::ErrorKind::WouldBlock)), "덱이 응답하지 않습니다");
        assert_eq!(why_lost(&io::Error::from(io::ErrorKind::TimedOut)), "덱이 응답하지 않습니다");
        assert_eq!(why_lost(&io::Error::from(io::ErrorKind::UnexpectedEof)), "덱이 연결을 닫았습니다");
        assert_eq!(why_lost(&io::Error::other("메시지 크기 초과")), "메시지 크기 초과");
    }
}
