//! 호스트 연결 세션: 핸드셰이크 후 `hello`를 검증해 수락 여부를 결정하고, 페어링된 호스트라면 연결 종료 시까지 상태를 수신한다.
//!
//! 미등록 키에는 거절 `hello`와 사유만 전송한다(덱 주소와 WoWLAN MAC 주소는 비워서 보낸다).
//! 호스트가 전달하는 문자열(이름, 권한, 명령 및 메트릭 목록)은 길이와 제어 문자를 정제·검증해 수신한다 — UI와 로그에 그대로 노출되기 때문이다.
//! 금고 요청(`vault`)은 화면에서 허용 여부를 묻거나 토큰 주소에 요청하는 동안 오래 걸릴 수 있으므로 요청마다 별도 스레드에서 처리한다
//! (그동안에도 다른 메시지는 계속 받는다). 연결이 끊기면 이 호스트가 화면에 띄운 질문을 거두어들인다.

use std::io;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use toss_proto::secure::Conn;
use toss_proto::{CommandInfo, DeckMsg, HostMsg, MetricInfo, DEAD_AFTER, DENIED_BUSY, DENIED_NOT_PAIRED, PING_EVERY, PROTO_VERSION, STATUS_DENIED, STATUS_PAIRED, STATUS_PAIRING, VAULT_ERR_BUSY};

use super::blob::take_piece;
use super::pairing::Pending;
use super::{Candidate, Entry, HostInfo, Hosts};
use crate::sys;

/// 호스트명·OS명 최대 길이(문자 수), 허용 권한 최대 개수·길이, 액션 오류 메시지 최대 길이
const NAME_MAX: usize = 64;
const OS_MAX: usize = 32;
const CAPS_MAX: usize = 32;
const CAP_LEN_MAX: usize = 32;
const ERROR_MAX: usize = 200;
/// 호스트당 명령·스위치/노브 값·메트릭 최대 개수
const COMMANDS_MAX: usize = 256;
const METRICS_MAX: usize = 512;
/// 호스트당 동시에 처리하는 금고 요청 최대 개수(넘으면 busy로 거절)
const VAULT_INFLIGHT_MAX: usize = 4;

/// 접속한 호스트 정보(`hello`에서 파싱)
struct Peer {
    /// 현재 세션 식별자
    session: u64,
    /// 호스트 공개키(hex)
    key: String,
    name: String,
    os: String,
    /// 연결 경로(`usb` / `lan` / `wlan`)
    via: String,
}

/// 세션에서 처리한 메시지로 인해 갱신이 필요한 상태 플래그
#[derive(Clone, Copy, Default)]
struct Changed {
    hosts: bool,
    values: bool,
    meters: bool,
}

impl Changed {
    const NONE: Self = Self { hosts: false, values: false, meters: false };
    const HOSTS: Self = Self { hosts: true, ..Self::NONE };
    const VALUES: Self = Self { values: true, ..Self::NONE };
    const METERS: Self = Self { meters: true, ..Self::NONE };
}

/// 호스트가 보낸 문자열 정제: 제어 문자를 제거하고 최대 `max` 글자로 자른다.
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// 호스트가 보고한 권한 목록 중 유효한 식별자만 필터링한다(`toss-proto`의 `CAP_*` 형식: 짧은 ASCII 그래픽 문자).
fn clean_caps(caps: Vec<String>) -> Vec<String> {
    caps.into_iter().filter(|c| !c.is_empty() && c.len() <= CAP_LEN_MAX && c.bytes().all(|b| b.is_ascii_graphic())).take(CAPS_MAX).collect()
}

fn sane_command(c: &CommandInfo) -> bool {
    c.id.len() <= 64 && c.name.chars().count() <= 80 && c.kind.len() <= 16 && c.steps.len() <= 32 && c.steps.iter().all(|s| s.chars().count() <= 32) && [c.min, c.max, c.step].iter().all(|v| v.is_finite())
}

fn sane_metric(m: &MetricInfo) -> bool {
    !m.id.is_empty() && m.id.len() <= 64 && m.name.chars().count() <= 8 && m.title.chars().count() <= 64 && m.unit.chars().count() <= 8 && m.max.is_finite() && m.max >= 0.0 && m.decimals <= 6
}

impl Hosts {
    /// 호스트 연결 세션 1건을 처음부터 끝까지 처리한다. 어떤 사유로든 종료되면 keep-alive 스레드를 중단하고 소켓을 닫는다.
    pub(super) fn session(self: &Arc<Self>, stream: TcpStream) -> io::Result<()> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(DEAD_AFTER))?;
        stream.set_write_timeout(Some(DEAD_AFTER))?;
        let via = sys::link_kind(stream.local_addr()?.ip());
        let (conn, mut reader) = Conn::establish(stream, &self.identity, false)?;
        let conn = Arc::new(conn);

        let Some(HostMsg::Hello { proto, name, os, caps, pair }) = conn.recv(&mut reader)? else {
            return Err(io::Error::other("첫 메시지가 hello가 아닙니다"));
        };
        let peer = Peer { session: self.next_session.fetch_add(1, Ordering::SeqCst), key: conn.remote.clone(), name: clean(&name, NAME_MAX), os: clean(&os, OS_MAX), via };
        let hello = |status: &str, known: bool| DeckMsg::Hello {
            proto: PROTO_VERSION,
            name: self.deck_name.clone(),
            status: status.to_string(),
            addrs: if known { sys::addresses() } else { Vec::new() },
            wake: if known { sys::wake_mac() } else { String::new() },
        };
        if proto != PROTO_VERSION {
            let _ = conn.send(&hello(STATUS_DENIED, false));
            conn.shutdown();
            return Err(io::Error::other(format!("프로토콜 버전 불일치(호스트 {proto}, 덱 {PROTO_VERSION})")));
        }

        enum Greeting {
            Paired,
            Pairing,
            Denied(&'static str),
        }
        let greeting = {
            let mut inner = self.inner.lock().unwrap();
            if inner.entries.iter().any(|e| e.info.id == peer.key) {
                Greeting::Paired
            } else if !pair || inner.pairing_until.is_none() {
                Greeting::Denied(DENIED_NOT_PAIRED)
            } else if inner.pending.is_some() {
                Greeting::Denied(DENIED_BUSY)
            } else {
                let info = Candidate { name: peer.name.clone(), os: peer.os.clone(), via: peer.via.clone(), code: conn.code.clone(), host_confirmed: false, deck_accepted: false };
                inner.pending = Some(Pending { session: peer.session, key: peer.key.clone(), info, conn: conn.clone() });
                self.publish(&inner);
                Greeting::Pairing
            }
        };

        let alive = Arc::new(AtomicBool::new(true));
        {
            let (alive, conn) = (alive.clone(), conn.clone());
            thread::spawn(move || {
                while alive.load(Ordering::SeqCst) {
                    thread::sleep(PING_EVERY);
                    if conn.send(&DeckMsg::Ping).is_err() {
                        break;
                    }
                }
            });
        }
        let greet = || match greeting {
            Greeting::Denied(why) => {
                eprintln!("host: 연결 거절({why}): {} ({})", peer.name, peer.via);
                conn.send(&hello(STATUS_DENIED, false))?;
                conn.send(&DeckMsg::Denied { reason: why.to_string() })
            }
            Greeting::Paired if pair => conn.send(&hello(STATUS_PAIRED, true)),
            Greeting::Paired => {
                conn.send(&hello(STATUS_PAIRED, true))?;
                self.serve(&conn, &mut reader, &peer, caps)
            }
            Greeting::Pairing => {
                eprintln!("host: 페어링 요청: {} ({})", peer.name, peer.via);
                conn.send(&hello(STATUS_PAIRING, true))?;
                if self.wait_pairing(&conn, &mut reader, peer.session)? {
                    self.serve(&conn, &mut reader, &peer, caps)?;
                }
                Ok(())
            }
        };
        let result = greet();
        alive.store(false, Ordering::SeqCst);
        conn.shutdown();
        result
    }

    /// 현재 세션의 호스트 엔트리를 갱신한다(이미 새 세션으로 교체된 경우 무시).
    /// `change`가 변경된 것으로 표시한 채널만 UI에 다시 전송한다.
    fn update(&self, session: u64, change: impl FnOnce(&mut Entry) -> Changed) {
        let mut inner = self.inner.lock().unwrap();
        let Some(e) = inner.entries.iter_mut().find(|e| e.session == session) else { return };
        let changed = change(e);
        if changed.hosts {
            self.publish(&inner);
        }
        if changed.values {
            self.publish_values(&inner);
        }
        if changed.meters {
            self.publish_meters(&inner);
        }
    }

    /// 페어링된 호스트의 일반 세션 루프. 연결이 끊길 때까지 반환하지 않는다.
    fn serve(self: &Arc<Self>, conn: &Arc<Conn>, reader: &mut TcpStream, peer: &Peer, caps: Vec<String>) -> io::Result<()> {
        let (old, renamed) = {
            let mut inner = self.inner.lock().unwrap();
            let watch = inner.watch.get(&peer.key).cloned().unwrap_or_default();
            let Some(e) = inner.entries.iter_mut().find(|e| e.info.id == peer.key) else {
                return Ok(());
            };
            let renamed = e.info.name != peer.name || e.info.os != peer.os;
            let old = e.conn.take();
            e.reset();
            e.info = HostInfo { caps: clean_caps(caps), via: peer.via.clone(), connected: true, ..HostInfo::offline(peer.key.clone(), peer.name.clone(), peer.os.clone()) };
            e.session = peer.session;
            e.conn = Some(conn.clone());
            self.publish_values(&inner);
            self.publish_meters(&inner);
            let _ = conn.send(&DeckMsg::Watch { metrics: watch });
            self.publish(&inner);
            (old, renamed)
        };
        if let Some(old) = old {
            old.shutdown();
        }
        if renamed {
            self.save();
        }
        eprintln!("host: 연결됨: {} ({})", peer.name, peer.via);
        let _ = conn.send(&DeckMsg::VaultState { services: self.vault.status_for(&peer.key) });
        let inflight = Arc::new(AtomicUsize::new(0));

        let ended = loop {
            let msg = match conn.recv::<HostMsg>(reader) {
                Ok(Some(msg)) => msg,
                Ok(None) => continue,
                Err(e) => break e,
            };
            match msg {
                HostMsg::Media { media } => self.update(peer.session, |e| {
                    let art_now = media.as_ref().map(|m| m.art.as_str()).unwrap_or_default();
                    if e.info.art.as_ref().is_some_and(|a| a.id != art_now) {
                        e.info.art = None;
                    }
                    e.info.media = media;
                    e.media_at = Instant::now();
                    Changed::HOSTS
                }),
                HostMsg::Blob { id, mime, index, count, data } => self.update(peer.session, |e| match take_piece(&mut e.blob, id, mime, index, count, data) {
                    Some(art) => {
                        e.info.art = Some(art);
                        Changed::HOSTS
                    }
                    None => Changed::NONE,
                }),
                HostMsg::Commands { commands } => self.update(peer.session, |e| {
                    let commands: Vec<CommandInfo> = commands.into_iter().filter(sane_command).take(COMMANDS_MAX).collect();
                    let before = e.values.len();
                    e.values.retain(|id, _| commands.iter().any(|c| c.id == *id));
                    let listed = e.info.commands != commands;
                    if listed {
                        e.info.commands = commands;
                    }
                    Changed { hosts: listed, values: e.values.len() != before, meters: false }
                }),
                HostMsg::Metrics { metrics } => self.update(peer.session, |e| {
                    let metrics: Vec<MetricInfo> = metrics.into_iter().filter(sane_metric).take(METRICS_MAX).collect();
                    e.metric_values.retain(|id, _| metrics.iter().any(|m| m.id == *id));
                    e.metrics = metrics;
                    Changed::METERS
                }),
                HostMsg::MetricValues { values } => self.update(peer.session, |e| {
                    for (id, v) in values.into_iter().filter(|(_, v)| v.is_finite()) {
                        if e.metrics.iter().any(|m| m.id == id) {
                            e.metric_values.insert(id, v);
                        }
                    }
                    Changed::METERS
                }),
                HostMsg::CommandValue { id, value } => self.update(peer.session, |e| {
                    let changed = match value.filter(|v| v.is_finite()) {
                        Some(v) if id.len() <= 64 && (e.values.len() < COMMANDS_MAX || e.values.contains_key(&id)) => e.values.insert(id, v) != Some(v),
                        Some(_) => false,
                        None => e.values.remove(&id).is_some(),
                    };
                    if changed {
                        Changed::VALUES
                    } else {
                        Changed::NONE
                    }
                }),
                HostMsg::Caps { caps } => self.update(peer.session, |e| {
                    let caps = clean_caps(caps);
                    if e.info.caps == caps {
                        return Changed::NONE;
                    }
                    eprintln!("host: {} 허용 권한 변경: {}", peer.name, if caps.is_empty() { "없음".to_string() } else { caps.join(", ") });
                    e.info.caps = caps;
                    Changed::HOSTS
                }),
                HostMsg::Sleep => {
                    let done = self.events.sleep();
                    eprintln!("host: {}의 절전 요청: {}", peer.name, done.as_ref().map(|()| "진입").unwrap_or_else(|e| e));
                    let _ = conn.send(&DeckMsg::Sleeping { ok: done.is_ok(), reason: done.err().unwrap_or_default() });
                }
                HostMsg::Result { id, ok, error } => {
                    let error = clean(&error, ERROR_MAX);
                    if !ok {
                        eprintln!("host: {}의 액션 {id} 실패: {error}", peer.name);
                    }
                    let widget = self.inner.lock().unwrap().entries.iter_mut().find(|e| e.session == peer.session).and_then(|e| e.asked.remove(&id));
                    if let Some(widget) = widget {
                        self.events.result(widget, ok, error);
                    }
                }
                HostMsg::Vault { id, op, service, args } => {
                    if inflight.fetch_add(1, Ordering::SeqCst) >= VAULT_INFLIGHT_MAX {
                        inflight.fetch_sub(1, Ordering::SeqCst);
                        let _ = conn.send(&DeckMsg::VaultReply { id, ok: false, error: VAULT_ERR_BUSY.into(), data: serde_json::Value::Null });
                        continue;
                    }
                    let (me, conn, key, name, inflight) = (self.clone(), conn.clone(), peer.key.clone(), peer.name.clone(), inflight.clone());
                    thread::spawn(move || {
                        let reply = match me.vault.handle(&key, &name, &op, &service, &args) {
                            Ok(data) => DeckMsg::VaultReply { id, ok: true, error: String::new(), data },
                            Err(error) => DeckMsg::VaultReply { id, ok: false, error, data: serde_json::Value::Null },
                        };
                        inflight.fetch_sub(1, Ordering::SeqCst);
                        let _ = conn.send(&reply);
                        me.announce_vault();
                    });
                }
                HostMsg::Hello { .. } | HostMsg::PairConfirm | HostMsg::Ping | HostMsg::Unknown => {}
            }
        };

        self.vault.cancel_asks(&peer.key);
        let mut inner = self.inner.lock().unwrap();
        if let Some(e) = inner.entries.iter_mut().find(|e| e.session == peer.session) {
            e.conn = None;
            e.reset();
            self.publish(&inner);
            self.publish_values(&inner);
            self.publish_meters(&inner);
            eprintln!("host: 연결 종료: {}: {ended}", peer.name);
        }
        Ok(())
    }
}
