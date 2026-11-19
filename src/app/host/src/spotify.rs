//! Spotify 연결(승인) 돕기: 덱이 Spotify 계정을 쓸 수 있도록 이 PC의 브라우저로 한 번 로그인시킨다.
//! Spotify 조회와 재생 제어는 덱이 직접 한다(덱의 `tossd/src/spotify.rs`). 이 PC는 토큰을 받지도 보관하지도 않는다.
//!
//! 절차
//!   1. 웹 콘솔(또는 `toss-connector spotify link`)이 `start`를 부른다. 덱 금고에서 공개 값(client_id, 권한 범위, redirect_uri)을 받고
//!      Spotify 승인 주소를 만든다. 위조된 콜백을 막는 일회용 `state`(무작위 32바이트)를 만들어 `LOGIN_WAIT` 동안 기억한다.
//!   2. 사용자가 브라우저에서 Spotify에 로그인하고 승인하면 Spotify가 `http://127.0.0.1:<콘솔 포트>/spotify/callback`으로 돌려보낸다.
//!   3. `callback`이 `state`를 확인하고(맞지 않거나 이미 쓴 것이면 거절) 승인 코드를 덱에 넘긴다. 덱이 그 코드를 토큰으로 바꿔 보관한다.
//!
//! Spotify는 Redirect URI에 `localhost`를 허용하지 않으므로 `127.0.0.1`을 쓴다. 덱에 저장한 redirect_uri가 이 콘솔의 콜백 주소와
//! 다르면 시작하지 않는다(브라우저가 돌아올 곳이 없기 때문). 콜백은 브라우저가 여는 주소라 콘솔 토큰을 요구할 수 없다.
//! 대신 `state`가 일회용 비밀 값 역할을 하며, 콘솔의 Host 검사는 그대로 적용된다.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use toss_proto::{SERVICE_SPOTIFY, VAULT_BEGIN, VAULT_TOKEN};

use crate::vault;

pub const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
pub const CALLBACK_PATH: &str = "/spotify/callback";
/// 브라우저에서 승인하기를 기다리는 시간
const LOGIN_WAIT: Duration = Duration::from_secs(600);

/// 진행 중인 로그인과 그 결과(웹 콘솔 표시용)
#[derive(Default)]
pub struct Login {
    state: String,
    until: Option<Instant>,
    /// idle / browser(브라우저 승인 대기) / deck(덱이 토큰을 발급받는 중) / done / failed
    phase: &'static str,
    error: String,
}

impl Login {
    pub fn shown(&self) -> Value {
        let waiting = self.phase == "browser" && self.until.is_some_and(|u| Instant::now() > u);
        json!({
            "phase": if waiting { "failed" } else if self.phase.is_empty() { "idle" } else { self.phase },
            "error": if waiting { "브라우저에서 승인하지 않아 시간이 지났습니다. 다시 연결하세요" } else { self.error.as_str() },
        })
    }

    /// 결과 표시를 지운다(진행 중이면 그대로 둔다)
    pub fn clear(&mut self) {
        if self.phase != "deck" {
            *self = Login::default();
        }
    }
}

/// 이 콘솔의 콜백 주소
pub fn callback_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

/// 로그인을 시작하고 브라우저로 열 Spotify 승인 주소를 돌려준다
pub fn start(login: &Mutex<Login>, port: u16) -> Result<String, String> {
    if login.lock().unwrap().phase == "deck" {
        return Err("덱이 토큰을 발급받는 중입니다. 잠시 후 다시 시도하세요".into());
    }
    let v = vault::ask(VAULT_BEGIN, SERVICE_SPOTIFY, Value::Null)?;
    let client_id = v["client_id"].as_str().unwrap_or_default();
    let redirect = v["redirect_uri"].as_str().unwrap_or_default();
    let scopes: Vec<&str> = v["scopes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let mine = callback_uri(port);
    if redirect != mine {
        return Err(format!("덱에 저장한 Redirect URI가 이 콘솔의 주소와 다릅니다. {mine} 으로 다시 저장하고, Spotify 앱 설정의 Redirect URIs에도 같은 주소를 등록하세요"));
    }
    let state = toss_proto::secure::Identity::generate().map_err(|e| e.to_string())?.id();
    let url = authorize_url(client_id, &scopes, redirect, &state);
    *login.lock().unwrap() = Login { state, until: Some(Instant::now() + LOGIN_WAIT), phase: "browser", error: String::new() };
    Ok(url)
}

/// Spotify 승인 주소
pub fn authorize_url(client_id: &str, scopes: &[&str], redirect: &str, state: &str) -> String {
    format!("{AUTHORIZE_URL}?response_type=code&client_id={}&scope={}&redirect_uri={}&state={}", encode(client_id), encode(&scopes.join(" ")), encode(redirect), encode(state))
}

/// 브라우저가 돌아온 콜백을 처리하고, 보여 줄 (상태 코드, 문장)을 돌려준다. 덱이 토큰을 발급받을 때까지 블로킹된다.
pub fn callback(login: &Mutex<Login>, query: &str) -> (u16, String) {
    let q = parse_query(query);
    let given = q.get("state").map(String::as_str).unwrap_or_default();
    {
        let mut l = login.lock().unwrap();
        let fresh = l.phase == "browser" && l.until.is_some_and(|u| Instant::now() <= u);
        if !fresh || given.is_empty() || !same(given, &l.state) {
            return (400, "이 요청은 진행 중인 Spotify 연결과 맞지 않습니다. 웹 콘솔에서 다시 연결하세요.".into());
        }
        l.state.clear();
        l.until = None;
        if let Some(err) = q.get("error") {
            l.phase = "failed";
            l.error = if err == "access_denied" { "Spotify 승인을 거절했습니다".into() } else { format!("Spotify가 승인을 돌려주지 않았습니다: {err}") };
            return (400, l.error.clone());
        }
        l.phase = "deck";
    }
    let code = q.get("code").cloned().unwrap_or_default();
    let got = if code.is_empty() { Err("Spotify가 승인 코드를 주지 않았습니다".to_string()) } else { vault::ask(VAULT_TOKEN, SERVICE_SPOTIFY, json!({ "code": code })) };
    let mut l = login.lock().unwrap();
    match got {
        Ok(_) => {
            l.phase = "done";
            (200, "Spotify 연결을 마쳤습니다. 덱이 이 계정의 재생 정보를 직접 가져옵니다. 이 창은 닫아도 됩니다.".into())
        }
        Err(e) => {
            l.phase = "failed";
            l.error = e.clone();
            (502, format!("덱이 토큰을 발급받지 못했습니다: {e}"))
        }
    }
}

/// 결과 문장을 담은 작은 HTML 페이지(스크립트 없음)
pub fn page(text: &str) -> String {
    let safe = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!("<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\"><title>Spotify 연결 - toss-connector</title><body><p>{safe}</p></body></html>")
}

/// 길이가 같을 때의 상수 시간 비교
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// URL 쿼리 값 인코딩(영문자, 숫자, `-._~`만 그대로 둔다)
fn encode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() && hex(bytes[i + 1]).is_some() && hex(bytes[i + 2]).is_some() => {
                out.push(hex(bytes[i + 1]).unwrap_or(0) * 16 + hex(bytes[i + 2]).unwrap_or(0));
                i += 2;
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query.split('&').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).map(|(k, v)| (decode(k), decode(v))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorize_url_is_encoded_and_queries_are_decoded() {
        let url = authorize_url("abc123", &["user-read-playback-state", "user-modify-playback-state"], "http://127.0.0.1:7880/spotify/callback", "s1");
        assert_eq!(
            url,
            "https://accounts.spotify.com/authorize?response_type=code&client_id=abc123&scope=user-read-playback-state%20user-modify-playback-state&redirect_uri=http%3A%2F%2F127.0.0.1%3A7880%2Fspotify%2Fcallback&state=s1"
        );
        let q = parse_query("code=AQ%2Bx-y_z&state=s1&error=&x=a+b&bad=%zz%4");
        assert_eq!((q["code"].as_str(), q["state"].as_str(), q["error"].as_str(), q["x"].as_str(), q["bad"].as_str()), ("AQ+x-y_z", "s1", "", "a b", "%zz%4"));
    }

    #[test]
    fn a_callback_needs_the_live_one_time_state() {
        let login = Mutex::new(Login { state: "good".into(), until: Some(Instant::now() + LOGIN_WAIT), phase: "browser", error: String::new() });
        assert_eq!(callback(&login, "code=c&state=bad").0, 400);
        assert_eq!(callback(&login, "code=c").0, 400);
        assert_eq!(login.lock().unwrap().phase, "browser", "틀린 state가 진행 중인 로그인을 망침");
        let (status, text) = callback(&login, "error=access_denied&state=good");
        assert_eq!((status, login.lock().unwrap().phase), (400, "failed"));
        assert!(text.contains("거절"));
        assert_eq!(callback(&login, "code=c&state=good").0, 400, "한 번 쓴 state를 다시 받음");

        let late = Mutex::new(Login { state: "good".into(), until: Some(Instant::now() - Duration::from_secs(1)), phase: "browser", error: String::new() });
        assert_eq!(callback(&late, "code=c&state=good").0, 400);
        assert_eq!(late.lock().unwrap().shown()["phase"], "failed");

        let ok = Mutex::new(Login { state: "good".into(), until: Some(Instant::now() + LOGIN_WAIT), phase: "browser", error: String::new() });
        let (status, _) = callback(&ok, "code=c&state=good");
        assert_eq!((status, ok.lock().unwrap().phase), (502, "failed"), "덱이 없는데 성공으로 표시함");
        assert!(page("<b>&").contains("&lt;b&gt;&amp;"));
    }
}
