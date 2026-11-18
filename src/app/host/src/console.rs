//! 웹 콘솔: 브라우저에서 호스트의 toss-connector를 관리하는 정적 웹 UI와 로컬 HTTP API.
//! CLI(`toss-connector allow`, `command add` 등) 기능을 웹 UI로 제공한다(덱별 권한, 명령 관리, 페어링, 절전/깨우기, 외부 연동).
//! `toss-connector discord` CLI도 이 API를 호출한다(덱 금고 요청은 실행 중인 데몬의 덱 세션을 거쳐야 하기 때문).
//!
//! 정적 파일(src/app/host/console/의 index.html, app.js, style.css; 빌드 과정 없음)로 구성된다.
//!   - 기본: 실행 중인 toss-connector가 http://127.0.0.1:<포트>/ 에서 직접 서빙(`toss-connector console`로 열기).
//!   - 선택: 외부 정적 호스팅(GitHub Pages 등)에 올려두고 사용 가능. 이 경우 호스트에서 해당 출처를
//!     미리 허용해야 API 호출이 가능하다(`toss-connector console allow <출처>`).
//!
//! 이 API에 접근할 수 있으면 호스트에서 임의의 명령을 등록·실행할 수 있다.
//! 그런데 브라우저에서는 어떤 웹 페이지든 127.0.0.1로 요청을 보낼 수 있으므로 4가지 보안 제어를 둔다:
//!   1. 127.0.0.1에만 바인딩해 외부 네트워크에서의 접근을 차단한다.
//!   2. 토큰 인증: 모든 `/api/` 요청에 `Authorization: Bearer <토큰>` 헤더가 필요하다. 토큰은 설정 디렉터리의
//!      `console-token`에 소유자 전용(유닉스에서 0600)으로 저장되며, `toss-connector console` 실행 시 URL 해시(#token=...)로 전달된다.
//!      외부 호스팅 페이지에서는 사용자가 직접 입력한다.
//!   3. Origin 검사: 요청에 Origin 헤더가 있으면 로컬 콘솔 출처이거나 허용 목록에 등록된 출처여야 한다.
//!      와일드카드(*) 허용은 지원하지 않으며, 허용 목록 수정은 CLI에서만 가능하다(API 미제공).
//!   4. Host 검사: 요청의 Host 헤더가 `127.0.0.1:<포트>` 또는 `localhost:<포트>`여야 한다
//!      (외부 도메인을 127.0.0.1로 가리켜 동일 출처처럼 우회하는 DNS 리바인딩 방지).
//!
//! 외부 호스팅 출처를 허용하는 것은 해당 페이지의 스크립트를 신뢰한다는 의미다:
//! 그 페이지를 수정할 수 있는 사람은 호스트에서 명령을 실행할 수 있다.
//!
//! API (모두 JSON, 실패 시 `{"error": "사유"}`와 4xx/5xx 반환)
//!   GET    /api/state                      전체 상태(호스트 정보, 권한 목록, 덱 상태·권한, 명령 목록, 페어링 상태, 허용 출처)
//!   POST   /api/decks/<키>/perms           `{"id": "command.run", "on": true}` (`on`이 null이면 기본값으로 초기화)
//!   POST   /api/decks/<키>/forget          페어링 삭제
//!   POST   /api/decks/<키>/sleep | wake    절전 전환 / 깨우기 (깨우기는 최대 20초 소요)
//!   POST   /api/commands                   명령 등록 (commands::Command 형식, ID 자동 생성)
//!   PUT    /api/commands/<id>              명령 수정 (ID 유지)
//!   DELETE /api/commands/<id>              명령 삭제
//!   POST   /api/commands/<id>/run          명령 테스트 실행 (run 종류)
//!   POST   /api/commands/<id>/set          `{"value": "on" | "auto" | 40}` 값 설정 테스트
//!   GET    /api/commands/<id>/state        현재 상태 조회 → `{"value": 숫자 또는 null, "text": "..."}`
//!   POST   /api/pair                       `{"addr": "172.16.42.1"}` 페어링 시작 (진행 상태는 state.pairing 참고)
//!   POST   /api/pair/confirm               검증 숫자 일치 확인 / POST /api/pair/cancel 페어링 취소
//!   POST   /api/vault/<서비스>/put         `{"client_id", "client_secret", "redirect_uri"}` 서비스 키를 덱 금고에 저장(이 PC에는 남기지 않음)
//!   POST   /api/vault/<서비스>/forget      덱 금고에서 서비스 키와 토큰 삭제
//!   POST   /api/discord/link | check       Discord 연결(승인) / 확인 시작 (진행 상태는 state.discord 참고) / POST /api/discord/clear 결과 지우기
//!   (금고 요청은 덱이 화면에서 허용 여부를 묻는 동안 최대 1분쯤 응답이 늦어질 수 있다. 연결된 덱의 금고 상태는 state.vault)
//!
//! 설정은 console.json에 저장한다: `{"port": 7880, "origins": ["https://..."]}`.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use toss_proto::secure::Identity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::{self, Command, Commands};
use crate::jsonfile;
use crate::pairing::{Answerer, Begin, Pairing, Step};
use crate::store::Store;
use crate::trust;

pub const CONSOLE_PORT: u16 = 7880;
/// HTTP 요청 헤더/본문 최대 크기
const HEAD_MAX: usize = 16 * 1024;
const BODY_MAX: usize = 64 * 1024;

const INDEX_HTML: &str = include_str!("../console/index.html");
const APP_JS: &str = include_str!("../console/app.js");
const STYLE_CSS: &str = include_str!("../console/style.css");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: u16,
    /// 로컬 콘솔 외에 API 호출을 허용할 외부 출처 목록 ("https://example.github.io" 형식)
    #[serde(default)]
    pub origins: Vec<String>,
}

fn default_port() -> u16 {
    CONSOLE_PORT
}

impl Default for Config {
    fn default() -> Self {
        Self { port: CONSOLE_PORT, origins: Vec::new() }
    }
}

/// 콘솔 설정 로드. 파일이 없거나 읽을 수 없어도 기본값(허용 출처 없음, 안전 상태)으로 동작한다(오류 시 1회 경고).
pub fn config(dir: &Path) -> Config {
    jsonfile::load_or_report(&dir.join("console.json"), "복구될 때까지 기본 설정(포트 7880, 외부 출처 허용 없음)으로 동작합니다").ok().flatten().unwrap_or_default()
}

/// 콘솔 설정 수정 (손상된 파일은 덮어쓰지 않음)
fn edit_config(dir: &Path, change: impl FnOnce(&mut Config)) -> io::Result<()> {
    jsonfile::edit(&dir.join("console.json"), |cfg: &mut Config| {
        change(cfg);
        Ok::<_, io::Error>(())
    })
}

/// 콘솔 인증 토큰을 읽거나, 없으면 새로 생성해 소유자 전용(유닉스에서 0600) 권한으로 저장한다.
pub fn token(dir: &Path) -> io::Result<String> {
    let path = dir.join("console-token");
    if let Ok(t) = fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let t = Identity::generate()?.id();
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(&path)?.write_all(t.as_bytes())?;
    Ok(t)
}

/// 출처 문자열 정규화: "http(s)://호스트[:포트]" 형식만 허용하고 경로는 제거한다. 유효하지 않으면 None
pub fn normalize_origin(text: &str) -> Option<String> {
    let text = text.trim().trim_end_matches('/');
    let (scheme, rest) = text.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = rest.split('/').next()?.to_ascii_lowercase();
    let ok = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c));
    ok.then(|| format!("{scheme}://{host}"))
}

pub fn cli(store: &Store, rest: &[String]) {
    let dir = store.dir();
    let cfg = config(dir);
    let tok = || token(dir).unwrap_or_else(|e| crate::fail(&format!("토큰을 읽거나 생성할 수 없습니다: {e}")));
    let url = || format!("http://127.0.0.1:{}/#token={}", cfg.port, tok());
    match rest.first().map(String::as_str) {
        None | Some("open") => {
            if TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, cfg.port)), Duration::from_millis(500)).is_err() {
                crate::fail(&format!("웹 콘솔이 실행 중이지 않습니다 (127.0.0.1:{}). `toss-connector` 데몬을 먼저 실행하세요", cfg.port));
            }
            let url = url();
            let opened = if cfg!(windows) {
                std::process::Command::new("cmd").args(["/C", "start", "", &url]).status()
            } else if cfg!(target_os = "macos") {
                std::process::Command::new("open").arg(&url).status()
            } else {
                std::process::Command::new("xdg-open").arg(&url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status()
            };
            match opened {
                Ok(s) if s.success() => println!("브라우저에서 열었습니다: http://127.0.0.1:{}/", cfg.port),
                _ => println!("브라우저를 자동으로 열지 못했습니다. 아래 주소로 직접 접속하세요:\n{url}"),
            }
        }
        Some("url") => println!("{}", url()),
        Some("token") => println!("{}", tok()),
        Some("list") => {
            println!("콘솔 주소: http://127.0.0.1:{}/", cfg.port);
            if cfg.origins.is_empty() {
                println!("허용된 외부 출처가 없습니다 (로컬 toss-connector에서 제공하는 콘솔만 사용 가능).");
            }
            for o in &cfg.origins {
                println!("  {o}");
            }
        }
        Some(what @ ("allow" | "deny")) => {
            let text = rest.get(1).unwrap_or_else(|| crate::usage("출처를 지정하세요. 예: toss-connector console allow https://example.github.io"));
            let origin = normalize_origin(text).unwrap_or_else(|| crate::fail("출처는 경로를 제외한 https://호스트 형식이어야 합니다"));
            edit_config(dir, |cfg| {
                cfg.origins.retain(|o| *o != origin);
                if what == "allow" {
                    cfg.origins.push(origin.clone());
                }
            })
            .unwrap_or_else(|e| crate::fail(&format!("설정을 저장할 수 없습니다: {e}")));
            if what == "allow" {
                println!("허용됨: {origin}");
                println!("이제 해당 페이지에서 토큰 입력 후 이 PC의 toss-connector를 제어할 수 있습니다. 신뢰하는 페이지인 경우에만 허용하세요.");
                println!("토큰 확인: toss-connector console token");
            } else {
                println!("허용 해제됨: {origin}");
            }
            println!("실행 중인 toss-connector에 곧 반영됩니다.");
        }
        Some("port") => {
            let port: u16 = rest.get(1).and_then(|p| p.parse().ok()).filter(|p| *p >= 1024).unwrap_or_else(|| crate::usage("포트 번호(1024~65535)를 지정하세요"));
            edit_config(dir, |cfg| cfg.port = port).unwrap_or_else(|e| crate::fail(&format!("설정을 저장할 수 없습니다: {e}")));
            println!("콘솔 포트가 {port}(으)로 변경되었습니다. toss-connector를 재시작하면 적용됩니다.");
        }
        Some(other) => crate::usage(&format!("알 수 없는 명령: console {other}")),
    }
}

/// 백그라운드 스레드로 웹 콘솔 서버를 시작한다. 바인딩에 실패해도 경고만 출력하고 toss-connector 본연의 기능은 유지한다.
pub fn spawn(dir: PathBuf, managed: Option<PathBuf>) {
    let port = config(&dir).port;
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("웹 콘솔 시작 실패 (127.0.0.1:{port}): {e}");
            return;
        }
    };
    if let Err(e) = token(&dir) {
        eprintln!("웹 콘솔 토큰 생성 실패: {e}");
        return;
    }
    eprintln!("웹 콘솔: http://127.0.0.1:{port}/  (열기: toss-connector console)");
    thread::spawn(move || serve(listener, dir, managed));
}

/// 연결 수락 루프 (요청별 스레드 생성)
pub fn serve(listener: TcpListener, dir: PathBuf, managed: Option<PathBuf>) {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(CONSOLE_PORT);
    let ctx = Arc::new(Ctx { dir, managed, port, pairing: Mutex::new(Progress::default()), discord: Mutex::new(Job::default()) });
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = handle(stream, &ctx);
        });
    }
}

struct Ctx {
    dir: PathBuf,
    /// 시스템 관리 설정(managed 모듈) 경로
    managed: Option<PathBuf>,
    port: u16,
    pairing: Mutex<Progress>,
    discord: Mutex<Job>,
}

struct Request {
    method: String,
    /// 쿼리 스트링을 제외한 경로
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > HEAD_MAX {
            return Err(bad("HTTP 헤더가 너무 큽니다"));
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(bad("요청이 중단되었습니다"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default());
    let path = target.split('?').next().unwrap_or_default().to_string();
    let headers: HashMap<String, String> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > BODY_MAX {
        return Err(bad("HTTP 본문이 너무 큽니다"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(bad("요청이 중단되었습니다"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Ok(Request { method, path, headers, body })
}

struct Response {
    status: u16,
    kind: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: Value) -> Self {
        Self { status, kind: "application/json; charset=utf-8", body: value.to_string().into_bytes() }
    }
    fn error(status: u16, why: impl Into<String>) -> Self {
        Self::json(status, json!({ "error": why.into() }))
    }
    fn file(kind: &'static str, text: &str) -> Self {
        Self { status: 200, kind, body: text.as_bytes().to_vec() }
    }
}

fn handle(mut stream: TcpStream, ctx: &Arc<Ctx>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let req = match read_request(&mut stream) {
        Ok(r) => r,
        Err(_) => return write_response(&mut stream, &Response::error(400, "잘못된 HTTP 요청입니다"), None),
    };
    let cfg = config(&ctx.dir);
    let origin = req.headers.get("origin").cloned();
    let allowed_origin = origin.as_deref().filter(|o| origin_allowed(o, ctx.port, &cfg));
    let res = route(&req, ctx, &cfg, origin.as_deref());
    write_response(&mut stream, &res, allowed_origin)
}

/// 로컬 콘솔 자체 출처이거나 허용 목록에 포함된 출처인지 확인
fn origin_allowed(origin: &str, port: u16, cfg: &Config) -> bool {
    origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}") || cfg.origins.iter().any(|o| o == origin)
}

/// 길이가 같을 때의 상수 시간 문자열 비교(타이밍 공격 방지)
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn route(req: &Request, ctx: &Arc<Ctx>, cfg: &Config, origin: Option<&str>) -> Response {
    let host = req.headers.get("host").map(String::as_str).unwrap_or_default();
    if host != format!("127.0.0.1:{}", ctx.port) && host != format!("localhost:{}", ctx.port) {
        return Response::error(403, "허용되지 않은 Host 헤더입니다");
    }
    if let Some(o) = origin {
        if !origin_allowed(o, ctx.port, cfg) {
            return Response::error(403, "허용되지 않은 출처입니다 (toss-connector console allow <출처>)");
        }
    }
    if req.method == "OPTIONS" {
        return if origin.is_some() { Response { status: 204, kind: "text/plain", body: Vec::new() } } else { Response::error(403, "Origin 헤더가 없습니다") };
    }
    if !req.path.starts_with("/api/") {
        return match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/" | "/index.html") => Response::file("text/html; charset=utf-8", INDEX_HTML),
            ("GET", "/app.js") => Response::file("text/javascript; charset=utf-8", APP_JS),
            ("GET", "/style.css") => Response::file("text/css; charset=utf-8", STYLE_CSS),
            _ => Response::error(404, "존재하지 않는 경로입니다"),
        };
    }
    let given = req.headers.get("authorization").and_then(|v| v.strip_prefix("Bearer ")).unwrap_or_default();
    match token(&ctx.dir) {
        Ok(t) if same(given.trim(), &t) => {}
        Ok(_) => return Response::error(401, "토큰이 없거나 올바르지 않습니다 (toss-connector console token)"),
        Err(e) => return Response::error(500, format!("토큰을 읽을 수 없습니다: {e}")),
    }
    match api(req, ctx, cfg) {
        Ok(v) => Response::json(200, v),
        Err((status, why)) => Response::error(status, why),
    }
}

fn write_response(stream: &mut TcpStream, res: &Response, allowed_origin: Option<&str>) -> io::Result<()> {
    let reason = match res.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
        res.status,
        res.kind,
        res.body.len()
    );
    if res.kind.starts_with("text/html") {
        head.push_str("Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'\r\n");
    }
    if let Some(o) = allowed_origin {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\nAccess-Control-Max-Age: 600\r\n"
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&res.body)?;
    stream.flush()
}

type ApiResult = Result<Value, (u16, String)>;

fn bad(why: impl Into<String>) -> (u16, String) {
    (400, why.into())
}

/// 명령 오류를 HTTP 상태 코드로 변환: 잠금/고정 409, 미존재 404, 유효성 오류 400, I/O·실행 오류 500
fn refused(e: commands::Error) -> (u16, String) {
    let status = match e {
        commands::Error::Locked | commands::Error::Managed => 409,
        commands::Error::NotFound => 404,
        commands::Error::Invalid(_) => 400,
        commands::Error::Io(_) => 500,
    };
    (status, e.to_string())
}

fn api(req: &Request, ctx: &Arc<Ctx>, cfg: &Config) -> ApiResult {
    let store = Store::open(Some(ctx.dir.clone())).map_err(|e| (500, format!("설정 디렉터리를 열 수 없습니다: {e}")))?.with_managed(ctx.managed.clone());
    let cmds = store.commands();
    let locked = |why: String| (409, why);
    let body = || -> Result<Value, (u16, String)> { serde_json::from_slice(&req.body).map_err(|_| bad("요청 본문이 유효한 JSON이 아닙니다")) };
    let parts: Vec<&str> = req.path.trim_start_matches("/api/").split('/').collect();
    let deck = |key: &str| store.decks().into_iter().find(|d| d.key == key).ok_or((404, "존재하지 않는 덱입니다".to_string()));
    let command = |id: &str| cmds.by_id(id).ok_or((404, "존재하지 않는 명령입니다".to_string()));
    let done = || Ok(json!({ "ok": true }));
    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["state"]) => Ok(state(&store, &cmds, ctx, cfg)),

        ("POST", ["decks", key, "perms"]) => {
            let b = body()?;
            let id = b["id"].as_str().and_then(trust::find).ok_or(bad("알 수 없는 권한입니다"))?.id;
            let on = match &b["on"] {
                Value::Bool(on) => Some(*on),
                Value::Null => None,
                _ => return Err(bad("on 값은 true, false, null 중 하나여야 합니다")),
            };
            if store.managed().is_some_and(|m| m.fixed(id).is_some()) {
                return Err(locked(format!("{id}은(는) 시스템 설정(Nix)으로 고정되어 있습니다")));
            }
            let found = store
                .update(key, |d| match on {
                    Some(on) => {
                        d.perms.insert(id.to_string(), on);
                    }
                    None => {
                        d.perms.remove(id);
                    }
                })
                .map_err(|e| (500, format!("설정 파일을 수정할 수 없습니다: {e}")))?;
            if !found {
                return Err((404, "존재하지 않는 덱입니다".into()));
            }
            done()
        }
        ("POST", ["decks", key, "forget"]) => {
            deck(key)?;
            store.forget(key).map_err(|e| (500, format!("설정 파일을 수정할 수 없습니다: {e}")))?;
            done()
        }
        ("POST", ["decks", key, "sleep"]) => {
            let d = deck(key)?;
            let me = store.identity().map_err(|e| (500, format!("키를 읽을 수 없습니다: {e}")))?;
            let (name, can_wake) = crate::standby::sleep_deck(&me, &d).map_err(|e| (409, e))?;
            Ok(json!({ "ok": true, "name": name, "can_wake": can_wake }))
        }
        ("POST", ["decks", key, "wake"]) => {
            let d = deck(key)?;
            let addr = crate::standby::wake_deck(&d, || {}).map_err(|e| (409, e))?;
            Ok(json!({ "ok": true, "addr": addr }))
        }

        ("POST", ["commands"]) => {
            let spec: Command = serde_json::from_value(body()?).map_err(|e| bad(format!("명령 형식이 올바르지 않습니다: {e}")))?;
            let c = cmds.add(spec).map_err(refused)?;
            Ok(json!({ "ok": true, "command": c }))
        }
        ("PUT", ["commands", id]) => {
            let spec: Command = serde_json::from_value(body()?).map_err(|e| bad(format!("명령 형식이 올바르지 않습니다: {e}")))?;
            let c = cmds.replace(id, spec).map_err(refused)?;
            Ok(json!({ "ok": true, "command": c }))
        }
        ("DELETE", ["commands", id]) => {
            cmds.remove(id).map_err(refused)?;
            done()
        }
        ("POST", ["commands", id, "run"]) => {
            Commands::run_by(&command(id)?).map_err(refused)?;
            done()
        }
        ("POST", ["commands", id, "set"]) => {
            let c = command(id)?;
            let b = body()?;
            let text = match &b["value"] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(on) => if *on { "on" } else { "off" }.to_string(),
                _ => return Err(bad("설정할 값이 없습니다")),
            };
            let value = c.parse_value(&text).map_err(bad)?;
            c.set_value(value).map_err(|e| (500, format!("값 설정 실패: {e}")))?;
            Ok(json!({ "ok": true, "value": value, "text": c.show_value(Some(value)) }))
        }
        ("GET", ["commands", id, "state"]) => {
            let c = command(id)?;
            if c.state.is_empty() {
                return Err(bad("상태 확인 커맨드가 설정되어 있지 않습니다"));
            }
            let value = c.read_value();
            Ok(json!({ "value": value, "text": c.show_value(value) }))
        }

        ("POST", ["pair"]) => {
            let b = body()?;
            let addr = b["addr"].as_str().map(str::trim).filter(|a| !a.is_empty() && a.len() <= 255).unwrap_or(toss_proto::USB_DECK_ADDR).to_string();
            pair_start(ctx, addr)?;
            done()
        }
        ("POST", ["pair", "confirm"]) => pair_answer(ctx, true),
        ("POST", ["pair", "cancel"]) => pair_answer(ctx, false),

        ("POST", ["vault", service, "put"]) => {
            let b = body()?;
            let text = |key: &str| b[key].as_str().map(str::trim).unwrap_or_default().to_string();
            let fields = json!({ "client_id": text("client_id"), "client_secret": text("client_secret"), "redirect_uri": text("redirect_uri") });
            crate::vault::ask(toss_proto::VAULT_PUT, service, fields).map_err(|e| (409, e))?;
            done()
        }
        ("POST", ["vault", service, "forget"]) => {
            crate::vault::ask(toss_proto::VAULT_FORGET, service, Value::Null).map_err(|e| (409, e))?;
            done()
        }
        ("POST", ["discord", kind @ ("link" | "check")]) => {
            discord_start(ctx, if *kind == "link" { "link" } else { "check" })?;
            done()
        }
        ("POST", ["discord", "clear"]) => {
            let mut job = ctx.discord.lock().unwrap();
            if !job.busy() {
                *job = Job::default();
            }
            done()
        }

        _ => Err((404, "존재하지 않는 경로입니다".into())),
    }
}

fn state(store: &Store, cmds: &Commands, ctx: &Ctx, cfg: &Config) -> Value {
    let attached = crate::daemon::ATTACHED.lock().unwrap().clone();
    let managed = store.managed();
    let fixed = |id: &str| managed.as_ref().is_some_and(|m| m.fixed(id).is_some());
    let decks: Vec<Value> = store
        .decks()
        .iter()
        .map(|d| {
            let here = attached.as_ref().filter(|(key, _)| *key == d.key);
            let now = managed.as_ref().map_or_else(|| d.perms.clone(), |m| m.apply(&d.perms));
            let perms: Vec<Value> =
                trust::PERMISSIONS.iter().map(|p| json!({ "id": p.id, "on": trust::allowed(&now, p.id), "chosen": d.perms.contains_key(p.id), "fixed": fixed(p.id) })).collect();
            json!({
                "key": d.key,
                "name": d.name,
                "addrs": d.addrs,
                "can_wake": !d.wake.is_empty(),
                "connected": here.is_some(),
                "addr": here.map(|(_, addr)| addr.clone()),
                "perms": perms,
            })
        })
        .collect();
    json!({
        "host": { "name": crate::link::host_name(), "os": std::env::consts::OS, "version": env!("CARGO_PKG_VERSION") },
        "permissions": trust::PERMISSIONS.iter().map(|p| json!({ "id": p.id, "title": p.title, "about": p.about, "default": p.default })).collect::<Vec<_>>(),
        "decks": decks,
        "commands": cmds.list().iter().map(|c| {
            let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
            v["managed"] = json!(managed.as_ref().is_some_and(|m| m.commands.iter().any(|k| k.id == c.id)));
            v
        }).collect::<Vec<_>>(),
        "managed": managed.as_ref().map(|m| json!({ "strict": m.strict, "permissions": m.permissions, "commands": m.commands.len() })),
        "pairing": ctx.pairing.lock().unwrap().shown(),
        "vault": crate::vault::state().map(|(deck, services)| json!({ "deck": deck, "services": services })),
        "discord": ctx.discord.lock().unwrap().shown(),
        "origins": cfg.origins,
        "usb_addr": toss_proto::USB_DECK_ADDR,
    })
}

/// 웹 UI 표시용 페어링 진행 상태
#[derive(Default)]
struct Progress {
    /// idle        대기 중
    /// connecting  덱에 연결 시도 중
    /// code        검증 코드 표시됨(사용자 확인 대기)
    /// waiting     호스트 확인 완료(덱 수락 대기)
    /// done        완료 / failed 실패 (error에 사유)
    phase: &'static str,
    addr: String,
    name: String,
    code: String,
    error: String,
    /// 사용자 응답(일치/취소)을 페어링 스레드에 전달하는 채널 (code 단계 이후 활성화)
    answer: Option<Answerer>,
}

impl Progress {
    fn shown(&self) -> Value {
        json!({ "phase": if self.phase.is_empty() { "idle" } else { self.phase }, "addr": self.addr, "name": self.name, "code": self.code, "error": self.error })
    }
    fn busy(&self) -> bool {
        matches!(self.phase, "connecting" | "code" | "waiting")
    }
}

/// 백그라운드 스레드에서 페어링을 진행하며 상태를 Progress에 기록한다.
fn pair_start(ctx: &Arc<Ctx>, addr: String) -> Result<(), (u16, String)> {
    let mut p = ctx.pairing.lock().unwrap();
    if p.busy() {
        return Err((409, "이미 페어링이 진행 중입니다".into()));
    }
    *p = Progress { phase: "connecting", addr: addr.clone(), ..Progress::default() };
    drop(p);
    let ctx = ctx.clone();
    thread::spawn(move || {
        let set = |change: &dyn Fn(&mut Progress)| change(&mut ctx.pairing.lock().unwrap());
        let fail = |why: String| {
            set(&|p| {
                p.phase = "failed";
                p.error = why.clone();
                p.answer = None;
            })
        };
        let store = match Store::open(Some(ctx.dir.clone())) {
            Ok(s) => s.with_managed(ctx.managed.clone()),
            Err(e) => return fail(format!("설정 디렉터리를 열 수 없습니다: {e}")),
        };
        let me = match store.identity() {
            Ok(m) => m,
            Err(e) => return fail(format!("키를 읽을 수 없습니다: {e}")),
        };
        let mut pairing = match Pairing::begin(&store, &me, &addr) {
            Ok(Begin::Ready(p)) => p,
            Ok(Begin::Already(name)) => {
                return set(&|p| {
                    p.phase = "done";
                    p.name = name.clone();
                    p.error = "이미 페어링된 덱입니다".into();
                })
            }
            Err(why) => return fail(why),
        };
        set(&|p| {
            p.phase = "code";
            p.name = pairing.name().to_string();
            p.code = pairing.code().to_string();
            p.answer = Some(pairing.answerer());
        });
        loop {
            match pairing.next() {
                Step::Confirmed => set(&|p| p.phase = "waiting"),
                Step::Paired => {
                    return set(&|p| {
                        p.phase = "done";
                        p.error.clear();
                        p.answer = None;
                    })
                }
                Step::Cancelled => return set(&|p| *p = Progress::default()),
                Step::Failed(why) => return fail(why),
            }
        }
    });
    Ok(())
}

/// 웹 UI 표시용 Discord 작업 상태
#[derive(Default)]
struct Job {
    /// link(연결·승인) 또는 check(확인)
    kind: &'static str,
    /// idle     대기 중
    /// deck     덱의 응답 대기(이 PC가 처음이면 덱 화면에서 허용 여부를 묻는 중)
    /// discord  Discord 앱 대기(link면 승인 창)
    /// done     완료(who에 인증된 사용자) / failed 실패(error에 사유)
    phase: &'static str,
    who: Option<crate::discord::Who>,
    error: String,
}

impl Job {
    fn shown(&self) -> Value {
        json!({
            "kind": self.kind,
            "phase": if self.phase.is_empty() { "idle" } else { self.phase },
            "who": self.who.as_ref().map(|w| json!({ "name": w.name, "expires": w.expires, "scopes": w.scopes })),
            "error": self.error,
        })
    }
    fn busy(&self) -> bool {
        matches!(self.phase, "deck" | "discord")
    }
}

/// 백그라운드 스레드에서 Discord 연결(link) 또는 확인(check)을 진행하며 상태를 Job에 기록한다.
fn discord_start(ctx: &Arc<Ctx>, kind: &'static str) -> Result<(), (u16, String)> {
    let mut job = ctx.discord.lock().unwrap();
    if job.busy() {
        return Err((409, "Discord 작업이 이미 진행 중입니다".into()));
    }
    *job = Job { kind, phase: crate::discord::STEP_DECK, ..Job::default() };
    drop(job);
    let ctx = ctx.clone();
    thread::spawn(move || {
        let step = |phase: &'static str| ctx.discord.lock().unwrap().phase = phase;
        let got = if kind == "link" { crate::discord::link(step) } else { crate::discord::check(step) };
        let mut job = ctx.discord.lock().unwrap();
        match got {
            Ok(who) => {
                eprintln!("Discord {}: {}", if kind == "link" { "연결됨" } else { "인증 확인" }, who.name);
                job.phase = "done";
                job.who = Some(who);
            }
            Err(why) => {
                eprintln!("Discord {} 실패: {why}", if kind == "link" { "연결" } else { "확인" });
                job.phase = "failed";
                job.error = why;
            }
        }
    });
    Ok(())
}

/// 실행 중인 toss-connector의 로컬 API를 호출한다(CLI용). 실패하면 사용자에게 보여 줄 문장을 돌려준다.
fn local_api(dir: &Path, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let port = config(dir).port;
    let tok = token(dir).map_err(|e| format!("웹 콘솔 토큰을 읽을 수 없습니다: {e}"))?;
    let mut s = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(800))
        .map_err(|_| format!("실행 중인 toss-connector에 연결할 수 없습니다 (127.0.0.1:{port}). `toss-connector` 데몬을 웹 콘솔과 함께 실행해 두세요"))?;
    s.set_read_timeout(Some(toss_proto::VAULT_ASK_WAIT + Duration::from_secs(40))).map_err(|e| e.to_string())?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    s.write_all(head.as_bytes()).and_then(|()| s.write_all(body.as_bytes())).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| format!("응답을 읽을 수 없습니다: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let (head, payload) = text.split_once("\r\n\r\n").ok_or("응답 형식이 올바르지 않습니다")?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let v: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    if status == 200 {
        Ok(v)
    } else {
        Err(v["error"].as_str().map_or_else(|| format!("요청 실패 (HTTP {status})"), str::to_string))
    }
}

/// 화면에 보이지 않게 한 줄을 입력받는다(터미널이면 `stty -echo`로 입력 표시를 끈다)
fn read_hidden(prompt: &str) -> String {
    use std::io::IsTerminal;
    print!("{prompt}");
    let _ = io::stdout().flush();
    let tty = io::stdin().is_terminal();
    let stty = |arg: &str| std::process::Command::new("stty").arg(arg).stdin(std::process::Stdio::inherit()).status().is_ok_and(|s| s.success());
    let hidden = tty && stty("-echo");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    if hidden {
        stty("echo");
        println!();
    }
    line.trim().to_string()
}

fn read_line(prompt: &str) -> String {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    line.trim().to_string()
}

/// `toss-connector discord ...`: 실행 중인 toss-connector의 로컬 API로 덱 금고와 Discord 연결을 다룬다.
pub fn discord_cli(store: &Store, sub: Option<&str>) {
    let dir = store.dir();
    let call = |method: &str, path: &str, body: Option<&Value>| local_api(dir, method, path, body).unwrap_or_else(|e| crate::fail(&e));
    match sub {
        None | Some("status") => {
            let st = call("GET", "/api/state", None);
            let deck = st["decks"].as_array().and_then(|d| d.iter().find(|d| d["connected"] == true)).map(|d| d["name"].as_str().unwrap_or_default().to_string());
            let Some(deck) = deck else { return println!("연결된 덱이 없습니다.") };
            let Some(s) = st["vault"]["services"].as_array().and_then(|l| l.iter().find(|s| s["id"] == toss_proto::SERVICE_DISCORD)) else {
                return println!("덱 {deck}이(가) 금고를 지원하지 않습니다 (덱 이미지를 새로 플래싱하세요).");
            };
            let yes = |k: &str| s[k] == true;
            println!("덱: {deck}");
            println!("  Discord 키      {}", if yes("registered") { "저장됨" } else { "없음 (toss-connector discord set)" });
            println!("  연결(승인)      {}", if yes("linked") { "됨" } else { "안 됨 (toss-connector discord link)" });
            println!("  이 PC의 사용    {}", if yes("allowed") { "허용됨" } else { "아직 허용 안 됨 (처음 쓸 때 덱 화면에서 묻습니다)" });
        }
        Some("set") => {
            println!("Discord 개발자 포털(https://discord.com/developers/applications)의 앱 > OAuth2 화면에서 값을 확인하세요.");
            let client_id = read_line("Client ID: ");
            let client_secret = read_hidden("Client Secret (입력이 표시되지 않음): ");
            let redirect = read_line("Redirect URI [http://localhost, 비우려면 -]: ");
            let redirect = match redirect.as_str() {
                "" => "http://localhost".to_string(),
                "-" => String::new(),
                _ => redirect,
            };
            println!("덱에 저장하는 중... (이 PC가 처음이면 덱 화면에서 ALLOW를 누르세요)");
            call("POST", "/api/vault/discord/put", Some(&json!({ "client_id": client_id, "client_secret": client_secret, "redirect_uri": redirect })));
            println!("덱에 저장했습니다. 이 PC에는 남기지 않았습니다. 이어서: toss-connector discord link");
        }
        Some(kind @ ("link" | "check")) => {
            call("POST", &format!("/api/discord/{kind}"), None);
            let mut said = "";
            loop {
                let job = call("GET", "/api/state", None)["discord"].clone();
                let phase = job["phase"].as_str().unwrap_or("idle").to_string();
                match phase.as_str() {
                    "done" => {
                        println!("Discord 인증 확인: {} (토큰 만료: {})", job["who"]["name"].as_str().unwrap_or_default(), job["who"]["expires"].as_str().unwrap_or("?"));
                        return;
                    }
                    "failed" => crate::fail(job["error"].as_str().unwrap_or("실패했습니다")),
                    "deck" if said != "deck" => {
                        said = "deck";
                        println!("덱의 응답을 기다리는 중... (이 PC가 처음이면 덱 화면에서 ALLOW를 누르세요)");
                    }
                    "discord" if said != "discord" => {
                        said = "discord";
                        println!("{}", if kind == "link" { "Discord 앱에 뜬 승인 창에서 승인하세요..." } else { "Discord 앱에 인증하는 중..." });
                    }
                    _ => {}
                }
                thread::sleep(Duration::from_millis(700));
            }
        }
        Some("forget") => {
            call("POST", "/api/vault/discord/forget", None);
            println!("덱에서 Discord 키와 토큰을 삭제했습니다.");
        }
        Some(other) => crate::usage(&format!("알 수 없는 명령: discord {other}")),
    }
}

fn pair_answer(ctx: &Ctx, same: bool) -> ApiResult {
    let mut p = ctx.pairing.lock().unwrap();
    match (&p.answer, p.phase) {
        (Some(answer), "code") | (Some(answer), "waiting") if !same || p.phase == "code" => {
            answer.send(same);
            Ok(json!({ "ok": true }))
        }
        (_, "connecting") if !same => Err((409, "덱에 연결 중입니다. 잠시 후 다시 시도하세요".into())),
        _ if !same => {
            if !p.busy() {
                *p = Progress::default();
            }
            Ok(json!({ "ok": true }))
        }
        _ => Err((409, "현재 확인할 페어링 요청이 없습니다".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Deck;
    use crate::trust::Choices;
    use toss_proto::secure::Conn;
    use toss_proto::{DeckMsg, HostMsg, STATUS_PAIRING};

    /// 테스트용 콘솔 서버 생성: 임시 디렉터리와 임의 포트로 리스닝
    fn console(name: &str) -> (PathBuf, u16, String) {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-console-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let tok = token(&dir).unwrap();
        let dir2 = dir.clone();
        thread::spawn(move || serve(listener, dir2, None));
        (dir, port, tok)
    }

    /// HTTP 요청 전송 후 (상태 코드, 소문자 헤더, 본문) 반환. Host 헤더 생략 시 기본값 삽입
    fn send(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, String) {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let mut text = format!("{method} {path} HTTP/1.1\r\n");
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            text.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
        }
        for (k, v) in headers {
            text.push_str(&format!("{k}: {v}\r\n"));
        }
        text.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        s.write_all(text.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
        let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, head.to_ascii_lowercase(), body.to_string())
    }

    /// 인증 토큰을 포함해 API 호출 후 (상태 코드, JSON) 반환
    fn call(port: u16, tok: &str, method: &str, path: &str, body: &str) -> (u16, Value) {
        let auth = format!("Bearer {tok}");
        let (status, _, text) = send(port, method, path, &[("Authorization", &auth)], body);
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    #[test]
    fn origins_are_written_in_one_shape() {
        assert_eq!(normalize_origin(" HTTPS://Example.GitHub.io/deck/ "), Some("https://example.github.io".into()));
        assert_eq!(normalize_origin("http://localhost:8080"), Some("http://localhost:8080".into()));
        for bad in ["example.github.io", "ftp://x", "https://", "*", "https://a b"] {
            assert_eq!(normalize_origin(bad), None, "{bad}");
        }
    }

    /// 정적 파일은 인증 없이 제공하지만, API는 유효한 토큰과 허용된 Host 및 Origin만 수락한다.
    #[test]
    fn the_api_takes_only_the_token_this_address_and_allowed_origins() {
        let (dir, port, tok) = console("guard");
        let auth = format!("Bearer {tok}");
        let me = format!("http://127.0.0.1:{port}");

        let (status, head, body) = send(port, "GET", "/", &[], "");
        assert_eq!(status, 200);
        assert!(head.contains("content-security-policy: default-src 'none'") && body.contains("<title>toss-connector"));
        assert_eq!(send(port, "GET", "/app.js", &[], "").0, 200);
        assert_eq!(send(port, "GET", "/style.css", &[], "").0, 200);
        assert_eq!(send(port, "GET", "/../key", &[], "").0, 404);

        assert_eq!(send(port, "GET", "/api/state", &[], "").0, 401);
        assert_eq!(send(port, "GET", "/api/state", &[("Authorization", "Bearer wrong")], "").0, 401);
        assert_eq!(send(port, "GET", "/api/state", &[("Authorization", &auth)], "").0, 200);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(dir.join("console-token")).unwrap().permissions().mode() & 0o777, 0o600);
        }

        assert_eq!(send(port, "GET", "/api/state", &[("Authorization", &auth), ("Host", "evil.example")], "").0, 403);
        assert_eq!(send(port, "GET", "/", &[("Host", &format!("evil.example:{port}"))], "").0, 403);

        let (status, head, _) = send(port, "GET", "/api/state", &[("Authorization", &auth), ("Origin", &me)], "");
        assert!(status == 200 && head.contains(&format!("access-control-allow-origin: {me}")));
        let (status, head, _) = send(port, "GET", "/api/state", &[("Authorization", &auth), ("Origin", "https://evil.example")], "");
        assert!(status == 403 && !head.contains("access-control-allow-origin"), "{head}");
        let (status, head, _) = send(port, "OPTIONS", "/api/commands", &[("Origin", "https://evil.example"), ("Access-Control-Request-Method", "POST")], "");
        assert!(status == 403 && !head.contains("access-control-allow-origin"));
        assert_eq!(send(port, "GET", "/api/state", &[("Authorization", &auth), ("Origin", "null")], "").0, 403);

        edit_config(&dir, |cfg| *cfg = Config { port, origins: vec!["https://me.example".into()] }).unwrap();
        let (status, head, _) = send(port, "OPTIONS", "/api/commands", &[("Origin", "https://me.example"), ("Access-Control-Request-Method", "POST")], "");
        assert!(status == 204 && head.contains("access-control-allow-origin: https://me.example") && head.contains("access-control-allow-headers: authorization"));
        assert_eq!(send(port, "GET", "/api/state", &[("Authorization", &auth), ("Origin", "https://me.example")], "").0, 200);
        assert_eq!(send(port, "GET", "/api/state", &[("Origin", "https://me.example")], "").0, 401);
        assert_eq!(call(port, &tok, "POST", "/api/origins", r#"{"origin":"https://evil.example"}"#).0, 404);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 덱별 권한과 명령 수정이 CLI와 동일한 설정 파일에 반영되는지 확인
    #[test]
    fn permissions_and_commands_are_edited_in_the_same_files_the_cli_uses() {
        let (dir, port, tok) = console("edit");
        let store = Store::open(Some(dir.clone())).unwrap();
        store.remember(Deck { key: "ab12".into(), name: "deck".into(), addrs: vec!["10.0.0.9".into()], wake: String::new(), perms: Choices::new() }).unwrap();

        let (_, st) = call(port, &tok, "GET", "/api/state", "");
        assert_eq!((&st["decks"][0]["name"], &st["decks"][0]["connected"]), (&json!("deck"), &json!(false)));
        let perm = |st: &Value, id: &str| st["decks"][0]["perms"].as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap();
        assert_eq!(perm(&st, "command.run"), json!({ "id": "command.run", "on": false, "chosen": false, "fixed": false }));
        assert_eq!(st["managed"], Value::Null);

        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"command.run","on":true}"#).0, 200);
        assert_eq!(store.decks()[0].perms.get("command.run"), Some(&true));
        assert_eq!(perm(&call(port, &tok, "GET", "/api/state", "").1, "command.run")["chosen"], true);
        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"command.run","on":null}"#).0, 200);
        assert!(store.decks()[0].perms.is_empty());
        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"everything","on":true}"#).0, 400);
        assert_eq!(call(port, &tok, "POST", "/api/decks/zz/perms", r#"{"id":"command.run","on":true}"#).0, 404);

        let (status, r) = call(port, &tok, "POST", "/api/commands", r#"{"name":"Mode","kind":"stepped","steps":[{"name":"a"},{"name":"b"}],"set":"true {}"}"#);
        assert_eq!(status, 200);
        let id = r["command"]["id"].as_str().unwrap().to_string();
        let cmds = Commands::new(&dir);
        assert_eq!(cmds.by_id(&id).unwrap().steps.len(), 2);
        let (status, r) = call(port, &tok, "POST", "/api/commands", r#"{"name":"Bad","kind":"linear","min":5,"max":1,"set":"x {}"}"#);
        assert!(status == 400 && r["error"].as_str().unwrap().contains("범위"), "{r}");
        assert_eq!(call(port, &tok, "PUT", &format!("/api/commands/{id}"), r#"{"name":"Mode 2","kind":"stepped","steps":[{"name":"a"},{"name":"b"},{"name":"c"}],"set":"true {}"}"#).0, 200);
        let c = cmds.by_id(&id).unwrap();
        assert_eq!((c.name.as_str(), c.steps.len()), ("Mode 2", 3));
        assert_eq!(call(port, &tok, "PUT", "/api/commands/nope", r#"{"name":"X","kind":"run","run":"true"}"#).0, 404);
        assert_eq!(call(port, &tok, "POST", &format!("/api/commands/{id}/set"), r#"{"value":"c"}"#).1["value"], 2.0);
        assert_eq!(call(port, &tok, "POST", &format!("/api/commands/{id}/set"), r#"{"value":"z"}"#).0, 400);
        assert_eq!(call(port, &tok, "POST", &format!("/api/commands/{id}/run"), "").0, 400);
        assert_eq!(call(port, &tok, "DELETE", &format!("/api/commands/{id}"), "").0, 200);
        assert!(cmds.list().is_empty());

        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/forget", "").0, 200);
        assert!(store.decks().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// 시스템 관리 설정(Nix)이 있을 때 고정된 항목이 표시되며 API로 수정할 수 없고, non-strict 모드에서는 추가가 가능한지 확인
    #[test]
    fn what_the_system_fixed_cannot_be_changed_from_the_screen() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-console-managed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("managed.json");
        let write = |strict: bool| fs::write(&file, json!({ "strict": strict, "permissions": ["command.run"], "commands": [{ "id": "aaaa0001", "name": "Fixed", "run": "true" }] }).to_string()).unwrap();
        write(true);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let tok = token(&dir).unwrap();
        let (dir2, file2) = (dir.clone(), file.clone());
        thread::spawn(move || serve(listener, dir2, Some(file2)));
        let store = Store::open(Some(dir.clone())).unwrap();
        store.remember(Deck { key: "ab12".into(), name: "deck".into(), addrs: Vec::new(), wake: String::new(), perms: Choices::from([("media.info".to_string(), true)]) }).unwrap();
        let perm = |st: &Value, id: &str| st["decks"][0]["perms"].as_array().unwrap().iter().find(|p| p["id"] == id).cloned().unwrap();

        let (_, st) = call(port, &tok, "GET", "/api/state", "");
        assert_eq!(st["managed"], json!({ "strict": true, "permissions": ["command.run"], "commands": 1 }));
        assert_eq!((&perm(&st, "command.run")["on"], &perm(&st, "command.run")["fixed"]), (&json!(true), &json!(true)));
        assert_eq!((&perm(&st, "media.info")["on"], &perm(&st, "media.info")["fixed"]), (&json!(false), &json!(true)));
        assert_eq!((&st["commands"][0]["name"], &st["commands"][0]["managed"], st["commands"].as_array().unwrap().len()), (&json!("Fixed"), &json!(true), 1));
        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"media.info","on":true}"#).0, 409);
        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"command.run","on":false}"#).0, 409);
        assert_eq!(call(port, &tok, "POST", "/api/commands", r#"{"name":"More","run":"true"}"#).0, 409);
        assert_eq!(call(port, &tok, "PUT", "/api/commands/aaaa0001", r#"{"name":"Fixed","run":"false"}"#).0, 409);
        assert_eq!(call(port, &tok, "DELETE", "/api/commands/aaaa0001", "").0, 409);
        assert!(!dir.join("commands.json").exists());
        assert_eq!(call(port, &tok, "POST", "/api/commands/aaaa0001/run", "").0, 200);

        write(false);
        let (_, st) = call(port, &tok, "GET", "/api/state", "");
        assert_eq!((&perm(&st, "command.run")["fixed"], &perm(&st, "media.info")["fixed"], &perm(&st, "media.info")["on"]), (&json!(true), &json!(false), &json!(true)));
        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"command.run","on":false}"#).0, 409);
        assert_eq!(call(port, &tok, "POST", "/api/decks/ab12/perms", r#"{"id":"media.info","on":false}"#).0, 200);
        let (status, r) = call(port, &tok, "POST", "/api/commands", r#"{"name":"More","run":"true"}"#);
        assert_eq!(status, 200);
        let more = r["command"]["id"].as_str().unwrap().to_string();
        assert_eq!(call(port, &tok, "POST", "/api/commands", r#"{"name":"Fixed","run":"true"}"#).0, 400);
        assert_eq!(call(port, &tok, "DELETE", "/api/commands/aaaa0001", "").0, 409);
        let (_, st) = call(port, &tok, "GET", "/api/state", "");
        let names: Vec<(&str, bool)> = st["commands"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap(), c["managed"].as_bool().unwrap())).collect();
        assert_eq!(names, [("Fixed", true), ("More", false)]);
        assert_eq!(call(port, &tok, "DELETE", &format!("/api/commands/{more}"), "").0, 200);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 웹 콘솔 페어링: 코드 표시 → 확인 → 덱 수락 후 저장. 페어링 모드가 아니면 실패 사유 반환.
    #[test]
    fn pairing_from_the_screen_follows_the_same_steps_as_the_cli() {
        let (dir, port, tok) = console("pair");
        let phase = |want: &str| {
            let until = std::time::Instant::now() + Duration::from_secs(8);
            loop {
                let p = call(port, &tok, "GET", "/api/state", "").1["pairing"].clone();
                if p["phase"] == want {
                    return p;
                }
                assert!(std::time::Instant::now() < until, "{want} 단계로 전환되지 않음: {p}");
                thread::sleep(Duration::from_millis(30));
            }
        };
        let deck_id = Identity::generate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let accept = |status: &str| {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
            let (conn, mut reader) = Conn::establish(stream, &deck_id, false).unwrap();
            let Some(HostMsg::Hello { pair, caps, .. }) = conn.recv::<HostMsg>(&mut reader).unwrap() else { panic!("hello 메시지가 아님") };
            assert!(pair && caps.is_empty());
            conn.send(&DeckMsg::Hello { proto: toss_proto::PROTO_VERSION, name: "new deck".into(), status: status.into(), addrs: vec!["10.0.0.7".into()], wake: "aa:bb:cc:dd:ee:ff".into() }).unwrap();
            (conn, reader)
        };

        assert_eq!(call(port, &tok, "POST", "/api/pair", &json!({ "addr": addr }).to_string()).0, 200);
        let (conn, _reader) = accept(toss_proto::STATUS_DENIED);
        conn.send(&DeckMsg::Denied { reason: toss_proto::DENIED_NOT_PAIRED.into() }).unwrap();
        assert!(phase("failed")["error"].as_str().unwrap().contains("페어링 모드"));
        assert_eq!(call(port, &tok, "POST", "/api/pair/cancel", "").0, 200);
        phase("idle");

        assert_eq!(call(port, &tok, "POST", "/api/pair", &json!({ "addr": addr }).to_string()).0, 200);
        let (conn, mut reader) = accept(STATUS_PAIRING);
        let p = phase("code");
        assert_eq!((p["code"].as_str().unwrap(), p["name"].as_str().unwrap()), (conn.code.as_str(), "new deck"));
        assert_eq!(call(port, &tok, "POST", "/api/pair", &json!({ "addr": addr }).to_string()).0, 409);
        assert_eq!(call(port, &tok, "POST", "/api/pair/confirm", "").0, 200);
        loop {
            match conn.recv::<HostMsg>(&mut reader).unwrap() {
                Some(HostMsg::PairConfirm) => break,
                Some(HostMsg::Ping) | None => {}
                Some(other) => panic!("pair_confirm이 아님: {other:?}"),
            }
        }
        phase("waiting");
        assert!(Store::open(Some(dir.clone())).unwrap().decks().is_empty(), "덱 완료 전에 저장되어 버림");
        conn.send(&DeckMsg::Paired).unwrap();
        phase("done");
        let decks = Store::open(Some(dir.clone())).unwrap().decks();
        assert_eq!((decks.len(), decks[0].key.as_str(), decks[0].name.as_str(), decks[0].wake.as_str()), (1, deck_id.id().as_str(), "new deck", "aa:bb:cc:dd:ee:ff"));
        assert!(decks[0].addrs.contains(&"10.0.0.7".to_string()) && decks[0].perms.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
