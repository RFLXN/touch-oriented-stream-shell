//! 덱 금고: 외부 연동(Discord, Spotify)의 키를 덱에 보관하고, 페어링된 PC에는 그 키로 발급한 짧은 수명의 토큰만 빌려준다.
//! 덱이 직접 쓰는 서비스(Spotify)는 PC에 토큰을 빌려주지 않는다(`Service::lend`). PC는 키를 저장하고 승인 코드를 넘기기만 하고,
//! 덱의 모듈이 `own_token`으로 토큰을 얻는다.
//! 요청 형식은 `toss-proto`의 `HostMsg::Vault`, 전체 설계는 `docs/04-host-link.md` 4.15절 참고.
//!
//! 서비스마다 보관하는 값
//!   client_id, client_secret, redirect_uri   PC에서 입력해 덱으로 보낸 앱 키(`put`). 덱은 이 값을 PC에 돌려주지 않는다
//!                                            (공개 값인 client_id만 `begin`으로 알려 준다)
//!   refresh_token                            승인을 한 번 받으면 생긴다. 덱이 토큰을 갱신할 때만 쓰며 PC에 보내지 않는다
//!   access_token, expires_at, scope          PC에 빌려주는 토큰과 그 만료 시각(유닉스 초), 허용 범위. 만료가 가까우면 덱이 갱신한다
//!
//! 토큰 발급과 갱신은 덱이 서비스의 토큰 주소로 직접 HTTPS 요청을 보낸다(덱이 인터넷에 연결되어 있어야 한다).
//! PC는 서비스 앱에서 받은 승인 코드를 덱에 넘기기만 하고(`token`의 code), 덱이 그 코드를 토큰으로 바꾼다.
//! 토큰 요청은 한 번에 하나씩 처리한다(갱신할 때마다 refresh 토큰이 바뀔 수 있어, 동시에 갱신하면 하나가 실패하기 때문).
//!
//! 금고를 쓸 수 있는 PC는 덱 사용자가 정한다. 허용하지 않은 PC가 요청하면 화면에 확인 다이얼로그를 띄우고(`VaultState::asking`),
//! 사용자가 답할 때까지(최대 `VAULT_ASK_WAIT`) 응답을 미룬다. 한 번에 하나만 묻고, 묻는 중에 온 다른 요청은 busy로 거절한다.
//! 거절하면 같은 PC의 같은 서비스 요청은 `DENY_COOLDOWN` 동안 묻지 않고 거절한다(다이얼로그를 반복해서 띄우지 못하게).
//! 허용은 화면의 설정(INTEGRATIONS)에서 PC별로 해제할 수 있고, 페어링을 삭제하면 함께 사라진다.
//!
//! 저장: `vault.json`에 ChaCha20-Poly1305로 암호화해 두고, 키는 `vault.key`에 둔다(둘 다 소유자 전용 0600).
//! 같은 기기에 키를 두므로 파일 사본(백업 이미지 등)이 새어 나갔을 때만 보호된다. 기기의 root 권한을 얻으면 읽을 수 있다.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use toss_proto::{
    VaultService, SERVICE_DISCORD, SERVICE_SPOTIFY, VAULT_ASK_WAIT, VAULT_BEGIN, VAULT_ERR_BUSY, VAULT_ERR_DENIED, VAULT_ERR_NOT_LINKED, VAULT_ERR_NOT_SET, VAULT_ERR_TIMEOUT, VAULT_ERR_UNKNOWN, VAULT_FORGET, VAULT_PUT,
    VAULT_TOKEN,
};

use crate::persist;

/// 거절한 PC의 같은 요청을 묻지 않고 거절하는 시간
const DENY_COOLDOWN: Duration = Duration::from_secs(300);
/// 토큰 주소 요청의 연결·응답 제한 시간
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// 암호문에 함께 묶는 값(다른 용도의 암호문과 섞이지 않게)
const AAD: &[u8] = b"toss-vault-1";
const SECRET_MAX: usize = 256;

/// 금고가 다루는 서비스: 토큰 주소, 요청할 권한 범위와 서비스마다 다른 규칙
pub struct Service {
    pub id: &'static str,
    pub title: &'static str,
    token_url: &'static str,
    scopes: &'static [&'static str],
    /// 보관한 토큰의 남은 시간이 이보다 짧으면 쓰기 전에 갱신한다(서비스마다 토큰 수명이 다르다)
    refresh_before: Duration,
    /// 토큰 주소에 앱 키를 HTTP Basic 인증으로 보낸다(아니면 폼 본문에 넣는다)
    basic_auth: bool,
    /// PC에 토큰을 빌려준다. 아니면 덱만 쓴다(PC의 `token` 요청은 승인 코드를 넘길 때만 받고, 토큰은 돌려주지 않는다)
    lend: bool,
    /// client_id 형식 검사와 그 설명
    client_id_ok: fn(&str) -> bool,
    client_id_about: &'static str,
}

pub const SERVICES: &[Service] = &[
    Service {
        id: SERVICE_DISCORD,
        title: "Discord",
        token_url: "https://discord.com/api/oauth2/token",
        scopes: &["rpc", "rpc.voice.read", "rpc.voice.write"],
        refresh_before: Duration::from_secs(3600),
        basic_auth: false,
        lend: true,
        client_id_ok: |id| id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit()),
        client_id_about: "client_id는 숫자로 된 애플리케이션 ID여야 합니다",
    },
    Service {
        id: SERVICE_SPOTIFY,
        title: "Spotify",
        token_url: "https://accounts.spotify.com/api/token",
        scopes: &["user-read-playback-state", "user-read-currently-playing", "user-modify-playback-state"],
        refresh_before: Duration::from_secs(300),
        basic_auth: true,
        lend: false,
        client_id_ok: |id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        client_id_about: "client_id는 32자리 16진수여야 합니다(Spotify 개발자 대시보드의 Client ID)",
    },
];

fn service(id: &str) -> Option<&'static Service> {
    SERVICES.iter().find(|s| s.id == id)
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
struct Keys {
    client_id: String,
    client_secret: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_at: u64,
    #[serde(default)]
    scope: String,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct Book {
    #[serde(default)]
    services: BTreeMap<String, Keys>,
    /// 서비스별로 금고를 쓸 수 있는 호스트 키
    #[serde(default)]
    grants: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    v: u32,
    nonce: String,
    data: String,
}

/// UI로 전달하는 금고 상태(키 값은 포함하지 않는다)
#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct VaultState {
    pub services: Vec<ServiceState>,
    /// 화면에서 허용 여부를 묻는 중인 요청(없으면 `null`)
    pub asking: Option<Asking>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct ServiceState {
    pub id: String,
    pub title: String,
    pub registered: bool,
    pub linked: bool,
    /// 금고를 쓸 수 있도록 허용한 호스트 키
    pub hosts: Vec<String>,
}

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct Asking {
    pub host: String,
    pub name: String,
    pub service: String,
    pub title: String,
}

/// 토큰 주소 요청 실패: 서비스가 키나 refresh 토큰을 받아 주지 않음 / 그 밖의 실패(연결 불가 등)
enum Fail {
    Rejected(String),
    Other(String),
}

pub struct Vault {
    path: PathBuf,
    key: Option<[u8; 32]>,
    book: Mutex<Book>,
    asking: Mutex<Option<(Asking, Sender<bool>)>>,
    denied: Mutex<HashMap<(String, String), Instant>>,
    tokens: Mutex<()>,
    saving: Mutex<()>,
    token_url: Option<String>,
    publish: Box<dyn Fn(VaultState) + Send + Sync>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let data = ChaCha20Poly1305::new(Key::from_slice(key)).encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: AAD }).map_err(|_| "암호화 실패".to_string())?;
    serde_json::to_vec_pretty(&Sealed { v: 1, nonce: hex(&nonce), data: hex(&data) }).map_err(|e| e.to_string())
}

fn unseal(key: &[u8; 32], bytes: &[u8]) -> Result<Vec<u8>, String> {
    let sealed: Sealed = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let nonce = unhex(&sealed.nonce).filter(|n| n.len() == 12).ok_or("nonce 형식 오류")?;
    let data = unhex(&sealed.data).ok_or("암호문 형식 오류")?;
    ChaCha20Poly1305::new(Key::from_slice(key)).decrypt(Nonce::from_slice(&nonce), Payload { msg: &data, aad: AAD }).map_err(|_| "복호화 실패(키가 다르거나 파일이 손상됨)".to_string())
}

/// 금고 키를 읽는다(없으면 만든다). 읽을 수 없는 키 파일은 백업해 두고 새로 만든다. 저장할 수 없으면 `None`
fn load_key(path: &Path) -> Option<[u8; 32]> {
    match fs::read_to_string(path) {
        Ok(text) => match unhex(&text).and_then(|b| <[u8; 32]>::try_from(b).ok()) {
            Some(key) => return Some(key),
            None => persist::set_aside(path, "vault", "키 형식 오류"),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => persist::set_aside(path, "vault", &e.to_string()),
    }
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).ok()?;
    match persist::write_private(path, hex(&key).as_bytes()) {
        Ok(()) => Some(key),
        Err(e) => {
            eprintln!("vault: {}에 금고 키를 저장할 수 없습니다({e}). 이번 실행 동안만 메모리에 보관합니다", path.display());
            None
        }
    }
}

fn load_book(path: &Path, key: Option<&[u8; 32]>) -> Book {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Book::default(),
        Err(e) => {
            persist::set_aside(path, "vault", &e.to_string());
            return Book::default();
        }
    };
    let Some(key) = key else { return Book::default() };
    match unseal(key, &bytes).and_then(|plain| serde_json::from_slice::<Book>(&plain).map_err(|e| e.to_string())) {
        Ok(book) => book,
        Err(why) => {
            persist::set_aside(path, "vault", &why);
            Book::default()
        }
    }
}

fn field<'a>(args: &'a Value, name: &str) -> &'a str {
    args[name].as_str().map(str::trim).unwrap_or_default()
}

fn printable(text: &str) -> bool {
    !text.is_empty() && text.len() <= SECRET_MAX && text.bytes().all(|b| b.is_ascii_graphic())
}

/// `put`으로 받은 키 필드 검증
fn checked_keys(svc: &Service, args: &Value) -> Result<(String, String, String), String> {
    let (id, secret, redirect) = (field(args, "client_id"), field(args, "client_secret"), field(args, "redirect_uri"));
    if id.is_empty() || !(svc.client_id_ok)(id) {
        return Err(svc.client_id_about.into());
    }
    if !printable(secret) {
        return Err("client_secret이 비어 있거나 허용되지 않는 문자가 있습니다".into());
    }
    if !redirect.is_empty() && !(printable(redirect) && (redirect.starts_with("http://") || redirect.starts_with("https://"))) {
        return Err("redirect_uri는 http:// 또는 https://로 시작하는 주소여야 합니다".into());
    }
    Ok((id.to_string(), secret.to_string(), redirect.to_string()))
}

/// 토큰 주소에 폼을 보내고 JSON 응답을 받는다. `basic`이 있으면 앱 키를 HTTP Basic 인증 헤더로 보낸다.
fn post_form(url: &str, basic: Option<(&str, &str)>, form: &[(&str, &str)]) -> Result<Value, Fail> {
    use base64::Engine;
    let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).user_agent(concat!("tossd/", env!("CARGO_PKG_VERSION"))).build();
    let mut req = agent.post(url).set("Accept", "application/json");
    if let Some((id, secret)) = basic {
        req = req.set("Authorization", &format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{id}:{secret}"))));
    }
    match req.send_form(form) {
        Ok(res) => {
            let text = res.into_string().map_err(|e| Fail::Other(format!("응답을 읽을 수 없습니다: {e}")))?;
            serde_json::from_str(&text).map_err(|_| Fail::Other("응답이 JSON이 아닙니다".into()))
        }
        Err(ureq::Error::Status(code, res)) => {
            let body: Value = res.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
            let error = body["error"].as_str().unwrap_or_default().to_string();
            let about = body["error_description"].as_str().unwrap_or_default();
            let why = if error.is_empty() { format!("HTTP {code}") } else if about.is_empty() { error.clone() } else { format!("{error}: {about}") };
            if matches!(error.as_str(), "invalid_grant" | "invalid_client" | "unauthorized_client") {
                Err(Fail::Rejected(why))
            } else {
                Err(Fail::Other(why))
            }
        }
        Err(e) => Err(Fail::Other(format!("토큰 주소에 연결할 수 없습니다(덱의 인터넷 연결 확인): {e}"))),
    }
}

/// 빌려줄 토큰
fn lent(keys: &Keys) -> Value {
    json!({ "access_token": keys.access_token, "expires_at": keys.expires_at, "scope": keys.scope })
}

impl Vault {
    /// `dir`의 금고를 연다. `publish`는 UI 상태가 바뀔 때마다 호출된다.
    pub fn open(dir: &Path, publish: impl Fn(VaultState) + Send + Sync + 'static) -> Self {
        Self::open_with(dir, None, publish)
    }

    /// `open`과 같되 모든 서비스의 토큰 주소를 `token_url`로 바꾼다(테스트용 가짜 서버)
    fn open_with(dir: &Path, token_url: Option<String>, publish: impl Fn(VaultState) + Send + Sync + 'static) -> Self {
        let key = load_key(&dir.join("vault.key"));
        let path = dir.join("vault.json");
        let book = load_book(&path, key.as_ref());
        Self {
            path,
            key,
            book: Mutex::new(book),
            asking: Mutex::new(None),
            denied: Mutex::new(HashMap::new()),
            tokens: Mutex::new(()),
            saving: Mutex::new(()),
            token_url,
            publish: Box::new(publish),
        }
    }

    fn save(&self) {
        let _saving = self.saving.lock().unwrap();
        let Some(key) = &self.key else { return };
        let plain = match serde_json::to_vec(&*self.book.lock().unwrap()) {
            Ok(p) => p,
            Err(e) => return eprintln!("vault: 저장 실패: {e}"),
        };
        if let Err(e) = seal(key, &plain).and_then(|bytes| persist::write_private(&self.path, &bytes).map_err(|e| e.to_string())) {
            eprintln!("vault: {} 저장 실패: {e}", self.path.display());
        }
    }

    pub fn state(&self) -> VaultState {
        let book = self.book.lock().unwrap().clone();
        let services = SERVICES
            .iter()
            .map(|s| {
                let keys = book.services.get(s.id);
                ServiceState {
                    id: s.id.into(),
                    title: s.title.into(),
                    registered: keys.is_some(),
                    linked: keys.is_some_and(|k| !k.refresh_token.is_empty()),
                    hosts: book.grants.get(s.id).map(|g| g.iter().cloned().collect()).unwrap_or_default(),
                }
            })
            .collect();
        VaultState { services, asking: self.asking.lock().unwrap().as_ref().map(|(a, _)| a.clone()) }
    }

    fn announce(&self) {
        (self.publish)(self.state());
    }

    /// 호스트에 보낼 금고 상태(`DeckMsg::VaultState`)
    pub fn status_for(&self, host: &str) -> Vec<VaultService> {
        let book = self.book.lock().unwrap();
        SERVICES
            .iter()
            .map(|s| VaultService {
                id: s.id.into(),
                title: s.title.into(),
                registered: book.services.contains_key(s.id),
                linked: book.services.get(s.id).is_some_and(|k| !k.refresh_token.is_empty()),
                allowed: book.grants.get(s.id).is_some_and(|g| g.contains(host)),
            })
            .collect()
    }

    /// 호스트의 금고 요청 1건을 처리한다(허용 여부를 묻는 동안이나 토큰 주소에 요청하는 동안 블로킹된다).
    pub fn handle(&self, host: &str, name: &str, op: &str, service_id: &str, args: &Value) -> Result<Value, String> {
        let svc = service(service_id).ok_or(VAULT_ERR_UNKNOWN)?;
        if ![VAULT_PUT, VAULT_FORGET, VAULT_BEGIN, VAULT_TOKEN].contains(&op) {
            return Err(VAULT_ERR_UNKNOWN.into());
        }
        let checked = if op == VAULT_PUT { Some(checked_keys(svc, args)?) } else { None };
        let code = field(args, "code");
        if op == VAULT_TOKEN && !code.is_empty() && !printable(code) {
            return Err("승인 코드 형식이 올바르지 않습니다".into());
        }
        if op == VAULT_TOKEN && code.is_empty() && !svc.lend {
            return Err(format!("{} 토큰은 덱만 씁니다(PC에 빌려주지 않음)", svc.title));
        }
        self.permit(host, name, svc)?;
        match op {
            VAULT_PUT => {
                let (client_id, client_secret, redirect_uri) = checked.unwrap_or_default();
                {
                    let mut book = self.book.lock().unwrap();
                    let keys = book.services.entry(svc.id.into()).or_default();
                    if keys.client_id != client_id {
                        *keys = Keys::default();
                    }
                    keys.client_id = client_id;
                    keys.client_secret = client_secret;
                    keys.redirect_uri = redirect_uri;
                }
                self.save();
                self.announce();
                eprintln!("vault: {name}이(가) {} 키를 저장함", svc.title);
                Ok(json!({ "ok": true }))
            }
            VAULT_FORGET => {
                self.forget(svc.id);
                eprintln!("vault: {name}이(가) {} 키를 삭제함", svc.title);
                Ok(json!({ "ok": true }))
            }
            VAULT_BEGIN => {
                let (client_id, redirect_uri) = self.book.lock().unwrap().services.get(svc.id).map(|k| (k.client_id.clone(), k.redirect_uri.clone())).ok_or(VAULT_ERR_NOT_SET)?;
                Ok(json!({ "client_id": client_id, "scopes": svc.scopes, "redirect_uri": redirect_uri }))
            }
            _ => {
                let got = self.token(svc, (!code.is_empty()).then_some(code));
                if !svc.lend {
                    if got.is_ok() {
                        eprintln!("vault: {name}이(가) 넘긴 승인 코드로 {} 토큰을 발급받음", svc.title);
                    }
                    return got.map(|_| json!({ "linked": true }));
                }
                if got.is_ok() {
                    eprintln!("vault: {name}에 {} 토큰을 빌려줌", svc.title);
                }
                got
            }
        }
    }

    /// 호스트가 이 서비스의 금고를 쓸 수 있는지 확인한다. 아직 허용하지 않았으면 화면에서 묻고 답을 기다린다.
    fn permit(&self, host: &str, name: &str, svc: &Service) -> Result<(), String> {
        if self.book.lock().unwrap().grants.get(svc.id).is_some_and(|g| g.contains(host)) {
            return Ok(());
        }
        let key = (host.to_string(), svc.id.to_string());
        if self.denied.lock().unwrap().get(&key).is_some_and(|at| at.elapsed() < DENY_COOLDOWN) {
            return Err(VAULT_ERR_DENIED.into());
        }
        let (tx, rx) = mpsc::channel();
        {
            let mut asking = self.asking.lock().unwrap();
            if asking.is_some() {
                return Err(VAULT_ERR_BUSY.into());
            }
            *asking = Some((Asking { host: host.into(), name: name.into(), service: svc.id.into(), title: svc.title.into() }, tx));
        }
        eprintln!("vault: {name}이(가) {} 사용 허용을 요청함", svc.title);
        self.announce();
        let answer = rx.recv_timeout(VAULT_ASK_WAIT);
        {
            let mut asking = self.asking.lock().unwrap();
            if asking.as_ref().is_some_and(|(a, _)| a.host == host && a.service == svc.id) {
                *asking = None;
            }
        }
        let result = match answer {
            Ok(true) => {
                self.book.lock().unwrap().grants.entry(svc.id.into()).or_default().insert(host.into());
                self.save();
                eprintln!("vault: {name}에 {} 사용을 허용함", svc.title);
                Ok(())
            }
            Ok(false) => {
                self.denied.lock().unwrap().insert(key, Instant::now());
                eprintln!("vault: {name}의 {} 사용 요청을 거절함", svc.title);
                Err(VAULT_ERR_DENIED.into())
            }
            Err(RecvTimeoutError::Timeout) => Err(VAULT_ERR_TIMEOUT.into()),
            Err(RecvTimeoutError::Disconnected) => Err(VAULT_ERR_DENIED.into()),
        };
        self.announce();
        result
    }

    /// 토큰을 빌려준다. `code`가 있으면 새로 발급받고, 없으면 보관한 토큰을 주거나 갱신한다.
    fn token(&self, svc: &Service, code: Option<&str>) -> Result<Value, String> {
        let _one = self.tokens.lock().unwrap();
        let keys = self.book.lock().unwrap().services.get(svc.id).cloned().ok_or(VAULT_ERR_NOT_SET)?;
        let url = self.token_url.as_deref().unwrap_or(svc.token_url);
        let basic = svc.basic_auth.then_some((keys.client_id.as_str(), keys.client_secret.as_str()));
        let mut form = if svc.basic_auth { Vec::new() } else { vec![("client_id", keys.client_id.as_str()), ("client_secret", keys.client_secret.as_str())] };
        match code {
            Some(code) => {
                form.extend([("grant_type", "authorization_code"), ("code", code)]);
                if !keys.redirect_uri.is_empty() {
                    form.push(("redirect_uri", keys.redirect_uri.as_str()));
                }
            }
            None if !keys.access_token.is_empty() && keys.expires_at > now_secs() + svc.refresh_before.as_secs() => return Ok(lent(&keys)),
            None if keys.refresh_token.is_empty() => return Err(VAULT_ERR_NOT_LINKED.into()),
            None => form.extend([("grant_type", "refresh_token"), ("refresh_token", keys.refresh_token.as_str())]),
        }
        let got = match post_form(url, basic, &form) {
            Ok(got) => got,
            Err(Fail::Rejected(why)) if code.is_none() => {
                eprintln!("vault: {} 토큰 갱신이 거절됨({why}). 다시 연결해야 합니다", svc.title);
                self.update(svc, &keys.client_id, |k| {
                    k.refresh_token.clear();
                    k.access_token.clear();
                    k.expires_at = 0;
                });
                return Err(VAULT_ERR_NOT_LINKED.into());
            }
            Err(Fail::Rejected(why) | Fail::Other(why)) => return Err(format!("{} 토큰 발급 실패: {why}", svc.title)),
        };
        let access = got["access_token"].as_str().filter(|t| printable(t)).ok_or_else(|| format!("{} 응답에 access_token이 없습니다", svc.title))?.to_string();
        let expires_at = now_secs() + got["expires_in"].as_u64().unwrap_or(0);
        let refresh = got["refresh_token"].as_str().filter(|t| printable(t)).map(str::to_string);
        let scope = got["scope"].as_str().unwrap_or_default().to_string();
        let mut fresh = None;
        self.update(svc, &keys.client_id, |k| {
            k.access_token = access.clone();
            k.expires_at = expires_at;
            k.scope = scope.clone();
            if let Some(r) = &refresh {
                k.refresh_token = r.clone();
            }
            fresh = Some(lent(k));
        });
        fresh.ok_or_else(|| "그사이 키가 바뀌거나 삭제되었습니다".to_string())
    }

    /// 보관 중인 키가 여전히 `client_id`의 것일 때만 고치고 저장한다(토큰 요청 중에 키가 바뀐 경우 무시)
    fn update(&self, svc: &Service, client_id: &str, change: impl FnOnce(&mut Keys)) {
        let changed = {
            let mut book = self.book.lock().unwrap();
            match book.services.get_mut(svc.id).filter(|k| k.client_id == client_id) {
                Some(k) => {
                    change(k);
                    true
                }
                None => false,
            }
        };
        if changed {
            self.save();
            self.announce();
        }
    }

    /// 덱이 직접 쓸 토큰을 얻는다(호스트 허용을 거치지 않는다). `stale`이면 보관한 토큰을 버리고 갱신한다
    /// (서비스가 토큰을 거부했을 때). 실패하면 `VAULT_ERR_*` 코드나 사유 문장을 돌려준다.
    pub fn own_token(&self, service_id: &str, stale: bool) -> Result<String, String> {
        let svc = service(service_id).ok_or(VAULT_ERR_UNKNOWN)?;
        if stale {
            let client_id = self.book.lock().unwrap().services.get(svc.id).map(|k| k.client_id.clone()).unwrap_or_default();
            self.update(svc, &client_id, |k| k.expires_at = 0);
        }
        self.token(svc, None).map(|v| v["access_token"].as_str().unwrap_or_default().to_string())
    }

    /// 서비스 승인을 받아 refresh 토큰이 있는지 여부
    pub fn linked(&self, service_id: &str) -> bool {
        self.book.lock().unwrap().services.get(service_id).is_some_and(|k| !k.refresh_token.is_empty())
    }

    /// 화면에서 묻고 있는 요청에 답한다
    pub fn answer(&self, allow: bool) {
        if let Some((_, tx)) = self.asking.lock().unwrap().take() {
            let _ = tx.send(allow);
        }
        self.announce();
    }

    /// 묻는 중인 요청이 이 호스트의 것이면 거두어들인다(연결이 끊긴 경우)
    pub fn cancel_asks(&self, host: &str) {
        let dropped = {
            let mut asking = self.asking.lock().unwrap();
            if asking.as_ref().is_some_and(|(a, _)| a.host == host) {
                asking.take()
            } else {
                None
            }
        };
        if dropped.is_some() {
            self.announce();
        }
    }

    /// 이 서비스에 대한 호스트의 허용을 해제한다
    pub fn revoke(&self, service_id: &str, host: &str) {
        let removed = self.book.lock().unwrap().grants.get_mut(service_id).is_some_and(|g| g.remove(host));
        if removed {
            self.save();
            self.announce();
        }
    }

    /// 서비스 키와 토큰을 모두 삭제한다(허용 목록은 남긴다)
    pub fn forget(&self, service_id: &str) {
        let removed = self.book.lock().unwrap().services.remove(service_id).is_some();
        if removed {
            self.save();
            self.announce();
        }
    }

    /// 페어링이 삭제된 호스트의 허용을 모두 지운다
    pub fn forget_host(&self, host: &str) {
        let removed = {
            let mut book = self.book.lock().unwrap();
            let mut removed = false;
            for g in book.grants.values_mut() {
                removed |= g.remove(host);
            }
            removed
        };
        if removed {
            self.save();
            self.announce();
        }
    }
}

#[cfg(test)]
mod tests;
