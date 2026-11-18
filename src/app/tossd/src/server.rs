//! UI 클라이언트 연결 처리: QML 클라이언트의 WebSocket 연결을 수락해 요청을 처리하고 변경된 상태를 푸시한다.
//! 연결마다 전담 스레드를 하나씩 생성하며, 모든 연결은 `Shared`(`state.rs`)를 공유한다. 메시지 형식은 `ui_proto.rs` 참고.
//!
//! 새로 연결된 클라이언트에는 현재 전체 상태를 보내고, 이후에는 버전이 바뀐 항목만 전송한다(`Seen`).

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tungstenite::{accept, Error as WsError, Message, WebSocket};

use crate::device::Device;
use crate::hosts;
use crate::layout;
use crate::state::{spawn_feeds, Shared};
use crate::ui_proto::{ClientMsg, ServerMsg};
use crate::wifi::WifiCmd;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// 클라이언트 수신 타임아웃. 이 주기마다 변경된 상태를 확인해 전송한다.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 연결별로 마지막에 전송한 상태 버전. 초기값은 모두 미전송 상태이므로 접속 직후 전체 상태가 전송된다.
/// 액션 결과(`result_seq`)만 예외적으로 접속 이후 발생한 것부터 전송한다.
struct Seen {
    sys: u64,
    wifi: u64,
    layout: u64,
    device: u64,
    hosts: u64,
    values: u64,
    meter_infos: u64,
    meter_values: u64,
    results: u64,
    result_seq: u64,
    vault: u64,
    icons: u64,
}

impl Seen {
    fn new(shared: &Shared) -> Self {
        let never = u64::MAX;
        Self { sys: never, wifi: never, layout: never, device: never, hosts: never, values: never, meter_infos: never, meter_values: never, results: never, result_seq: shared.results.lock().seq(), vault: never, icons: never }
    }
}

fn send(ws: &mut WebSocket<TcpStream>, msg: &ServerMsg) -> Result<(), WsError> {
    ws.send(Message::text(serde_json::to_string(msg).expect("메시지 직렬화 실패")))
}

/// 변경된 상태를 클라이언트로 전송한다.
fn push(ws: &mut WebSocket<TcpStream>, shared: &Shared, seen: &mut Seen) -> Result<(), WsError> {
    if let Some(sys) = shared.sys.fresh(&mut seen.sys) {
        send(ws, &ServerMsg::State(&sys))?;
    }
    if let Some(wifi) = shared.wifi.fresh(&mut seen.wifi) {
        send(ws, &ServerMsg::Wifi(&wifi))?;
    }
    if let Some(layout) = shared.layout.fresh(&mut seen.layout) {
        send(ws, &ServerMsg::Layout(&layout))?;
    }
    if let Some(device) = shared.device_state.fresh(&mut seen.device) {
        send(ws, &ServerMsg::Device(&device))?;
    }
    if let Some(hosts) = shared.hosts.fresh(&mut seen.hosts) {
        send(ws, &ServerMsg::Hosts(&hosts))?;
    }
    if let Some(values) = shared.values.fresh(&mut seen.values) {
        send(ws, &ServerMsg::Values { hosts: &values })?;
    }
    if let Some(infos) = shared.meter_infos.fresh(&mut seen.meter_infos) {
        send(ws, &ServerMsg::Meters { hosts: &infos })?;
    }
    if let Some(values) = shared.meter_values.fresh(&mut seen.meter_values) {
        send(ws, &ServerMsg::MeterValues { hosts: &values })?;
    }
    if let Some(vault) = shared.vault.fresh(&mut seen.vault) {
        send(ws, &ServerMsg::Vault(&vault))?;
    }
    if let Some(icons) = shared.icons.fresh(&mut seen.icons) {
        send(ws, &ServerMsg::Icons { icons: &icons })?;
    }
    if let Some(results) = shared.results.fresh(&mut seen.results) {
        for (widget, seq, ok, error) in results.since(seen.result_seq) {
            send(ws, &ServerMsg::Result { widget, seq, ok, error })?;
        }
        seen.result_seq = results.seq();
    }
    Ok(())
}

/// 클라이언트 요청 메시지 1건을 처리한다.
fn handle(shared: &Arc<Shared>, msg: ClientMsg) {
    let msg = match msg.into_edit() {
        Ok(edit) => return shared.edit_layout(edit),
        Err(msg) => msg,
    };
    match msg {
        ClientMsg::Brightness { value, keep } => shared.set_brightness(value, keep),
        ClientMsg::PowerOff => shared.power_off(),
        ClientMsg::Sleep => {
            if let Err(why) = shared.try_sleep() {
                eprintln!("device: 절전 진입 거부({why})");
            }
        }
        ClientMsg::Ssh { on } => shared.set_ssh(on),
        ClientMsg::WifiScan => shared.wifi_cmd(WifiCmd::Scan),
        ClientMsg::WifiConnect { key, psk } => shared.wifi_cmd(WifiCmd::Connect { key, psk }),
        ClientMsg::WifiForget { key } => shared.wifi_cmd(WifiCmd::Forget { key }),
        ClientMsg::Action { host, name, args, widget } => shared.action(&host, &name, args, widget),
        about_hosts => {
            let Some(hosts) = shared.hosts_ctl() else { return };
            match about_hosts {
                ClientMsg::PairStart => hosts.pair_start(),
                ClientMsg::PairStop | ClientMsg::PairReject => hosts.pair_stop(toss_proto::DENIED_REJECTED),
                ClientMsg::PairAccept { code } => hosts.pair_accept(&code),
                ClientMsg::HostForget { id } => hosts.forget(&id),
                ClientMsg::VaultAnswer { allow } => hosts.vault_answer(allow),
                ClientMsg::VaultRevoke { service, host } => hosts.vault_revoke(&service, &host),
                ClientMsg::VaultForget { service } => hosts.vault_forget(&service),
                _ => {}
            }
        }
    }
}

/// 파싱할 수 없는 메시지를 수신했을 때 종류와 길이만 로깅한다(본문에는 Wi-Fi 비밀번호가 포함될 수 있으므로 남기지 않는다).
/// 배치 편집 메시지(`layout_*`)였다면 현재 배치를 다시 보내 클라이언트의 임시 UI 상태를 되돌린다.
fn unknown(shared: &Shared, text: &str) {
    let kind = serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| v["type"].as_str().map(str::to_string)).unwrap_or_else(|| "?".into());
    eprintln!("알 수 없는 메시지: type={kind}, {}바이트", text.len());
    if kind.starts_with("layout_") {
        shared.resend_layout();
    }
}

fn serve(stream: TcpStream, shared: Arc<Shared>) -> Result<(), WsError> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let mut ws = accept(stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(e) => e,
        tungstenite::HandshakeError::Interrupted(_) => WsError::Io(std::io::Error::new(ErrorKind::TimedOut, "핸드셰이크 시간 초과")),
    })?;
    ws.get_ref().set_read_timeout(Some(POLL_INTERVAL))?;

    send(&mut ws, &ServerMsg::Hello { version: env!("CARGO_PKG_VERSION") })?;
    send(&mut ws, &ServerMsg::Catalog { apps: &layout::catalog() })?;
    if let Some(hosts) = shared.hosts_ctl() {
        hosts.refresh();
    }

    let mut seen = Seen::new(&shared);
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<ClientMsg>(text.as_str()) {
                Ok(msg) => handle(&shared, msg),
                Err(_) => unknown(&shared, text.as_str()),
            },
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(WsError::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => return Ok(()),
            Err(e) => return Err(e),
        }
        push(&mut ws, &shared, &mut seen)?;
    }
}

/// WebSocket 연결을 수락해 연결마다 스레드를 생성한다(반환하지 않음).
/// `layout_store`는 배치를 읽고 저장하는 저장소다(저장된 파일이 없으면 기본 배치로 시작한다).
/// `wpa_ctrl`과 `host`는 상태 갱신 스레드에 전달한다(`state.rs`의 `spawn_feeds`).
pub fn run(listener: TcpListener, layout_store: layout::Store, device: Device, wpa_ctrl: PathBuf, host: Option<(SocketAddr, PathBuf)>) {
    let shared = Arc::new(Shared::new(layout_store.load(), layout_store, device));
    spawn_feeds(&shared, wpa_ctrl, host);

    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            thread::sleep(hosts::ACCEPT_RETRY);
            continue;
        };
        let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
        let shared = shared.clone();
        thread::spawn(move || {
            eprintln!("클라이언트 접속: {peer}");
            match serve(stream, shared) {
                Ok(()) => eprintln!("클라이언트 종료: {peer}"),
                Err(e) => eprintln!("클라이언트 연결 끊김: {peer}: {e}"),
            }
        });
    }
}

#[cfg(test)]
mod tests;
