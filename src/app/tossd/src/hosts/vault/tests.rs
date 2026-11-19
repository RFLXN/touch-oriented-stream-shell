use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::thread;

use super::*;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tossd-test-vault-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn keys(id: &str) -> Value {
    json!({ "client_id": id, "client_secret": "s3cret-value", "redirect_uri": "http://localhost" })
}

/// 묻는 중인 요청이 생길 때까지 기다린다
fn wait_ask(v: &Vault) -> Asking {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(a) = v.state().asking {
            return a;
        }
        assert!(Instant::now() < until, "허용 여부를 묻지 않음");
        thread::sleep(Duration::from_millis(10));
    }
}

/// 요청을 다른 스레드에서 보내고, 화면에서 `allow`로 답한 뒤 결과를 돌려준다
fn asked(v: &Arc<Vault>, host: &str, op: &'static str, args: Value, allow: bool) -> Result<Value, String> {
    let (v2, host2) = (v.clone(), host.to_string());
    let t = thread::spawn(move || v2.handle(&host2, "pc", op, SERVICE_DISCORD, &args));
    let a = wait_ask(v);
    assert_eq!((a.host.as_str(), a.service.as_str(), a.title.as_str()), (host, SERVICE_DISCORD, "Discord"));
    v.answer(allow);
    t.join().unwrap()
}

/// 받은 폼 본문을 기록하고, 정해 둔 응답을 차례로 돌려주는 가짜 토큰 주소
struct FakeTokens {
    url: String,
    forms: Arc<Mutex<Vec<String>>>,
    heads: Arc<Mutex<Vec<String>>>,
}

fn fake_tokens(replies: Vec<(u16, Value)>) -> FakeTokens {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api/oauth2/token", listener.local_addr().unwrap());
    let forms = Arc::new(Mutex::new(Vec::new()));
    let heads = Arc::new(Mutex::new(Vec::new()));
    let (seen, seen_heads) = (forms.clone(), heads.clone());
    thread::spawn(move || {
        for (status, body) in replies {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            let head = loop {
                let n = s.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i;
                }
            };
            let text = String::from_utf8_lossy(&buf[..head]).to_ascii_lowercase();
            let len: usize = text.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
            while buf.len() < head + 4 + len {
                let n = s.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
            }
            seen.lock().unwrap().push(String::from_utf8_lossy(&buf[head + 4..head + 4 + len]).into_owned());
            seen_heads.lock().unwrap().push(String::from_utf8_lossy(&buf[..head]).into_owned());
            let body = body.to_string();
            let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    FakeTokens { url, forms, heads }
}

#[test]
fn the_book_is_sealed_on_disk_and_comes_back_with_the_same_key() {
    let dir = dir("sealed");
    let v = Arc::new(Vault::open(&dir, |_| {}));
    asked(&v, "host-a", VAULT_PUT, keys("123456"), true).unwrap();
    let text = fs::read_to_string(dir.join("vault.json")).unwrap();
    assert!(!text.contains("s3cret-value") && !text.contains("123456") && !text.contains("host-a"), "평문이 파일에 남음");
    for f in ["vault.json", "vault.key"] {
        assert_eq!(fs::metadata(dir.join(f)).unwrap().permissions().mode() & 0o777, 0o600, "{f} 권한");
    }
    let again = Vault::open(&dir, |_| {});
    assert_eq!(again.status_for("host-a")[0], VaultService { id: SERVICE_DISCORD.into(), title: "Discord".into(), registered: true, linked: false, allowed: true });
    assert_eq!(again.handle("host-a", "pc", VAULT_BEGIN, SERVICE_DISCORD, &Value::Null).unwrap(), json!({ "client_id": "123456", "scopes": ["rpc", "rpc.voice.read", "rpc.voice.write"], "redirect_uri": "http://localhost" }));

    fs::write(dir.join("vault.key"), hex(&[7u8; 32])).unwrap();
    let other = Vault::open(&dir, |_| {});
    assert!(!other.status_for("host-a")[0].registered);
    assert!(dir.join("vault.json.bad").exists(), "다른 키로 열 수 없는 금고를 백업하지 않음");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn only_hosts_the_deck_allowed_get_in_and_a_refusal_is_not_asked_again_soon() {
    let dir = dir("permit");
    let v = Arc::new(Vault::open(&dir, |_| {}));
    assert_eq!(asked(&v, "host-b", VAULT_PUT, keys("1"), false), Err(VAULT_ERR_DENIED.into()));
    assert_eq!(v.handle("host-b", "pc", VAULT_PUT, SERVICE_DISCORD, &keys("1")), Err(VAULT_ERR_DENIED.into()));
    assert!(v.state().asking.is_none() && !v.state().services[0].registered);

    asked(&v, "host-a", VAULT_PUT, keys("1"), true).unwrap();
    assert_eq!(v.handle("host-a", "pc", VAULT_BEGIN, SERVICE_DISCORD, &Value::Null).unwrap()["client_id"], "1");
    assert_eq!(v.state().services[0].hosts, vec!["host-a".to_string()]);

    let (v2, t) = (v.clone(), thread::spawn({
        let v = v.clone();
        move || v.handle("host-c", "pc", VAULT_BEGIN, SERVICE_DISCORD, &Value::Null)
    }));
    wait_ask(&v2);
    assert_eq!(v2.handle("host-d", "pc", VAULT_BEGIN, SERVICE_DISCORD, &Value::Null), Err(VAULT_ERR_BUSY.into()));
    v2.cancel_asks("host-c");
    assert_eq!(t.join().unwrap(), Err(VAULT_ERR_DENIED.into()));

    v.revoke(SERVICE_DISCORD, "host-a");
    assert!(!v.status_for("host-a")[0].allowed);
    asked(&v, "host-a", VAULT_BEGIN, Value::Null, true).unwrap();
    v.forget_host("host-a");
    assert!(v.state().services[0].hosts.is_empty());

    assert_eq!(v.handle("host-a", "pc", VAULT_BEGIN, "slack", &Value::Null), Err(VAULT_ERR_UNKNOWN.into()));
    assert_eq!(v.handle("host-a", "pc", "read", SERVICE_DISCORD, &Value::Null), Err(VAULT_ERR_UNKNOWN.into()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn keys_that_do_not_look_right_are_refused_before_asking() {
    let dir = dir("check");
    let v = Vault::open(&dir, |_| {});
    for bad in [
        json!({ "client_id": "abc", "client_secret": "x" }),
        json!({ "client_id": "1", "client_secret": "" }),
        json!({ "client_id": "1", "client_secret": "has space" }),
        json!({ "client_id": "1", "client_secret": "x", "redirect_uri": "file:///etc" }),
    ] {
        assert!(v.handle("host-a", "pc", VAULT_PUT, SERVICE_DISCORD, &bad).is_err(), "{bad}");
    }
    assert!(v.state().asking.is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tokens_are_issued_reused_refreshed_and_dropped_when_the_service_says_no() {
    let dir = dir("tokens");
    let fake = fake_tokens(vec![
        (200, json!({ "access_token": "A1", "token_type": "Bearer", "expires_in": 604800, "refresh_token": "R1", "scope": "rpc rpc.voice.read rpc.voice.write" })),
        (200, json!({ "access_token": "A2", "expires_in": 10, "refresh_token": "R2", "scope": "rpc" })),
        (200, json!({ "access_token": "A3", "expires_in": 604800, "refresh_token": "R3", "scope": "rpc" })),
        (400, json!({ "error": "invalid_grant", "error_description": "Invalid refresh token" })),
    ]);
    let v = Arc::new(Vault::open_with(&dir, Some(fake.url.clone()), |_| {}));
    asked(&v, "host-a", VAULT_PUT, keys("42"), true).unwrap();
    let token = |args: Value| v.handle("host-a", "pc", VAULT_TOKEN, SERVICE_DISCORD, &args);

    assert_eq!(token(Value::Null), Err(VAULT_ERR_NOT_LINKED.into()));
    let got = token(json!({ "code": "C0" })).unwrap();
    assert_eq!((got["access_token"].as_str(), got["scope"].as_str()), (Some("A1"), Some("rpc rpc.voice.read rpc.voice.write")));
    assert!(got["expires_at"].as_u64().unwrap() > now_secs() + 600000);
    assert!(got.get("refresh_token").is_none() && !got.to_string().contains("s3cret"), "갱신 토큰이나 비밀 값이 나감");
    assert!(v.status_for("host-a")[0].linked);
    assert_eq!(token(Value::Null).unwrap()["access_token"], "A1");

    let form = |i: usize| fake.forms.lock().unwrap()[i].clone();
    assert_eq!(fake.forms.lock().unwrap().len(), 1, "남은 토큰이 있는데 다시 요청함");
    for part in ["client_id=42", "client_secret=s3cret-value", "grant_type=authorization_code", "code=C0", "redirect_uri=http%3A%2F%2Flocalhost"] {
        assert!(form(0).contains(part), "{part} 없음: {}", form(0));
    }

    v.update(service(SERVICE_DISCORD).unwrap(), "42", |k| k.expires_at = now_secs() + 5);
    assert_eq!(token(Value::Null).unwrap()["access_token"], "A2");
    assert!(form(1).contains("grant_type=refresh_token") && form(1).contains("refresh_token=R1"), "{}", form(1));
    assert_eq!(token(Value::Null).unwrap()["access_token"], "A3", "만료가 가까운 토큰을 그대로 빌려줌");
    assert!(form(2).contains("refresh_token=R2"), "바뀐 갱신 토큰을 쓰지 않음: {}", form(2));

    v.update(service(SERVICE_DISCORD).unwrap(), "42", |k| k.expires_at = 0);
    assert_eq!(token(Value::Null), Err(VAULT_ERR_NOT_LINKED.into()));
    assert!(!v.status_for("host-a")[0].linked);
    assert!(v.status_for("host-a")[0].registered, "갱신이 거절됐다고 앱 키까지 지움");

    asked(&v, "host-z", VAULT_PUT, keys("43"), true).unwrap();
    assert_eq!(v.handle("host-z", "pc", VAULT_BEGIN, SERVICE_DISCORD, &Value::Null).unwrap()["client_id"], "43");
    v.handle("host-a", "pc", VAULT_FORGET, SERVICE_DISCORD, &Value::Null).unwrap();
    assert_eq!(token(Value::Null), Err(VAULT_ERR_NOT_SET.into()));
    let _ = fs::remove_dir_all(&dir);
}

/// Spotify: 앱 키는 Basic 인증 헤더로 보내고, PC에는 토큰을 빌려주지 않으며(승인 코드만 받는다), 덱은 `own_token`으로 쓴다
#[test]
fn spotify_tokens_stay_on_the_deck() {
    let dir = dir("spotify");
    let fake = fake_tokens(vec![
        (200, json!({ "access_token": format!("SA1{}", "x".repeat(400)), "token_type": "Bearer", "expires_in": 3600, "refresh_token": "SR1", "scope": "user-read-playback-state" })),
        (200, json!({ "access_token": "SA2", "token_type": "Bearer", "expires_in": 3600, "scope": "user-read-playback-state" })),
    ]);
    let v = Arc::new(Vault::open_with(&dir, Some(fake.url.clone()), |_| {}));
    let id = "0123456789abcdef0123456789abcdef";
    let keys = json!({ "client_id": id, "client_secret": "sp-secret", "redirect_uri": "http://127.0.0.1:7880/spotify/callback" });
    assert!(v.handle("host-a", "pc", VAULT_PUT, SERVICE_SPOTIFY, &json!({ "client_id": "123", "client_secret": "x" })).unwrap_err().contains("32"));
    let put = {
        let v = v.clone();
        thread::spawn(move || v.handle("host-a", "pc", VAULT_PUT, SERVICE_SPOTIFY, &keys))
    };
    wait_ask(&v);
    v.answer(true);
    put.join().unwrap().unwrap();
    let begin = v.handle("host-a", "pc", VAULT_BEGIN, SERVICE_SPOTIFY, &Value::Null).unwrap();
    assert_eq!((begin["client_id"].as_str(), begin["redirect_uri"].as_str()), (Some(id), Some("http://127.0.0.1:7880/spotify/callback")));
    assert_eq!(v.own_token(SERVICE_SPOTIFY, false), Err(VAULT_ERR_NOT_LINKED.into()));
    assert!(v.handle("host-a", "pc", VAULT_TOKEN, SERVICE_SPOTIFY, &Value::Null).is_err(), "PC에 Spotify 토큰을 빌려줌");

    let long_code = format!("SC0{}", "-_aZ9".repeat(80));
    assert!(v.handle("host-a", "pc", VAULT_TOKEN, SERVICE_SPOTIFY, &json!({ "code": "has space" })).is_err());
    let linked = v.handle("host-a", "pc", VAULT_TOKEN, SERVICE_SPOTIFY, &json!({ "code": long_code })).unwrap();
    assert_eq!(linked, json!({ "linked": true }), "승인 코드를 넘긴 PC에 토큰이 나감");
    assert!(v.linked(SERVICE_SPOTIFY));
    let head = fake.heads.lock().unwrap()[0].to_ascii_lowercase();
    assert!(head.contains("authorization: basic mde"), "Basic 인증 헤더가 없음: {head}");
    let form = fake.forms.lock().unwrap()[0].clone();
    assert!(!form.contains("client_secret") && form.contains(&format!("code={long_code}")) && form.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A7880%2Fspotify%2Fcallback"), "{form}");

    assert_eq!(v.own_token(SERVICE_SPOTIFY, false).unwrap(), format!("SA1{}", "x".repeat(400)), "긴 토큰을 받지 못함");
    assert_eq!(fake.forms.lock().unwrap().len(), 1, "남은 토큰이 있는데 다시 요청함");
    assert_eq!(v.own_token(SERVICE_SPOTIFY, true).unwrap(), "SA2", "거부된 토큰을 버리지 않음");
    assert!(fake.forms.lock().unwrap()[1].contains("refresh_token=SR1"));
    assert!(v.linked(SERVICE_SPOTIFY), "갱신 응답에 refresh 토큰이 없다고 승인이 사라짐");
    let _ = fs::remove_dir_all(&dir);
}
