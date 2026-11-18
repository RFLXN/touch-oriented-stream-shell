//! 사이트 아이콘: 웹페이지 아이콘 버튼(`web.icon`)이 표시할 아이콘을 덱이 직접 가져와 보관한다(덱이 인터넷에 연결되어 있어야 한다).
//!
//! 후보를 고르는 순서
//!   1. 페이지의 `<link rel="apple-touch-icon">`(`apple-touch-icon-precomposed` 포함), `<link rel="icon">`(`shortcut icon` 포함),
//!      웹 앱 매니페스트(`<link rel="manifest">`)의 `icons`를 모은다.
//!   2. 사이트 루트의 `/apple-touch-icon.png`, `/apple-touch-icon-precomposed.png`, `/favicon.ico`를 덧붙인다.
//!   3. 크기(`sizes`)가 큰 것부터 하나씩 받아 보고, 이미지로 읽히는 첫 번째 것을 쓴다.
//!      크기를 적지 않은 항목은 `apple-touch-icon`이면 180, 루트의 `favicon.ico`와 그 밖의 항목은 32로 본다.
//!      화면이 그릴 수 있는 형식(PNG, JPEG, GIF, ICO)만 받으며 SVG와 WebP는 건너뛴다.
//!
//! 받은 아이콘은 `icons/`에 저장해 재시작하거나 인터넷이 끊겨도 바로 보이게 한다(목록은 `icons/index.json`).
//! 가져오지 못한 주소는 `RETRY_AFTER`가 지난 뒤 다시 시도하고, 배치에서 빠진 주소의 아이콘은 지운다.
//! UI에는 `주소 -> data: URI` 맵으로 전달한다(`ui_proto.rs`의 `icons`).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::persist;

/// UI로 전달하는 아이콘: 페이지 주소 -> `data:` URI
pub type IconMap = BTreeMap<String, String>;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// 가져오지 못한 주소를 다시 시도하기까지의 시간
const RETRY_AFTER: Duration = Duration::from_secs(600);
/// 다시 시도할 주소가 있는지 확인하는 주기
const TICK: Duration = Duration::from_secs(60);
/// 페이지 HTML과 매니페스트는 앞부분만 읽는다
const PAGE_MAX: u64 = 512 * 1024;
const MANIFEST_MAX: u64 = 128 * 1024;
const ICON_MAX: u64 = 512 * 1024;
/// 한 페이지에서 시도하는 후보 최대 개수
const TRIES_MAX: usize = 12;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Saved {
    file: String,
    mime: String,
}

/// 후보 아이콘: 주소와 크기(px)
#[derive(Clone, PartialEq, Debug)]
struct Candidate {
    url: String,
    size: u32,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).user_agent(concat!("tossd/", env!("CARGO_PKG_VERSION"))).build()
}

fn get(url: &str, max: u64) -> Result<(String, Vec<u8>), String> {
    let res = agent().get(url).call().map_err(|e| e.to_string())?;
    let at = res.get_url().to_string();
    let mut bytes = Vec::new();
    res.into_reader().take(max).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok((at, bytes))
}

/// `scheme://host[:port]`
fn origin(url: &str) -> Option<&str> {
    let start = url.find("://")? + 3;
    let end = url[start..].find(['/', '?', '#']).map_or(url.len(), |i| start + i);
    Some(&url[..end])
}

/// `href`를 `base` 기준의 절대 주소로 바꾼다
fn join(base: &str, href: &str) -> Option<String> {
    let href = href.trim().replace("&amp;", "&");
    if href.is_empty() || href.starts_with('#') {
        return None;
    }
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") {
        return Some(href);
    }
    if let Some(rest) = href.strip_prefix("//") {
        return Some(format!("{}{rest}", &base[..base.find("://")? + 3]));
    }
    let root = origin(base)?;
    if href.starts_with('/') {
        return Some(format!("{root}{href}"));
    }
    let path = &base[root.len()..];
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
    let dir = if dir.is_empty() { "/" } else { dir };
    Some(format!("{root}{dir}{href}"))
}

/// 태그 안의 속성들(이름은 소문자로)
fn attrs(tag: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' {
            i += 1;
        }
        let name = tag[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = tag[vs..i].to_string();
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = tag[vs..i].to_string();
            }
        }
        if !name.is_empty() {
            out.entry(name).or_insert(value);
        }
        if start == i {
            i += 1;
        }
    }
    out
}

/// `sizes` 값에서 가장 큰 변 길이. `any`(벡터)나 읽을 수 없는 값이면 `None`
fn largest(sizes: &str) -> Option<u32> {
    sizes.split_whitespace().filter_map(|s| s.split(['x', 'X']).next()?.parse::<u32>().ok()).max()
}

fn skipped_type(kind: &str, url: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    let path = url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    kind.contains("svg") || kind.contains("webp") || path.ends_with(".svg") || path.ends_with(".webp") || url.starts_with("data:image/svg")
}

/// 페이지 HTML에서 후보 아이콘과 매니페스트 주소를 찾는다
fn scan(page_url: &str, html: &str) -> (Vec<Candidate>, Option<String>) {
    let lower = html.to_ascii_lowercase();
    let end = lower.find("</head>").unwrap_or(lower.len());
    let (mut found, mut manifest) = (Vec::new(), None);
    let mut from = 0;
    while let Some(at) = lower[from..end].find("<link") {
        let start = from + at + 5;
        let Some(close) = lower[start..end].find('>') else { break };
        let a = attrs(&html[start..start + close]);
        from = start + close;
        let rel: Vec<String> = a.get("rel").map(|r| r.to_ascii_lowercase().split_whitespace().map(str::to_string).collect()).unwrap_or_default();
        let Some(url) = a.get("href").and_then(|h| join(page_url, h)) else { continue };
        if rel.iter().any(|r| r == "manifest") {
            manifest.get_or_insert(url);
            continue;
        }
        let touch = rel.iter().any(|r| r.starts_with("apple-touch-icon"));
        if !(touch || rel.iter().any(|r| r == "icon")) || skipped_type(a.get("type").map_or("", String::as_str), &url) {
            continue;
        }
        let size = a.get("sizes").and_then(|s| largest(s)).unwrap_or(if touch { 180 } else { 32 });
        found.push(Candidate { url, size });
    }
    (found, manifest)
}

/// 웹 앱 매니페스트의 `icons`
fn manifest_icons(manifest_url: &str, text: &str) -> Vec<Candidate> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    v["icons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| !i["purpose"].as_str().unwrap_or_default().contains("monochrome"))
        .filter_map(|i| {
            let url = join(manifest_url, i["src"].as_str()?)?;
            (!skipped_type(i["type"].as_str().unwrap_or_default(), &url)).then(|| Candidate { size: i["sizes"].as_str().and_then(largest).unwrap_or(32), url })
        })
        .collect()
}

/// 받은 바이트가 화면이 그릴 수 있는 이미지면 MIME 형식을 돌려준다
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [0, 0, 1, 0, n, _, ..] if *n > 0 => Some("image/x-icon"),
        _ => None,
    }
}

fn load_icon(url: &str) -> Option<(&'static str, Vec<u8>)> {
    let bytes = if let Some(data) = url.strip_prefix("data:") {
        let (_, payload) = data.split_once(";base64,")?;
        base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()?
    } else {
        get(url, ICON_MAX).ok()?.1
    };
    Some((sniff(&bytes)?, bytes))
}

/// 페이지의 아이콘을 가져온다(크기가 큰 후보부터 시도)
fn fetch(page_url: &str) -> Result<(&'static str, Vec<u8>), String> {
    let (at, html) = match get(page_url, PAGE_MAX) {
        Ok((at, bytes)) => (at, String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => (page_url.to_string(), String::new()),
    };
    let (mut found, manifest) = scan(&at, &html);
    if let Some(m) = manifest {
        if let Ok((m_at, text)) = get(&m, MANIFEST_MAX) {
            found.extend(manifest_icons(&m_at, &String::from_utf8_lossy(&text)));
        }
    }
    if let Some(root) = origin(&at) {
        found.push(Candidate { url: format!("{root}/apple-touch-icon.png"), size: 180 });
        found.push(Candidate { url: format!("{root}/apple-touch-icon-precomposed.png"), size: 180 });
        found.push(Candidate { url: format!("{root}/favicon.ico"), size: 32 });
    }
    found.sort_by_key(|c| std::cmp::Reverse(c.size));
    let mut tried: Vec<String> = Vec::new();
    for c in found {
        if tried.len() >= TRIES_MAX {
            break;
        }
        if tried.contains(&c.url) {
            continue;
        }
        tried.push(c.url.clone());
        if let Some(got) = load_icon(&c.url) {
            return Ok(got);
        }
    }
    Err("아이콘을 찾지 못했습니다".into())
}

/// 주소마다 파일 이름으로 쓸 해시(FNV-1a 64비트)
fn file_name(url: &str) -> String {
    let h = url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{h:016x}")
}

fn data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

struct Store {
    dir: Option<PathBuf>,
    index: BTreeMap<String, Saved>,
    shown: IconMap,
}

impl Store {
    fn open(dir: Option<PathBuf>) -> Self {
        let mut s = Self { dir, index: BTreeMap::new(), shown: IconMap::new() };
        let Some(dir) = s.dir.clone() else { return s };
        s.index = persist::read_json(&dir.join("index.json"), "icons", Ok).unwrap_or_default();
        s.index.retain(|url, saved| match fs::read(dir.join(&saved.file)) {
            Ok(bytes) => {
                s.shown.insert(url.clone(), data_uri(&saved.mime, &bytes));
                true
            }
            Err(_) => false,
        });
        s
    }

    fn keep(&mut self, url: &str, mime: &str, bytes: &[u8]) {
        self.shown.insert(url.to_string(), data_uri(mime, bytes));
        let Some(dir) = &self.dir else { return };
        let file = file_name(url);
        let saved = fs::create_dir_all(dir).and_then(|()| persist::write_atomic(&dir.join(&file), bytes));
        match saved {
            Ok(()) => {
                self.index.insert(url.to_string(), Saved { file, mime: mime.to_string() });
                self.save_index();
            }
            Err(e) => eprintln!("icons: 아이콘을 저장하지 못했습니다({}): {e}", dir.display()),
        }
    }

    fn drop_unwanted(&mut self, want: &[String]) -> bool {
        let before = self.shown.len();
        self.shown.retain(|url, _| want.contains(url));
        let gone: Vec<String> = self.index.keys().filter(|u| !want.contains(u)).cloned().collect();
        for url in &gone {
            if let (Some(dir), Some(saved)) = (&self.dir, self.index.remove(url)) {
                let _ = fs::remove_file(dir.join(saved.file));
            }
        }
        if !gone.is_empty() {
            self.save_index();
        }
        before != self.shown.len()
    }

    fn save_index(&self) {
        if let Some(dir) = &self.dir {
            if let Err(e) = persist::write_json(&dir.join("index.json"), &self.index) {
                eprintln!("icons: 목록 저장 실패: {e}");
            }
        }
    }
}

/// 아이콘 작업 스레드를 시작한다. 반환된 채널로 배치에 있는 주소 목록을 보내면 새 주소의 아이콘을 가져온다.
/// `dir`이 `None`이면 디스크에 보관하지 않는다. `publish`는 아이콘 맵이 바뀔 때마다 호출된다.
pub fn spawn(dir: Option<PathBuf>, publish: impl Fn(IconMap) + Send + 'static) -> Sender<Vec<String>> {
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    thread::spawn(move || {
        let mut store = Store::open(dir);
        publish(store.shown.clone());
        let mut want: Vec<String> = Vec::new();
        let mut failed: HashMap<String, Instant> = HashMap::new();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(list) => {
                    want = list;
                    if store.drop_unwanted(&want) {
                        publish(store.shown.clone());
                    }
                    failed.retain(|u, _| want.contains(u));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            for url in want.clone() {
                if store.shown.contains_key(&url) || failed.get(&url).is_some_and(|at| at.elapsed() < RETRY_AFTER) {
                    continue;
                }
                match fetch(&url) {
                    Ok((mime, bytes)) => {
                        store.keep(&url, mime, &bytes);
                        failed.remove(&url);
                        eprintln!("icons: 아이콘을 가져옴: {url} ({mime}, {}KB)", bytes.len().div_ceil(1024));
                        publish(store.shown.clone());
                    }
                    Err(why) => {
                        eprintln!("icons: {url}: {why}");
                        failed.insert(url, Instant::now());
                    }
                }
            }
        }
    });
    tx
}

/// 아이콘을 보관할 디렉터리(`data_dir/icons`)
pub fn dir_under(data_dir: &Path) -> PathBuf {
    data_dir.join("icons")
}

#[cfg(test)]
mod tests;
