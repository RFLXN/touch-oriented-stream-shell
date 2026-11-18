//! 덱 절전 전환 및 WoWLAN 깨우기(CLI `sleep`/`wake` 및 웹 콘솔 공용).
//!
//! 절전 전환은 덱에 직접 접속해 HostMsg::Sleep을 보내 처리한다.
//! 절전 중인 덱은 TCP 접속을 받을 수 없으므로, 깨우기는 연결 시 덱이 전달해 둔 Wi-Fi MAC(Deck.wake)으로 매직 패킷을 전송한다.

use std::net::{IpAddr, TcpStream, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

use toss_proto::secure::Identity;
use toss_proto::{DeckMsg, HostMsg, SLEEP_NO_WIFI, STATUS_PAIRED};

use crate::link::{attach, denied_reason, dial_list, explain, resolve, AttachError};
use crate::store::Deck;

/// 덱에 접속해 절전 명령을 보내고 응답을 기다린다. 성공 시 (덱 이름, wake 주소 보유 여부)를 반환한다.
/// 실행 중인 toss-connector가 해당 덱에 연결되어 있었다면 그 연결은 끊긴다(덱이 절전에 들어가면 어차피 끊긴다).
pub fn sleep_deck(me: &Identity, deck: &Deck) -> Result<(String, bool), String> {
    let mut failed = None;
    for addr in dial_list([deck]) {
        let mut a = match attach(&addr, me, false, |key| (key == deck.key).then(Vec::new)) {
            Ok(Some(a)) => a,
            Ok(None) | Err(AttachError::Unreachable(_)) => continue,
            Err(AttachError::Failed(why)) => {
                failed = Some(format!("{why} ({addr})"));
                continue;
            }
        };
        if a.status != STATUS_PAIRED {
            let reason = denied_reason(&mut a);
            return Err(format!("덱이 연결을 거부했습니다: {}", explain(&reason)));
        }
        a.conn.send(&HostMsg::Sleep).map_err(|e| format!("메시지 전송 실패: {e}"))?;
        let until = Instant::now() + Duration::from_secs(4);
        while Instant::now() < until {
            match a.conn.recv::<DeckMsg>(&mut a.reader) {
                Ok(Some(DeckMsg::Sleeping { ok: true, .. })) => return Ok((a.name.clone(), !(a.wake.is_empty() && deck.wake.is_empty()))),
                Ok(Some(DeckMsg::Sleeping { ok: false, reason })) => {
                    return Err(match reason.as_str() {
                        SLEEP_NO_WIFI => "덱이 Wi-Fi에 연결되어 있지 않아 절전에 들어가지 않았습니다 (깨울 수단이 없음). 덱 설정에서 Wi-Fi 연결 후 다시 시도하세요",
                        _ => "이 덱은 절전 모드를 지원하지 않습니다",
                    }
                    .to_string())
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        return Err("덱이 응답하지 않았습니다 (절전 기능을 지원하지 않는 구버전일 수 있음)".into());
    }
    Err(failed.unwrap_or_else(|| "덱에 연결할 수 없습니다 (이미 절전 중이거나 전원이 꺼져 있음)".to_string()))
}

/// 절전 중인 덱의 Wi-Fi MAC으로 매직 패킷을 브로드캐스트하고, 다시 TCP 접속이 가능한지 확인한다.
/// 깨어나면 연결 확인된 주소를 반환한다. sent 콜백은 패킷을 전송한 뒤 한 번 호출된다.
pub fn wake_deck(deck: &Deck, sent: impl FnOnce()) -> Result<String, String> {
    let mac: Vec<u8> = deck.wake.split(':').filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
    if mac.len() != 6 {
        return Err("이 덱을 깨울 Wi-Fi MAC 주소를 모릅니다. 덱이 Wi-Fi에 연결된 상태에서 `toss-connector`를 한 번 실행하면 자동으로 저장됩니다".into());
    }
    let mut packet = vec![0xffu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }
    let mut targets = vec!["255.255.255.255".to_string()];
    for a in &deck.addrs {
        if let Ok(IpAddr::V4(ip)) = a.parse::<IpAddr>() {
            let o = ip.octets();
            for t in [format!("{}.{}.{}.255", o[0], o[1], o[2]), a.clone()] {
                if !targets.contains(&t) {
                    targets.push(t);
                }
            }
        }
    }
    let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("UDP 소켓 생성 실패: {e}"))?;
    let _ = sock.set_broadcast(true);
    let mut count = 0;
    for _ in 0..3 {
        for t in &targets {
            if sock.send_to(&packet, (t.as_str(), 9)).is_ok() {
                count += 1;
            }
        }
        thread::sleep(Duration::from_millis(150));
    }
    if count == 0 {
        return Err("매직 패킷을 전송하지 못했습니다 (네트워크 연결을 확인하세요)".into());
    }
    sent();
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        for addr in dial_list([deck]) {
            if let Ok(sa) = resolve(&addr) {
                if TcpStream::connect_timeout(&sa, Duration::from_millis(700)).is_ok() {
                    return Ok(addr);
                }
            }
        }
        for t in &targets {
            let _ = sock.send_to(&packet, (t.as_str(), 9));
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err("덱이 깨어나지 않았습니다. 절전 전 Wi-Fi 연결 상태와 이 PC와 같은 네트워크인지 확인하세요. 응답이 없으면 덱의 재부팅 버튼을 누르세요".into())
}
