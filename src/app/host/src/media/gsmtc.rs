//! GSMTC(GlobalSystemMediaTransportControls): Windows가 미디어 키·볼륨 플라이아웃에 쓰는 시스템 미디어 세션 목록.
//! Spotify, 브라우저(Chrome, Edge, Firefox), 미디어 플레이어 앱 등이 여기에 세션을 등록한다.
//!
//! 재생 중인 세션을 우선 선택하고, 없으면 직전 선택, Windows가 정한 현재 세션, 첫 번째 세션 순으로 고른다.
//! 세션 목록 변경과 선택한 세션의 재생 상태·곡 정보·타임라인 변경 이벤트를 구독해 on_change 콜백을 호출하며, 실제 데이터는 poll에서 다시 읽는다.
//!
//! 타임라인의 위치는 앱이 마지막으로 보고한 시각(LastUpdatedTime) 기준 값이므로, 재생 중이면 그 뒤로 흐른 시간만큼 더해 현재 위치를 구한다.
//! 곡 길이를 보고하지 않는 앱은 위치도 믿을 수 없으므로 rate를 0으로 보내 덱이 시간을 진행하지 않게 한다.
//!
//! 앨범 아트는 URL이 아니라 이미지 스트림으로 주어진다. 곡이 바뀔 때만 읽어 임시 폴더에 내용 해시 이름으로 저장하고
//! 그 파일의 file:// URL을 넘긴다(아트 ID가 URL 해시이므로 이미지가 바뀌면 ID도 바뀐다).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use toss_proto::{
    Media, ACTION_MEDIA_LOOP, ACTION_MEDIA_NEXT, ACTION_MEDIA_PLAY_PAUSE, ACTION_MEDIA_PREV, ACTION_MEDIA_SEEK, ACTION_MEDIA_SHUFFLE, LOOP_NONE,
    LOOP_PLAYLIST, LOOP_TRACK,
};
use windows::core::{Interface, HSTRING};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::MediaPlaybackAutoRepeatMode as Repeat;
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows_future::IAsyncOperation;

use super::{MediaControl, Notify, Now};

/// 앨범 아트 원본의 최대 크기(art.rs의 RAW_MAX와 같은 한도)
const THUMB_MAX: u64 = 16 * 1024 * 1024;
/// 임시 폴더에 남겨 둘 앨범 아트 파일 수(덱이 아직 받아 가지 않은 이전 곡 파일을 지우지 않도록 몇 개 유지)
const THUMBS_KEPT: usize = 4;

type Shared = Arc<Mutex<Option<Notify>>>;

/// 이벤트를 구독 중인 세션과 해제용 토큰
struct Watched {
    app: String,
    session: Session,
    tokens: [i64; 3],
}

pub struct Gsmtc {
    manager: Manager,
    /// 현재 추적 중인 세션의 앱 ID(AUMID)
    current: Option<String>,
    notify: Shared,
    watched: Option<Watched>,
    /// 마지막으로 읽은 곡(앱, 제목, 아티스트, 앨범)과 그 앨범 아트 URL
    thumb: Option<(String, String)>,
}

impl Gsmtc {
    pub fn open() -> windows::core::Result<Self> {
        // 이 스레드를 MTA로 둔다(이미 다른 모드로 초기화되어 있으면 실패해도 그대로 쓴다)
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let manager = Manager::RequestAsync()?.join()?;
        let notify: Shared = Arc::default();
        let n = notify.clone();
        manager.SessionsChanged(&TypedEventHandler::new(move |_, _| {
            fire(&n);
            Ok(())
        }))?;
        let n = notify.clone();
        manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            fire(&n);
            Ok(())
        }))?;
        Ok(Self { manager, current: None, notify, watched: None, thumb: None })
    }

    /// 활성 세션 선택 우선순위: 재생 중 > 직전 선택 > Windows의 현재 세션 > 첫 번째 세션
    fn pick(&mut self) -> Option<Session> {
        let sessions: Vec<Session> = self.manager.GetSessions().ok()?.into_iter().collect();
        let app = |s: &Session| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
        let playing = sessions.iter().find(|s| s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).is_ok_and(|st| st == Status::Playing));
        let chosen = playing
            .or_else(|| self.current.as_ref().and_then(|c| sessions.iter().find(|s| app(s) == *c)))
            .cloned()
            .or_else(|| self.manager.GetCurrentSession().ok())
            .or_else(|| sessions.first().cloned());
        self.current = chosen.as_ref().map(app);
        self.watch(chosen.as_ref());
        chosen
    }

    /// 선택한 세션의 변경 이벤트만 구독한다(세션이 바뀌면 이전 구독을 해제한다)
    fn watch(&mut self, session: Option<&Session>) {
        let app = session.and_then(|s| s.SourceAppUserModelId().ok()).map(|h| h.to_string());
        if self.watched.as_ref().map(|w| &w.app) == app.as_ref() {
            return;
        }
        if let Some(w) = self.watched.take() {
            let _ = w.session.RemovePlaybackInfoChanged(w.tokens[0]);
            let _ = w.session.RemoveMediaPropertiesChanged(w.tokens[1]);
            let _ = w.session.RemoveTimelinePropertiesChanged(w.tokens[2]);
        }
        let (Some(session), Some(app)) = (session, app) else { return };
        let (a, b, c) = (self.notify.clone(), self.notify.clone(), self.notify.clone());
        let tokens = (|| -> windows::core::Result<[i64; 3]> {
            Ok([
                session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                    fire(&a);
                    Ok(())
                }))?,
                session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    fire(&b);
                    Ok(())
                }))?,
                session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    fire(&c);
                    Ok(())
                }))?,
            ])
        })();
        if let Ok(tokens) = tokens {
            self.watched = Some(Watched { app, session: session.clone(), tokens });
        }
    }

    /// 곡이 바뀌었을 때만 앨범 아트를 읽어 임시 파일로 저장하고 그 URL을 돌려준다
    fn art_url(&mut self, key: String, session_props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> String {
        if let Some((k, url)) = &self.thumb {
            if *k == key {
                return url.clone();
            }
        }
        let url = read_thumb(session_props).and_then(|bytes| save_thumb(&bytes)).unwrap_or_default();
        // 앱이 곡 정보를 먼저 바꾸고 아트를 나중에 채우는 경우가 있어, 아트가 없으면 기억하지 않고 다음 poll에 다시 읽는다
        if !url.is_empty() {
            self.thumb = Some((key, url.clone()));
        }
        url
    }
}

fn fire(notify: &Shared) {
    if let Some(f) = notify.lock().unwrap().as_ref() {
        f();
    }
}

/// 앱 ID(AUMID)를 짧은 플레이어 이름으로 바꾼다.
///   `Spotify.exe` → `spotify`, `SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify` → `spotify`,
///   `AyuGram.AyuGramDesktop.a7a3f65f…`(점으로 나뉜 ID) → 해시 조각을 뺀 마지막 조각 `ayugramdesktop`
fn player_name(app: &str) -> String {
    let name = app.rsplit('!').next().unwrap_or(app);
    let name = name.rsplit(['\\', '/']).next().unwrap_or(name).to_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    // Firefox는 설치 경로의 해시를 앱 ID로 쓴다(기본 설치 경로의 값)
    if name == "308046b0af4a39cb" {
        return "firefox".to_string();
    }
    let hash = |s: &str| s.len() >= 16 && s.bytes().all(|b| b.is_ascii_hexdigit());
    name.split('.').filter(|s| !s.is_empty() && !hash(s)).last().unwrap_or(name).to_string()
}

/// 지금 시각(1601-01-01부터 100ns 단위, WinRT DateTime과 같은 기준)
fn now_ticks() -> i64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

fn read_thumb(props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<Vec<u8>> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()?;
    if size == 0 || size > THUMB_MAX {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
    let loaded = reader.LoadAsync(size as u32).ok()?.cast::<IAsyncOperation<u32>>().ok()?.join().ok()?;
    let mut bytes = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

fn thumb_dir() -> PathBuf {
    std::env::temp_dir().join("toss-connector-art")
}

/// 이미지를 내용 해시 이름으로 저장하고 file:// URL을 돌려준다. 오래된 파일은 THUMBS_KEPT개만 남기고 지운다.
fn save_thumb(bytes: &[u8]) -> Option<String> {
    let dir = thumb_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let path = dir.join(format!("{h:016x}.img"));
    if !path.exists() {
        std::fs::write(&path, bytes).ok()?;
    }
    let mut old: Vec<_> = std::fs::read_dir(&dir).ok()?.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    old.sort();
    let extra = old.len().saturating_sub(THUMBS_KEPT);
    for (_, p) in old.into_iter().take(extra).filter(|(_, p)| *p != path) {
        let _ = std::fs::remove_file(p);
    }
    Some(file_url(&path))
}

/// Windows 경로를 file:///C:/... 형식의 URL로 바꾼다(공백 등은 퍼센트 인코딩)
fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"/:._-~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 비동기 제어 요청 결과(앱이 거절하면 false)를 오류로 바꾼다
fn done(op: windows::core::Result<IAsyncOperation<bool>>) -> Result<(), String> {
    match op.and_then(|o| o.join()) {
        Ok(true) => Ok(()),
        Ok(false) => Err("플레이어가 요청을 거절했습니다".to_string()),
        Err(e) => Err(e.message()),
    }
}

impl MediaControl for Gsmtc {
    fn poll(&mut self) -> Option<Now> {
        let session = self.pick()?;
        let app = session.SourceAppUserModelId().ok()?.to_string();
        let info = session.GetPlaybackInfo().ok()?;
        let status = match info.PlaybackStatus().ok()? {
            Status::Playing => "playing",
            Status::Paused => "paused",
            _ => "stopped",
        };
        let props = session.TryGetMediaPropertiesAsync().ok().and_then(|o| o.join().ok());
        let text = |f: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> windows::core::Result<HSTRING>| {
            props.as_ref().and_then(|p| f(p).ok()).map(|h| h.to_string()).unwrap_or_default()
        };
        let (title, artist, album) = (text(|p| p.Title()), text(|p| p.Artist()), text(|p| p.AlbumTitle()));
        let art_url = match &props {
            Some(p) => self.art_url(format!("{app}\n{title}\n{artist}\n{album}"), p),
            None => String::new(),
        };

        let rate = info.PlaybackRate().ok().and_then(|r| r.Value().ok()).filter(|r| *r > 0.0).unwrap_or(1.0);
        let (mut length_ms, mut position_ms, mut flowing) = (0u64, 0u64, false);
        if let Ok(t) = session.GetTimelineProperties() {
            let (start, end, pos) = (t.StartTime().map_or(0, |v| v.Duration), t.EndTime().map_or(0, |v| v.Duration), t.Position().map_or(0, |v| v.Duration));
            if end > start {
                let mut ticks = pos - start;
                let updated = t.LastUpdatedTime().map_or(0, |d| d.UniversalTime);
                if status == "playing" && updated > 0 {
                    ticks += ((now_ticks() - updated).max(0) as f64 * rate) as i64;
                }
                length_ms = ((end - start) / 10_000) as u64;
                position_ms = (ticks.clamp(0, end - start) / 10_000) as u64;
                flowing = true;
            }
        }
        let controls = info.Controls().ok();
        let can = |f: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> windows::core::Result<bool>| {
            controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false)
        };
        Some(Now {
            media: Media {
                player: player_name(&app),
                status: status.to_string(),
                title,
                artist,
                album,
                length_ms,
                position_ms,
                rate: if flowing { rate } else { 0.0 },
                can_next: can(|c| c.IsNextEnabled()),
                can_prev: can(|c| c.IsPreviousEnabled()),
                can_seek: flowing && can(|c| c.IsPlaybackPositionEnabled()),
                shuffle: if can(|c| c.IsShuffleEnabled()) { info.IsShuffleActive().ok().and_then(|r| r.Value().ok()) } else { None },
                loop_mode: if can(|c| c.IsRepeatEnabled()) {
                    info.AutoRepeatMode().ok().and_then(|r| r.Value().ok()).map(|m| {
                        match m {
                            Repeat::Track => LOOP_TRACK,
                            Repeat::List => LOOP_PLAYLIST,
                            _ => LOOP_NONE,
                        }
                        .to_string()
                    })
                } else {
                    None
                },
                art: String::new(),
            },
            art_url,
        })
    }

    fn action(&mut self, name: &str, args: &serde_json::Value) -> Result<(), String> {
        let session = match &self.watched {
            Some(w) => w.session.clone(),
            None => self.pick().ok_or("활성 플레이어가 없습니다")?,
        };
        match name {
            ACTION_MEDIA_PLAY_PAUSE => done(session.TryTogglePlayPauseAsync()),
            ACTION_MEDIA_NEXT => done(session.TrySkipNextAsync()),
            ACTION_MEDIA_PREV => done(session.TrySkipPreviousAsync()),
            ACTION_MEDIA_SEEK => {
                let offset_ms = args["offset_ms"].as_i64().ok_or("offset_ms 인자가 없습니다")?.clamp(-3_600_000, 3_600_000);
                let now = self.poll().ok_or("활성 플레이어가 없습니다")?.media;
                if now.length_ms == 0 {
                    return Err("이 플레이어는 위치 이동을 지원하지 않습니다".to_string());
                }
                // 곡 범위를 벗어나면 앱마다 동작이 달라서(다음 곡으로 넘어가는 등) 곡 끝 0.5초 전까지로 제한한다
                let target = (now.position_ms as i64 + offset_ms).clamp(0, (now.length_ms as i64 - 500).max(0));
                let start = session.GetTimelineProperties().and_then(|t| t.StartTime()).map_or(0, |v| v.Duration);
                done(session.TryChangePlaybackPositionAsync(start + target * 10_000))
            }
            ACTION_MEDIA_SHUFFLE => {
                let on = args["on"].as_bool().ok_or("on 인자가 없습니다")?;
                done(session.TryChangeShuffleActiveAsync(on))
            }
            ACTION_MEDIA_LOOP => {
                let mode = match args["mode"].as_str() {
                    Some(LOOP_NONE) => Repeat::None,
                    Some(LOOP_PLAYLIST) => Repeat::List,
                    Some(LOOP_TRACK) => Repeat::Track,
                    _ => return Err("mode는 none, playlist, track 중 하나여야 합니다".to_string()),
                };
                done(session.TryChangeAutoRepeatModeAsync(mode))
            }
            other => Err(format!("알 수 없는 액션: {other}")),
        }
    }

    fn on_change(&mut self, notify: Option<Notify>) {
        *self.notify.lock().unwrap() = notify;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_become_short_player_names() {
        assert_eq!(player_name("Spotify.exe"), "spotify");
        assert_eq!(player_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "spotify");
        assert_eq!(player_name("MSEdge"), "msedge");
        assert_eq!(player_name(r"C:\Program Files\VideoLAN\VLC\vlc.exe"), "vlc");
        assert_eq!(player_name("AyuGram.AyuGramDesktop.a7a3f65ff334927679c33dd600e3b42a"), "ayugramdesktop");
        assert_eq!(player_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "zunemusic");
        assert_eq!(player_name("308046B0AF4A39CB"), "firefox");
    }

    /// 이 PC의 실제 미디어 세션을 읽어 출력한다: `cargo test show_this_pc -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn show_this_pc() {
        let mut g = Gsmtc::open().unwrap();
        let now = g.poll();
        println!("{now:#?}");
        if let Some(n) = now.filter(|n| !n.art_url.is_empty()) {
            let art = crate::art::fetch(&n.art_url).unwrap();
            println!("앨범 아트: {} 바이트 JPEG", art.bytes.len());
        }
    }

    #[test]
    fn paths_become_file_urls_that_art_can_open() {
        let url = file_url(std::path::Path::new(r"C:\Users\홍 길동\AppData\Local\Temp\toss-connector-art\0123.img"));
        assert!(url.starts_with("file:///C:/Users/%ED%99%8D%20"), "{url}");
        assert!(url.ends_with("/toss-connector-art/0123.img"), "{url}");
    }
}
