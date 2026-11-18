//! 덱 금고 이용: 웹 콘솔과 CLI의 금고 요청을 지금 연결된 덱 세션으로 보내고 응답을 기다린다(설계는 `docs/04-host-link.md` 4.15절).
//!
//! 서비스 키는 덱으로 보내기만 하고 이 PC에 저장하지 않는다. 덱에서 빌린 토큰도 메모리에만 둔다.
//! 이 PC가 처음 금고를 쓰려 하면 덱이 화면에서 허용 여부를 묻는다. 그동안(최대 `VAULT_ASK_WAIT`) 응답을 기다린다.
//! 요청은 실행 중인 데몬의 세션(`session.rs`)이 덱에 전달한다. 연결된 덱이 없거나 금고를 모르는 덱이면 바로 실패한다.

use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use toss_proto::{VaultService, VAULT_ASK_WAIT, VAULT_ERR_BUSY, VAULT_ERR_DENIED, VAULT_ERR_NOT_LINKED, VAULT_ERR_NOT_SET, VAULT_ERR_TIMEOUT, VAULT_ERR_UNKNOWN};

/// 세션에 전달하는 금고 요청 1건
pub struct Call {
    pub op: String,
    pub service: String,
    pub args: Value,
    pub reply: Sender<Result<Value, String>>,
}

type Line = Box<dyn Fn(Call) -> bool + Send>;

/// 지금 연결된 덱 세션으로 요청을 넘기는 통로(세션이 열려 있는 동안만 있다)
static LINE: Mutex<Option<Line>> = Mutex::new(None);
/// 연결된 덱이 마지막으로 알려 준 금고 상태: (덱 키, 서비스 목록)
static STATE: Mutex<Option<(String, Vec<VaultService>)>> = Mutex::new(None);

/// 세션이 시작될 때 요청 통로를 연다
pub fn attach(line: Line) {
    *LINE.lock().unwrap() = Some(line);
}

/// 세션이 끝날 때 통로를 닫고 상태를 지운다
pub fn detach() {
    *LINE.lock().unwrap() = None;
    *STATE.lock().unwrap() = None;
}

/// 덱이 보낸 금고 상태를 기록한다
pub fn set_state(deck: &str, services: Vec<VaultService>) {
    *STATE.lock().unwrap() = Some((deck.to_string(), services));
}

/// 연결된 덱의 금고 상태(덱이 없거나 금고를 모르는 덱이면 `None`)
pub fn state() -> Option<(String, Vec<VaultService>)> {
    STATE.lock().unwrap().clone()
}

/// 덱이 돌려준 오류 코드를 사용자에게 보여 줄 문장으로 바꾼다
pub fn explain(error: &str) -> String {
    match error {
        VAULT_ERR_DENIED => "덱에서 이 PC의 금고 사용을 거절했습니다 (5분 동안 다시 묻지 않습니다)".into(),
        VAULT_ERR_BUSY => "덱이 다른 요청의 허용 여부를 묻는 중입니다. 잠시 후 다시 시도하세요".into(),
        VAULT_ERR_TIMEOUT => "덱 화면에서 허용 여부에 답하지 않았습니다".into(),
        VAULT_ERR_NOT_SET => "덱에 이 서비스의 키가 없습니다. 먼저 키를 저장하세요".into(),
        VAULT_ERR_NOT_LINKED => "아직 연결(승인)하지 않았거나 연결이 만료되었습니다. 다시 연결하세요".into(),
        VAULT_ERR_UNKNOWN => "덱이 이 요청을 처리할 수 없습니다".into(),
        other => other.to_string(),
    }
}

/// 연결된 덱에 금고 요청을 보내고 응답을 기다린다. 실패하면 사용자에게 보여 줄 문장을 돌려준다.
pub fn ask(op: &str, service: &str, args: Value) -> Result<Value, String> {
    if state().is_none() {
        return Err(if LINE.lock().unwrap().is_some() { "연결된 덱이 금고를 지원하지 않습니다 (덱 이미지를 새로 플래싱하세요)" } else { "덱이 연결되어 있지 않습니다 (실행 중인 toss-connector가 덱에 연결되어 있어야 합니다)" }.into());
    }
    let (tx, rx) = mpsc::channel();
    let call = Call { op: op.into(), service: service.into(), args, reply: tx };
    let sent = LINE.lock().unwrap().as_ref().is_some_and(|line| line(call));
    if !sent {
        return Err("덱이 연결되어 있지 않습니다".into());
    }
    match rx.recv_timeout(VAULT_ASK_WAIT + Duration::from_secs(30)) {
        Ok(reply) => reply.map_err(|e| explain(&e)),
        Err(_) => Err("덱이 응답하지 않습니다".into()),
    }
}
