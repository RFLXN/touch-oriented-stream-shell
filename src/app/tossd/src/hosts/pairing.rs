//! 페어링 관리: 미등록 호스트의 공개키를 사용자 확인을 거쳐 신뢰 목록에 추가한다(절차는 `toss-proto` 상단 및 `docs/04-host-link.md` 참고).
//!
//! UI에서 페어링 모드를 시작하면(`pair_start`) `PAIRING_WINDOW` 동안 유지된다. 그동안 페어링을 요청한 호스트 1대가 후보(`Pending`)로 등록되며,
//! 화면에 호스트명과 검증 코드가 표시된다. 덱 사용자가 코드를 확인해 수락하고(`pair_accept`) 호스트 사용자도 확인하면(`pair_confirm`)
//! 후보가 페어링된 호스트로 승격된다: 목록에 추가 후 파일에 저장하고 호스트에 `Paired`를 보낸다. 해당 연결은 그대로 일반 세션으로 이어진다.
//!
//! 후보 슬롯은 페어링이 끝나는 즉시(수락·중단 무관) 비워지므로, 호스트가 `Paired` 수신 직후 연결을 닫아도 다음 페어링을 방해하지 않는다.

use std::io;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Instant;

use toss_proto::secure::Conn;
use toss_proto::{DeckMsg, HostMsg, DENIED_TIMEOUT, PAIRING_WINDOW};

use super::{Candidate, Entry, Hosts, Inner};

/// 페어링 승인을 대기 중인 호스트
pub(super) struct Pending {
    pub(super) session: u64,
    pub(super) key: String,
    pub(super) info: Candidate,
    pub(super) conn: Arc<Conn>,
}

impl Hosts {
    /// 페어링 모드를 시작한다(이미 켜져 있으면 만료 타이머를 초기화한다).
    pub fn pair_start(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.pairing_until = Some(Instant::now() + PAIRING_WINDOW);
        self.publish(&inner);
        eprintln!("host: 페어링 모드 시작");
    }

    /// 페어링 모드를 종료한다. 대기 중인 후보가 있으면 `why`(`toss-proto`의 `DENIED_*`)를 보내고 연결을 닫는다.
    pub fn pair_stop(&self, why: &str) {
        let dropped = {
            let mut inner = self.inner.lock().unwrap();
            if inner.pairing_until.is_none() && inner.pending.is_none() {
                return;
            }
            inner.pairing_until = None;
            let dropped = inner.pending.take();
            self.publish(&inner);
            dropped
        };
        if let Some(p) = dropped {
            let _ = p.conn.send(&DeckMsg::Denied { reason: why.to_string() });
            p.conn.shutdown();
            eprintln!("host: 페어링 종료({why}): {}", p.info.name);
        }
    }

    /// 페어링 모드가 `PAIRING_WINDOW`를 경과했으면 자동으로 종료한다(1초마다 호출).
    pub(super) fn expire_pairing(&self) {
        let expired = self.inner.lock().unwrap().pairing_until.is_some_and(|t| Instant::now() >= t);
        if expired {
            self.pair_stop(DENIED_TIMEOUT);
        }
    }

    /// 덱 사용자가 검증 코드를 확인하고 수락했다. `code`는 UI에 표시된 값이다.
    /// 현재 대기 중인 후보의 코드와 다르면 무시한다(수락하는 찰나에 후보가 바뀌었을 수 있음).
    pub fn pair_accept(&self, code: &str) {
        let paired = {
            let mut inner = self.inner.lock().unwrap();
            match inner.pending.as_mut() {
                Some(p) if p.info.code == code => p.info.deck_accepted = true,
                Some(p) => {
                    eprintln!("host: 페어링 수락 무시: UI 코드와 현재 후보({})의 코드가 다름", p.info.name);
                    return;
                }
                None => return,
            }
            let paired = self.finish_pairing(&mut inner);
            self.publish(&inner);
            paired
        };
        self.announce_paired(paired);
    }

    /// 양측 확인이 모두 완료되었으면 페어링을 확정한다: 후보를 정식 목록으로 옮기고(후보 슬롯 해제) 페어링 모드를 종료한다.
    /// 확정되었으면 해당 연결을 반환하며, 호출 측이 락을 푼 뒤 `announce_paired`에 넘긴다.
    fn finish_pairing(&self, inner: &mut Inner) -> Option<Arc<Conn>> {
        if !inner.pending.as_ref().is_some_and(|p| p.info.deck_accepted && p.info.host_confirmed) {
            return None;
        }
        let p = inner.pending.take()?;
        eprintln!("host: 페어링 완료: {}", p.info.name);
        inner.entries.retain(|e| e.info.id != p.key);
        let mut entry = Entry::new(p.key, p.info.name, p.info.os);
        entry.session = p.session;
        inner.entries.push(entry);
        inner.pairing_until = None;
        Some(p.conn)
    }

    /// 확정된 페어링을 파일에 저장한 뒤 호스트에 `Paired`를 전송한다(저장 후 전송하므로 호스트는 저장 완료를 신뢰할 수 있다).
    fn announce_paired(&self, conn: Option<Arc<Conn>>) {
        if let Some(conn) = conn {
            self.save();
            let _ = conn.send(&DeckMsg::Paired);
        }
    }

    /// 후보 세션에서 페어링 완료 또는 종료를 대기한다. 성공 시 `true`, 거절/중단되었거나 그사이에 연결이 끊겨 이미 후보가 아니면 `false`.
    /// 후보인 채로 연결이 끊기면 후보 슬롯을 비우고 오류를 반환한다.
    pub(super) fn wait_pairing(&self, conn: &Conn, reader: &mut TcpStream, session: u64) -> io::Result<bool> {
        loop {
            {
                let inner = self.inner.lock().unwrap();
                if !inner.pending.as_ref().is_some_and(|p| p.session == session) {
                    return Ok(inner.entries.iter().any(|e| e.session == session));
                }
            }
            match conn.recv::<HostMsg>(reader) {
                Ok(Some(HostMsg::PairConfirm)) => {
                    let paired = {
                        let mut inner = self.inner.lock().unwrap();
                        if let Some(p) = inner.pending.as_mut().filter(|p| p.session == session) {
                            p.info.host_confirmed = true;
                        }
                        let paired = self.finish_pairing(&mut inner);
                        self.publish(&inner);
                        paired
                    };
                    self.announce_paired(paired);
                }
                Ok(_) => {}
                Err(e) => {
                    let mut inner = self.inner.lock().unwrap();
                    if !inner.pending.as_ref().is_some_and(|p| p.session == session) {
                        return Ok(false);
                    }
                    inner.pending = None;
                    self.publish(&inner);
                    return Err(e);
                }
            }
        }
    }
}
