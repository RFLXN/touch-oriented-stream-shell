//! 기기 신원(키 쌍)과 암호화 전송 계층.
//!
//! Noise XX 패턴을 사용해 접속 시 양쪽이 공개키를 암호화된 상태로 교환하고, 이후 모든 트래픽을 암호화한다.
//! 상대방 인증(페어링 여부 확인)은 이 계층이 담당하지 않고, 연결 수립 후 상대 공개키(remote)만 호출자에게 넘긴다.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::Serialize;
use snow::{Builder, TransportState};

const PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// 핸드셰이크 단계에서 양쪽 프로토콜 호환성을 묶는 프롤로그(다르면 핸드셰이크가 실패함).
/// 끝의 숫자는 키 교환 및 프레이밍 버전이며 메시지 버전(PROTO_VERSION)과는 별개다.
/// 메시지만 바뀔 때는 유지해야 버전이 다른 상대에게도 "프로토콜 버전 불일치" 응답을 보낼 수 있다.
const PROLOGUE: &[u8] = b"tossfront-deck/2";
/// Noise 메시지 최대 크기. 암호문은 평문보다 16바이트 크다.
const MAX_NOISE: usize = 65535;
pub const MAX_PLAIN: usize = MAX_NOISE - 16;
/// 키 길이(바이트). 비밀키와 공개키가 동일하다.
const KEY_LEN: usize = 32;

fn bad(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() || !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

/// 기기 키 쌍. 공개키가 신원 ID가 된다.
#[derive(Clone)]
pub struct Identity {
    private: Vec<u8>,
    public: Vec<u8>,
}

impl Identity {
    pub fn generate() -> io::Result<Self> {
        let pair = Builder::new(PATTERN.parse().map_err(bad)?).generate_keypair().map_err(bad)?;
        Ok(Self { private: pair.private, public: pair.public })
    }

    /// 16진수 인코딩된 공개키(기기 ID)
    pub fn id(&self) -> String {
        to_hex(&self.public)
    }

    /// 파일에서 키를 불러오고, 없으면 새로 생성해 저장한다(비밀키 포함이므로 소유자 전용 권한).
    /// 파일이 존재하지만 읽을 수 없거나 포맷이 올바르지 않으면 덮어쓰지 않고 에러를 반환한다.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let mut lines = text.lines();
                let mut key = || lines.next().and_then(from_hex).filter(|k| k.len() == KEY_LEN);
                return match (key(), key()) {
                    (Some(private), Some(public)) => Ok(Self { private, public }),
                    _ => Err(bad(format!("{}의 키 형식이 올바르지 않음 (16진수 {KEY_LEN}바이트 키 두 줄 필요)", path.display()))),
                };
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            Err(_) => {}
        }
        let me = Self::generate()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(path)?;
        writeln!(f, "{}\n{}", to_hex(&me.private), to_hex(&me.public))?;
        Ok(me)
    }
}

fn write_frame(w: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(2 + body.len());
    frame.extend_from_slice(&(body.len() as u16).to_be_bytes());
    frame.extend_from_slice(body);
    w.write_all(&frame)?;
    w.flush()
}

fn read_frame(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len)?;
    let mut body = vec![0u8; u16::from_be_bytes(len) as usize];
    r.read_exact(&mut body)?;
    Ok(body)
}

/// 암호화된 연결. send는 여러 스레드에서 호출할 수 있고, recv는 단일 수신 스레드에서 사용한다.
pub struct Conn {
    writer: Mutex<TcpStream>,
    noise: Mutex<TransportState>,
    /// 상대방 공개키(16진수 ID)
    pub remote: String,
    /// 이 세션에서 파생된 6자리 검증 코드("123 456"). 양쪽에서 동일한 값이 계산된다.
    /// 중간자 공격 시 서로 다른 값이 나오므로 페어링 시 사용자가 두 화면의 숫자를 비교한다.
    pub code: String,
}

impl Conn {
    /// 새로 맺은 TCP 연결에서 키 교환 핸드셰이크를 수행한다. initiator는 접속을 연 쪽(호스트).
    /// 반환되는 TcpStream은 수신(recv) 전용으로 사용한다.
    pub fn establish(stream: TcpStream, me: &Identity, initiator: bool) -> io::Result<(Conn, TcpStream)> {
        let mut reader = stream.try_clone()?;
        let mut writer = stream;
        let builder = Builder::new(PATTERN.parse().map_err(bad)?).local_private_key(&me.private).map_err(bad)?.prologue(PROLOGUE).map_err(bad)?;
        let mut hs = if initiator { builder.build_initiator() } else { builder.build_responder() }.map_err(bad)?;

        let mut buf = vec![0u8; MAX_NOISE];
        let mut scratch = vec![0u8; MAX_NOISE];
        let mut my_turn = initiator;
        while !hs.is_handshake_finished() {
            if my_turn {
                let n = hs.write_message(&[], &mut buf).map_err(bad)?;
                write_frame(&mut writer, &buf[..n])?;
            } else {
                let msg = read_frame(&mut reader)?;
                hs.read_message(&msg, &mut scratch).map_err(bad)?;
            }
            my_turn = !my_turn;
        }
        let remote = to_hex(hs.get_remote_static().ok_or_else(|| bad("상대방 공개키를 받지 못함"))?);
        let hash = hs.get_handshake_hash();
        let n = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) % 1_000_000;
        let code = format!("{:03} {:03}", n / 1000, n % 1000);
        let noise = hs.into_transport_mode().map_err(bad)?;
        Ok((Conn { writer: Mutex::new(writer), noise: Mutex::new(noise), remote, code }, reader))
    }

    /// 메시지 1개 전송
    pub fn send<T: Serialize>(&self, msg: &T) -> io::Result<()> {
        let plain = serde_json::to_vec(msg).map_err(bad)?;
        if plain.len() > MAX_PLAIN {
            return Err(bad("메시지 크기 초과"));
        }
        let mut writer = self.writer.lock().unwrap();
        let mut out = vec![0u8; plain.len() + 16];
        let n = self.noise.lock().unwrap().write_message(&plain, &mut out).map_err(bad)?;
        write_frame(&mut *writer, &out[..n])
    }

    /// 메시지 1개 수신(블로킹). JSON 파싱 실패 시 Ok(None)을 반환해 무시하고 다음 메시지를 계속 읽을 수 있게 한다.
    /// 복호화 실패 시에는 에러를 반환하므로 연결을 닫아야 한다.
    pub fn recv<T: DeserializeOwned>(&self, reader: &mut TcpStream) -> io::Result<Option<T>> {
        let cipher = read_frame(reader)?;
        let mut plain = vec![0u8; cipher.len()];
        let n = self.noise.lock().unwrap().read_message(&cipher, &mut plain).map_err(bad)?;
        Ok(serde_json::from_slice(&plain[..n]).ok())
    }

    pub fn shutdown(&self) {
        let _ = self.writer.lock().unwrap().shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeckMsg, HostMsg};
    use std::net::TcpListener;
    use std::thread;

    fn pair_of_conns(deck: &Identity, host: &Identity) -> ((Conn, TcpStream), (Conn, TcpStream)) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let deck = deck.clone();
        let accept = thread::spawn(move || Conn::establish(listener.accept().unwrap().0, &deck, false).unwrap());
        let h = Conn::establish(TcpStream::connect(addr).unwrap(), host, true).unwrap();
        (accept.join().unwrap(), h)
    }

    #[test]
    fn both_sides_learn_each_others_key_and_the_same_code() {
        let (deck, host) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let ((d, mut d_reader), (h, mut h_reader)) = pair_of_conns(&deck, &host);
        assert_eq!(d.remote, host.id());
        assert_eq!(h.remote, deck.id());
        assert_eq!(d.code, h.code);
        assert_eq!(d.code.len(), 7);

        h.send(&HostMsg::Ping).unwrap();
        h.send(&HostMsg::PairConfirm).unwrap();
        assert_eq!(d.recv::<HostMsg>(&mut d_reader).unwrap(), Some(HostMsg::Ping));
        assert_eq!(d.recv::<HostMsg>(&mut d_reader).unwrap(), Some(HostMsg::PairConfirm));
        d.send(&DeckMsg::Paired).unwrap();
        assert_eq!(h.recv::<DeckMsg>(&mut h_reader).unwrap(), Some(DeckMsg::Paired));

        let ((d2, _), _) = pair_of_conns(&deck, &host);
        assert_ne!(d.code, d2.code);
    }

    #[test]
    fn the_wire_does_not_carry_plain_text() {
        let (deck, host) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let raw = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let me = deck;
            let (conn, mut reader) = Conn::establish(s.try_clone().unwrap(), &me, false).unwrap();
            drop(conn);
            let mut seen = vec![0u8; 256];
            let n = reader.read(&mut seen).unwrap();
            let _ = s.flush();
            seen.truncate(n);
            seen
        });
        let (h, _) = Conn::establish(TcpStream::connect(addr).unwrap(), &host, true).unwrap();
        h.send(&serde_json::json!({"t": "media", "secret-title": "아무도 모르는 곡"})).unwrap();
        let seen = raw.join().unwrap();
        assert!(!seen.is_empty());
        assert!(!String::from_utf8_lossy(&seen).contains("secret-title"));
    }

    #[test]
    fn identity_is_kept_in_a_file() {
        let path = std::env::temp_dir().join(format!("toss-proto-key-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        let a = Identity::load_or_create(&path).unwrap();
        let b = Identity::load_or_create(&path).unwrap();
        assert_eq!(a.id(), b.id());
        assert_eq!(a.id().len(), 64);
        let _ = fs::remove_file(&path);
    }

    /// 길이가 맞지 않는 키는 거부한다(긴 키는 연결 시 오류, 짧은 키는 조용히 다른 기기로 취급될 수 있음). 기존 파일은 건드리지 않는다.
    #[test]
    fn a_key_of_the_wrong_length_is_refused_and_left_alone() {
        let path = std::env::temp_dir().join(format!("toss-proto-badkey-{}", std::process::id()));
        for text in [format!("{}\n{}\n", "ab".repeat(40), "cd".repeat(32)), format!("{}\n{}\n", "ab".repeat(32), "cd".repeat(8)), "not a key\n".to_string()] {
            fs::write(&path, &text).unwrap();
            assert!(Identity::load_or_create(&path).is_err(), "{text}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
        let _ = fs::remove_file(&path);
    }
}
