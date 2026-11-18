//! 페어링 상태 머신: 덱과 공개키를 교환하고 저장하기까지의 단일 흐름(CLI `pair`와 웹 콘솔 공용).
//!
//!   1. begin   덱에 페어링 모드로 접속한다. 덱이 페어링 모드라면 양쪽에 같은 검증 코드가 표시된다.
//!   2. 사용자가 코드를 비교해 일치하면(answer true) 덱에 알리고(Confirmed), 아니면 취소한다(Cancelled).
//!   3. 덱에서도 수락하면 paired 응답이 오고, 이때 호스트 저장소에 덱을 등록한다(Paired).
//!
//! 호스트 사용자가 코드를 확인하기 전에는 절대 등록하지 않는다.
//! 덱이 먼저 paired를 보내더라도 거부한다(해당 주소의 피어가 진짜 덱인지 확인하는 유일한 수단이 코드 비교이기 때문).
//!
//! 사용자 응답은 웹 콘솔처럼 다른 스레드에서 올 수 있으므로 answerer 채널로 전달한다.
//! 페어링 제한 시간(PAIRING_WINDOW)이 지나거나 드롭되면 연결을 닫고 종료한다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use toss_proto::secure::{Conn, Identity};
use toss_proto::{DeckMsg, HostMsg, PAIRING_WINDOW, STATUS_PAIRED, STATUS_PAIRING};

use crate::link::{attach, denied_reason, explain, merged_addrs, start_pings, AttachError, Attached};
use crate::store::{Deck, Store};
use crate::trust::Choices;

/// 페어링 시작 결과
pub enum Begin {
    /// 검증 코드 표시됨. 사용자 확인 대기 중
    Ready(Pairing),
    /// 이미 페어링된 덱 (덱 이름)
    Already(String),
}

/// 페어링 진행 단계
pub enum Step {
    /// 호스트 사용자가 코드 일치를 확인해 덱에 전달함. 덱 측 수락 대기 중
    Confirmed,
    /// 페어링 완료 및 저장됨
    Paired,
    /// 호스트 사용자가 취소함
    Cancelled,
    /// 실패 (사유)
    Failed(String),
}

enum Event {
    Answer(bool),
    Deck(DeckMsg),
    Lost,
}

/// 사용자 응답(일치/취소)을 페어링 스레드에 전달하는 핸들(다른 스레드에서 호출 가능)
#[derive(Clone)]
pub struct Answerer(Sender<Event>);

impl Answerer {
    pub fn send(&self, same: bool) {
        let _ = self.0.send(Event::Answer(same));
    }
}

pub struct Pairing {
    store: Store,
    /// 연결에 성공한 주소(저장 시 맨 앞에 기록)
    addr: String,
    conn: Arc<Conn>,
    name: String,
    addrs: Vec<String>,
    wake: String,
    alive: Arc<AtomicBool>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    until: Instant,
    /// 호스트 측 숫자 확인 완료 여부
    confirmed: bool,
}

impl Pairing {
    /// 덱(addr)에 페어링을 요청한다. 페어링 모드가 아니거나 연결할 수 없으면 에러 사유를 반환한다.
    pub fn begin(store: &Store, me: &Identity, addr: &str) -> Result<Begin, String> {
        let mut a = match attach(addr, me, true, |_| Some(Vec::new())) {
            Ok(Some(a)) => a,
            Ok(None) => return Err("덱이 응답하지 않았습니다".to_string()),
            Err(AttachError::Unreachable(e)) => return Err(format!("덱에 연결할 수 없습니다 ({addr}): {e}")),
            Err(AttachError::Failed(why)) => return Err(why),
        };
        let known = store.decks().iter().any(|d| d.key == a.conn.remote);
        match a.status.as_str() {
            STATUS_PAIRING => {}
            STATUS_PAIRED if known => {
                a.conn.shutdown();
                return Ok(Begin::Already(a.name));
            }
            STATUS_PAIRED => {
                a.conn.shutdown();
                return Err("덱에는 이 PC가 이미 페어링되어 있지만 이 PC에는 기록이 없습니다. 덱 설정 → Paired devices에서 이 PC를 삭제(FORGET)한 뒤 다시 시도하세요".to_string());
            }
            _ => return Err(explain(&denied_reason(&mut a)).to_string()),
        }
        let alive = start_pings(&a.conn);
        let (tx, rx) = mpsc::channel::<Event>();
        let Attached { conn, mut reader, name, addrs, wake, .. } = a;
        let (read_conn, deck_tx) = (conn.clone(), tx.clone());
        thread::spawn(move || loop {
            match read_conn.recv::<DeckMsg>(&mut reader) {
                Ok(Some(m @ (DeckMsg::Paired | DeckMsg::Denied { .. }))) => {
                    if deck_tx.send(Event::Deck(m)).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    let _ = deck_tx.send(Event::Lost);
                    return;
                }
            }
        });
        Ok(Begin::Ready(Pairing { store: store.clone(), addr: addr.to_string(), conn, name, addrs, wake, alive, tx, rx, until: Instant::now() + PAIRING_WINDOW, confirmed: false }))
    }

    /// 덱 이름
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 이 연결에서 계산된 6자리 검증 코드("123 456"). 덱 화면에도 동일한 숫자가 표시되어야 한다.
    pub fn code(&self) -> &str {
        &self.conn.code
    }

    pub fn answerer(&self) -> Answerer {
        Answerer(self.tx.clone())
    }

    /// 다음 상태 전환까지 대기(사용자 응답, 덱 응답, 연결 끊김, 타임아웃).
    /// Confirmed 반환 시에는 덱의 수락을 기다리기 위해 다시 호출해야 하며, 그 외의 결과는 종료 상태다.
    pub fn next(&mut self) -> Step {
        loop {
            match self.rx.recv_timeout(self.until.saturating_duration_since(Instant::now())) {
                Ok(Event::Answer(true)) if self.confirmed => {}
                Ok(Event::Answer(true)) => {
                    if self.conn.send(&HostMsg::PairConfirm).is_err() {
                        return Step::Failed("덱과의 연결이 끊어졌습니다 (덱에서 거절했거나 페어링 모드가 만료됨)".to_string());
                    }
                    self.confirmed = true;
                    return Step::Confirmed;
                }
                Ok(Event::Answer(false)) => return Step::Cancelled,
                Ok(Event::Deck(DeckMsg::Paired)) if !self.confirmed => {
                    return Step::Failed("호스트에서 숫자를 확인하기 전에 덱이 페어링 완료를 보내와 거부했습니다".to_string());
                }
                Ok(Event::Deck(DeckMsg::Paired)) => {
                    return match self.remember() {
                        Ok(()) => Step::Paired,
                        Err(e) => Step::Failed(format!("페어링은 완료되었으나 설정 저장에 실패했습니다: {e}")),
                    };
                }
                Ok(Event::Deck(DeckMsg::Denied { reason })) => return Step::Failed(explain(&reason).to_string()),
                Ok(Event::Deck(_)) => {}
                Ok(Event::Lost) | Err(RecvTimeoutError::Disconnected) => return Step::Failed("덱과의 연결이 끊어졌습니다".to_string()),
                Err(RecvTimeoutError::Timeout) => return Step::Failed("페어링 제한 시간이 초과되었습니다. 덱에서 페어링 모드를 다시 켠 뒤 시도하세요".to_string()),
            }
        }
    }

    /// 덱 정보를 저장한다. 이전에 페어링했던 덱이라도 권한은 기본값으로 초기화한다.
    fn remember(&self) -> std::io::Result<()> {
        self.store.remember(Deck { key: self.conn.remote.clone(), name: self.name.clone(), addrs: merged_addrs(&self.addr, &self.addrs, &[]), wake: self.wake.clone(), perms: Choices::new() })
    }
}

impl Drop for Pairing {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.conn.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Duration;
    use toss_proto::PROTO_VERSION;

    /// 모의 덱 서버를 띄워 페어링을 시작하고, 덱 측 연결, Pairing 핸들, 임시 Store를 반환한다.
    fn rig(name: &str) -> (Arc<Conn>, std::net::TcpStream, Pairing, Store) {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-pairing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(Some(dir)).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let deck = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
            let (conn, mut reader) = Conn::establish(stream, &Identity::generate().unwrap(), false).unwrap();
            let Some(HostMsg::Hello { pair: true, .. }) = conn.recv::<HostMsg>(&mut reader).unwrap() else { panic!("페어링 hello가 아님") };
            conn.send(&DeckMsg::Hello { proto: PROTO_VERSION, name: "new deck".into(), status: STATUS_PAIRING.into(), addrs: vec!["10.0.0.7".into(), "evil.example:1".into()], wake: String::new() }).unwrap();
            (Arc::new(conn), reader)
        });
        let Ok(Begin::Ready(pairing)) = Pairing::begin(&store, &Identity::generate().unwrap(), &addr) else { panic!("페어링 시작 실패") };
        let (conn, reader) = deck.join().unwrap();
        (conn, reader, pairing, store)
    }

    #[test]
    fn the_deck_is_recorded_only_after_both_sides_agreed() {
        let (deck, mut reader, mut pairing, store) = rig("both");
        assert_eq!((pairing.name(), pairing.code()), ("new deck", deck.code.as_str()));
        pairing.answerer().send(true);
        assert!(matches!(pairing.next(), Step::Confirmed));
        loop {
            match deck.recv::<HostMsg>(&mut reader).unwrap() {
                Some(HostMsg::PairConfirm) => break,
                Some(HostMsg::Ping) | None => {}
                Some(other) => panic!("pair_confirm이 아님: {other:?}"),
            }
        }
        assert!(store.decks().is_empty());
        deck.send(&DeckMsg::Paired).unwrap();
        assert!(matches!(pairing.next(), Step::Paired));
        let decks = store.decks();
        assert_eq!((decks.len(), decks[0].name.as_str(), decks[0].addrs.len(), decks[0].addrs[1].as_str()), (1, "new deck", 2, "10.0.0.7"));
        let _ = std::fs::remove_dir_all(store.dir());
    }

    /// 호스트 확인 전에 덱이 Paired를 보내도 저장하지 않고 실패 처리한다.
    #[test]
    fn a_deck_that_says_paired_before_this_side_confirmed_is_not_recorded() {
        let (deck, _reader, mut pairing, store) = rig("early");
        deck.send(&DeckMsg::Paired).unwrap();
        assert!(matches!(pairing.next(), Step::Failed(_)));
        assert!(store.decks().is_empty());
        let _ = std::fs::remove_dir_all(store.dir());
    }

    #[test]
    fn saying_no_or_being_refused_ends_it_without_a_record() {
        let (_deck, _reader, mut pairing, store) = rig("no");
        pairing.answerer().send(false);
        assert!(matches!(pairing.next(), Step::Cancelled));
        assert!(store.decks().is_empty());
        let _ = std::fs::remove_dir_all(store.dir());

        let (deck, _reader, mut pairing, store) = rig("refused");
        deck.send(&DeckMsg::Denied { reason: toss_proto::DENIED_REJECTED.into() }).unwrap();
        let Step::Failed(why) = pairing.next() else { panic!("거절 응답 후 종료되지 않음") };
        assert_eq!(why, explain(toss_proto::DENIED_REJECTED));
        let _ = std::fs::remove_dir_all(store.dir());
    }
}
