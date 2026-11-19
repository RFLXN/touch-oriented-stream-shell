//! Spotify 연동: 덱이 Spotify Web API로 그 계정에서 지금 재생 중인 곡을 직접 조회하고 재생을 제어한다(PC를 거치지 않는다).
//!
//! 키와 토큰은 덱 금고(`hosts/vault.rs`)의 `spotify` 서비스에 있다. PC(웹 콘솔)는 앱 키를 저장하고 브라우저 로그인으로 받은
//! 승인 코드를 덱에 넘기기만 한다. 이 모듈은 금고에서 토큰을 얻어(`Link::token`) API를 부르고, 토큰이 거부되면 한 번 갱신해 다시 부른다.
//!
//! 미디어 플레이어 위젯의 대상이 `SOURCE`(`"@spotify"`)인 위젯이 배치에 있을 때만 조회한다(`Cmd::Want`).
//! Spotify는 상태 변경을 알려 주지 않으므로 주기적으로 묻는다: 재생 중이면 `PLAYING_EVERY`(곡이 그보다 먼저 끝나면 끝난 직후),
//! 일시정지면 `PAUSED_EVERY`, 재생 기기가 없으면 `IDLE_EVERY`, 오류면 `ERROR_EVERY`. 요청 제한(429)에 걸리면 `Retry-After`만큼 쉰다.
//! 제어 후에는 `AFTER_ACTION` 뒤에 다시 조회한다. 재생 위치는 조회 사이에 UI가 스스로 진행시키므로, 곡·상태가 같고 위치가 예상과
//! `DRIFT_MS` 안으로 맞으면 UI에 다시 보내지 않는다(앨범 아트가 든 상태를 매번 보내지 않기 위함). 다만 `RESEND_AFTER`마다는 보낸다.
//!
//! 재생 제어(`media.*` 액션)는 Spotify Premium 계정에서만 된다. 실패하면 위젯 앞으로 결과를 남긴다(`Events::result`와 같은 경로).
//! 앨범 아트는 곡이 바뀔 때만 받아 `data:` URI용 base64로 보관한다(`ART_MAX`보다 크면 받지 않는다).

use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use toss_proto::{Media, ACTION_MEDIA_LOOP, ACTION_MEDIA_NEXT, ACTION_MEDIA_PLAY_PAUSE, ACTION_MEDIA_PREV, ACTION_MEDIA_SEEK, ACTION_MEDIA_SHUFFLE, LOOP_NONE, LOOP_PLAYLIST, LOOP_TRACK};

use crate::hosts::Art;

/// 미디어 위젯 설정의 `host`에 넣는 Spotify 대상 값
pub const SOURCE: &str = "@spotify";
const API: &str = "https://api.spotify.com/v1";

const PLAYING_EVERY: Duration = Duration::from_secs(3);
const PAUSED_EVERY: Duration = Duration::from_secs(6);
const IDLE_EVERY: Duration = Duration::from_secs(10);
const ERROR_EVERY: Duration = Duration::from_secs(15);
const UNLINKED_EVERY: Duration = Duration::from_secs(5);
const AFTER_ACTION: Duration = Duration::from_millis(400);
const LIMITED_MAX: Duration = Duration::from_secs(120);
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const ART_MAX: u64 = 2 * 1024 * 1024;
const DRIFT_MS: u64 = 2000;
/// 바뀐 것이 없어도 이만큼 지나면 다시 보낸다(나중에 연결한 UI가 오래된 재생 위치를 받지 않게)
const RESEND_AFTER: Duration = Duration::from_secs(30);

/// UI에 보내는 Spotify 상태
#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct SpotifyState {
    /// 덱 금고에 승인된 Spotify 계정이 있는지 여부
    pub linked: bool,
    /// 마지막 조회가 성공했는지 여부
    pub live: bool,
    /// 재생 중이거나 일시정지한 곡(재생 기기가 없으면 `null`). `position_ms`는 보낸 시점 기준이다
    pub media: Option<Media>,
    pub art: Option<Art>,
    /// 마지막 조회가 실패한 사유(화면에 보여 줄 짧은 영어 문장)
    pub error: String,
}

/// 작업 스레드에 보내는 요청
pub enum Cmd {
    /// 배치에 Spotify 미디어 위젯이 있는지(있을 때만 조회한다)
    Want(bool),
    /// 위젯이 요청한 재생 제어
    Act { name: String, args: Value, widget: Option<u32> },
}

/// 덱 금고와의 연결: 토큰 얻기(`true`면 보관한 토큰을 버리고 갱신)와 승인 여부
pub struct Link {
    pub token: Box<dyn Fn(bool) -> Result<String, String> + Send>,
    pub linked: Box<dyn Fn() -> bool + Send>,
}

enum Fail {
    /// 요청 제한: 이만큼 쉬고 다시 묻는다
    Limited(Duration),
    /// 승인이 없거나 갱신이 거절됨
    Unlinked,
    Failed(String),
}

/// 작업 스레드를 시작한다. `publish`는 UI 상태가 바뀔 때, `result`는 위젯의 제어 요청 결과가 나올 때 호출된다.
pub fn spawn(link: Link, publish: impl Fn(SpotifyState) + Send + 'static, result: impl Fn(u32, bool, String) + Send + 'static) -> Sender<Cmd> {
    spawn_with(API.into(), link, publish, result)
}

fn spawn_with(api: String, link: Link, publish: impl Fn(SpotifyState) + Send + 'static, result: impl Fn(u32, bool, String) + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut w = Worker::new(api, link, Box::new(publish));
        let mut want = false;
        let mut next = Instant::now();
        loop {
            let wait = if want { next.saturating_duration_since(Instant::now()) } else { UNLINKED_EVERY };
            match rx.recv_timeout(wait) {
                Ok(Cmd::Want(on)) => {
                    if on && !want {
                        next = Instant::now();
                    }
                    want = on;
                    if !want {
                        w.idle();
                    }
                    continue;
                }
                Ok(Cmd::Act { name, args, widget }) => {
                    let done = w.act(&name, &args);
                    if let Err(why) = &done {
                        eprintln!("spotify: {name} 실패: {why}");
                    }
                    if let Some(widget) = widget {
                        result(widget, done.is_ok(), done.err().unwrap_or_default());
                    }
                    next = Instant::now() + AFTER_ACTION;
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if want {
                next = Instant::now() + w.poll();
            } else {
                w.idle();
            }
        }
    });
    tx
}

struct Worker {
    api: String,
    agent: ureq::Agent,
    link: Link,
    publish: Box<dyn Fn(SpotifyState) + Send>,
    /// 마지막으로 UI에 보낸 상태와 보낸 시각
    shown: Option<(SpotifyState, Instant)>,
    /// 지금 곡의 상태와 조회 시각(제어할 때 현재 위치를 계산하는 기준)
    now: Option<(Media, Instant)>,
    art_url: String,
    art: Option<Art>,
}

impl Worker {
    fn new(api: String, link: Link, publish: Box<dyn Fn(SpotifyState) + Send>) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).user_agent(concat!("tossd/", env!("CARGO_PKG_VERSION"))).build();
        Self { api, agent, link, publish, shown: None, now: None, art_url: String::new(), art: None }
    }

    /// 조회하지 않는 동안: 승인 여부만 알린다
    fn idle(&mut self) {
        self.now = None;
        let linked = (self.link.linked)();
        self.show(SpotifyState { linked, ..SpotifyState::default() });
    }

    /// 지금 상태를 조회해 알리고, 다음 조회까지 기다릴 시간을 돌려준다
    fn poll(&mut self) -> Duration {
        if !(self.link.linked)() {
            self.now = None;
            self.show(SpotifyState::default());
            return UNLINKED_EVERY;
        }
        match self.call("GET", "/me/player", &[("additional_types", "track,episode".into())]) {
            Ok(Some(v)) => match media_from(&v) {
                Some((media, art_url)) => {
                    self.fetch_art(&art_url);
                    let mut media = media;
                    media.art = self.art.as_ref().filter(|_| !art_url.is_empty()).map(|a| a.id.clone()).unwrap_or_default();
                    let wait = if media.status == "playing" {
                        let left = media.length_ms.saturating_sub(media.position_ms);
                        PLAYING_EVERY.min(Duration::from_millis(left + 600))
                    } else {
                        PAUSED_EVERY
                    };
                    self.now = Some((media.clone(), Instant::now()));
                    let art = self.art.clone().filter(|_| !media.art.is_empty());
                    self.show(SpotifyState { linked: true, live: true, media: Some(media), art, error: String::new() });
                    wait
                }
                None => self.nothing(),
            },
            Ok(None) => self.nothing(),
            Err(Fail::Limited(wait)) => wait,
            Err(Fail::Unlinked) => {
                self.now = None;
                self.show(SpotifyState::default());
                UNLINKED_EVERY
            }
            Err(Fail::Failed(why)) => {
                self.now = None;
                self.show(SpotifyState { linked: true, error: why, ..SpotifyState::default() });
                ERROR_EVERY
            }
        }
    }

    /// 재생 기기가 없거나 아무것도 재생하지 않는 상태
    fn nothing(&mut self) -> Duration {
        self.now = None;
        self.show(SpotifyState { linked: true, live: true, ..SpotifyState::default() });
        IDLE_EVERY
    }

    /// 곡, 재생 상태, 아트가 같고 위치만 예상대로 흘렀으면 다시 보내지 않는다
    fn show(&mut self, state: SpotifyState) {
        if let Some((last, at)) = &self.shown {
            if at.elapsed() < RESEND_AFTER && same_but_time(last, &state, at.elapsed().as_millis() as u64) {
                return;
            }
        }
        self.shown = Some((state.clone(), Instant::now()));
        (self.publish)(state);
    }

    fn fetch_art(&mut self, url: &str) {
        if url.is_empty() || url == self.art_url {
            return;
        }
        self.art_url = url.to_string();
        self.art = match self.agent.get(url).call() {
            Ok(res) => {
                let mime = res.content_type().to_string();
                let mut bytes = Vec::new();
                match res.into_reader().take(ART_MAX + 1).read_to_end(&mut bytes) {
                    Ok(n) if n as u64 <= ART_MAX && mime.starts_with("image/") => {
                        Some(Art { id: art_id(url), mime, data: base64::engine::general_purpose::STANDARD.encode(&bytes) })
                    }
                    _ => None,
                }
            }
            Err(e) => {
                eprintln!("spotify: 앨범 아트를 받을 수 없습니다: {e}");
                None
            }
        };
    }

    /// 위젯의 제어 요청을 실행한다. 실패하면 화면에 보여 줄 짧은 사유를 돌려준다.
    fn act(&mut self, name: &str, args: &Value) -> Result<(), String> {
        let now = self.now.as_ref().map(|(m, at)| (m, m.position_after(at.elapsed().as_millis() as u64)));
        let (method, path, query) = request_for(name, args, now)?;
        match self.call(method, path, &query) {
            Ok(_) => Ok(()),
            Err(Fail::Limited(_)) => Err("Spotify is busy. Try again shortly".into()),
            Err(Fail::Unlinked) => Err("Spotify is not linked".into()),
            Err(Fail::Failed(why)) => Err(why),
        }
    }

    /// API를 부른다. 토큰이 거부되면(401) 한 번 갱신해 다시 부른다. 본문이 없는 응답이면 `None`
    fn call(&self, method: &str, path: &str, query: &[(&str, String)]) -> Result<Option<Value>, Fail> {
        for stale in [false, true] {
            let token = (self.link.token)(stale).map_err(|e| match e.as_str() {
                toss_proto::VAULT_ERR_NOT_LINKED | toss_proto::VAULT_ERR_NOT_SET => Fail::Unlinked,
                _ => Fail::Failed(format!("Token: {e}")),
            })?;
            let mut req = self.agent.request(method, &format!("{}{path}", self.api)).set("Authorization", &format!("Bearer {token}"));
            for (k, v) in query {
                req = req.query(k, v);
            }
            let res = if method == "GET" { req.call() } else { req.send_string("") };
            match res {
                Ok(res) if res.status() == 204 => return Ok(None),
                Ok(res) => return Ok(res.into_string().ok().and_then(|t| serde_json::from_str(&t).ok())),
                Err(ureq::Error::Status(401, _)) if !stale => continue,
                Err(ureq::Error::Status(429, res)) => {
                    let secs = res.header("Retry-After").and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(5);
                    eprintln!("spotify: 요청 제한에 걸림. {secs}초 뒤에 다시 묻습니다");
                    return Err(Fail::Limited(Duration::from_secs(secs.max(1)).min(LIMITED_MAX)));
                }
                Err(ureq::Error::Status(code, res)) => {
                    let body: Value = res.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
                    return Err(Fail::Failed(explain(code, &body)));
                }
                Err(e) => return Err(Fail::Failed(format!("Cannot reach Spotify ({})", e.kind()))),
            }
        }
        Err(Fail::Failed("Spotify refused the token".into()))
    }
}

/// API 오류 응답을 화면에 보여 줄 짧은 문장으로 바꾼다
fn explain(code: u16, body: &Value) -> String {
    match body["error"]["reason"].as_str().unwrap_or_default() {
        "PREMIUM_REQUIRED" => "Spotify Premium required".into(),
        "NO_ACTIVE_DEVICE" => "No active Spotify device".into(),
        _ => match body["error"]["message"].as_str().filter(|m| !m.is_empty()) {
            Some(m) => format!("Spotify: {m}"),
            None => format!("Spotify: HTTP {code}"),
        },
    }
}

/// 앨범 아트 주소에서 짧은 ID를 만든다(FNV-1a)
fn art_id(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("sp-{h:016x}")
}

/// `/me/player` 응답을 재생 정보와 앨범 아트 주소로 바꾼다(곡이 없으면 `None`)
fn media_from(v: &Value) -> Option<(Media, String)> {
    let item = &v["item"];
    if !item.is_object() {
        return None;
    }
    let episode = item["type"] == "episode";
    let names = |list: &Value| list.as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let (artist, album, images) = if episode {
        (item["show"]["name"].as_str().unwrap_or_default().to_string(), String::new(), if item["images"].is_array() { &item["images"] } else { &item["show"]["images"] })
    } else {
        (names(&item["artists"]), item["album"]["name"].as_str().unwrap_or_default().to_string(), &item["album"]["images"])
    };
    let off = |what: &str| v["actions"]["disallows"][what].as_bool().unwrap_or(false);
    let length_ms = item["duration_ms"].as_u64().unwrap_or(0);
    let media = Media {
        player: "spotify".into(),
        status: if v["is_playing"].as_bool().unwrap_or(false) { "playing" } else { "paused" }.into(),
        title: item["name"].as_str().unwrap_or_default().into(),
        artist,
        album,
        length_ms,
        position_ms: v["progress_ms"].as_u64().unwrap_or(0).min(length_ms.max(1)),
        rate: 1.0,
        can_next: !off("skipping_next"),
        can_prev: !off("skipping_prev"),
        can_seek: !off("seeking"),
        shuffle: v["shuffle_state"].as_bool().filter(|_| !off("toggling_shuffle")),
        loop_mode: match v["repeat_state"].as_str() {
            _ if off("toggling_repeat_context") && off("toggling_repeat_track") => None,
            Some("context") => Some(LOOP_PLAYLIST.into()),
            Some("track") => Some(LOOP_TRACK.into()),
            Some(_) => Some(LOOP_NONE.into()),
            None => None,
        },
        art: String::new(),
    };
    Some((media, pick_image(images)))
}

/// 위젯에 맞는 크기(가로 200px 이상 중 가장 작은 것)의 이미지 주소. 없으면 가장 큰 것
fn pick_image(images: &Value) -> String {
    let list: Vec<(u64, &str)> = images.as_array().map(|a| a.iter().filter_map(|i| Some((i["width"].as_u64().unwrap_or(0), i["url"].as_str()?))).collect()).unwrap_or_default();
    list.iter().filter(|(w, _)| *w >= 200).min_by_key(|(w, _)| *w).or_else(|| list.iter().max_by_key(|(w, _)| *w)).map(|(_, u)| u.to_string()).unwrap_or_default()
}

type Request = (&'static str, &'static str, Vec<(&'static str, String)>);

/// 위젯 액션을 API 요청(메서드, 경로, 쿼리)으로 바꾼다. `now`는 지금 곡과 현재 위치(ms)
fn request_for(name: &str, args: &Value, now: Option<(&Media, u64)>) -> Result<Request, String> {
    let playing = now.is_some_and(|(m, _)| m.status == "playing");
    Ok(match name {
        ACTION_MEDIA_PLAY_PAUSE if playing => ("PUT", "/me/player/pause", vec![]),
        ACTION_MEDIA_PLAY_PAUSE => ("PUT", "/me/player/play", vec![]),
        ACTION_MEDIA_NEXT => ("POST", "/me/player/next", vec![]),
        ACTION_MEDIA_PREV => ("POST", "/me/player/previous", vec![]),
        ACTION_MEDIA_SEEK => {
            let (m, pos) = now.ok_or("Nothing playing")?;
            let offset = args["offset_ms"].as_i64().ok_or("Bad seek offset")?;
            let to = (pos as i64 + offset).clamp(0, m.length_ms.max(1) as i64 - 1);
            ("PUT", "/me/player/seek", vec![("position_ms", to.to_string())])
        }
        ACTION_MEDIA_SHUFFLE => ("PUT", "/me/player/shuffle", vec![("state", args["on"].as_bool().ok_or("Bad shuffle value")?.to_string())]),
        ACTION_MEDIA_LOOP => {
            let state = match args["mode"].as_str() {
                Some(LOOP_NONE) => "off",
                Some(LOOP_PLAYLIST) => "context",
                Some(LOOP_TRACK) => "track",
                _ => return Err("Bad repeat mode".into()),
            };
            ("PUT", "/me/player/repeat", vec![("state", state.to_string())])
        }
        _ => return Err("Spotify cannot do that".into()),
    })
}

/// 두 상태가 재생 위치 말고는 같고, 위치도 `elapsed_ms`만큼 흐른 예상 위치에서 `DRIFT_MS` 안인지 여부
fn same_but_time(a: &SpotifyState, b: &SpotifyState, elapsed_ms: u64) -> bool {
    match (&a.media, &b.media) {
        (Some(x), Some(y)) => {
            let expected = x.position_after(elapsed_ms);
            let near = expected.abs_diff(y.position_ms) <= DRIFT_MS;
            near && Media { position_ms: 0, ..x.clone() } == Media { position_ms: 0, ..y.clone() } && (a.linked, a.live, &a.art, &a.error) == (b.linked, b.live, &b.art, &b.error)
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests;
