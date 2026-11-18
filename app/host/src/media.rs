//! 미디어 플레이어 제어 및 상태 조회(OS별 구현 분기, 현재 리눅스 MPRIS 지원).
//!
//! 덱에는 곡 변경, 재생/일시정지, 탐색 등 실제 상태 변화가 있을 때만 전송하고,
//! 재생 중 경과 시간은 덱이 자체적으로 계산한다.
//! 전송 필요 여부는 changed()가 덱이 알고 있을 상태와 실제 상태를 비교해 판단하며,
//! 호출 시점은 session 모듈에서 상태 변경 시그널(on_change) 및 주기적 폴링으로 결정한다.

use std::time::Duration;

use toss_proto::Media;

/// 덱의 예상 재생 위치와 실제 위치가 이 값을 초과해 벌어지면 탐색(seek)으로 간주해 재전송한다.
pub const DRIFT: Duration = Duration::from_millis(1500);

/// 현재 재생 상태
#[derive(Debug, Clone, PartialEq)]
pub struct Now {
    /// art 필드는 비워 둔다(호출자가 art_url 기반으로 채움)
    pub media: Media,
    /// 앨범 아트 URL(file:// 또는 http(s)://). 없으면 빈 문자열
    pub art_url: String,
}

/// 플레이어 상태 변경 알림 콜백
pub type Notify = Box<dyn Fn() + Send + Sync>;

pub trait MediaControl {
    /// 현재 재생 상태 조회. 활성 플레이어가 없으면 None
    fn poll(&mut self) -> Option<Now>;
    /// 덱의 미디어 제어 액션 실행 (toss-proto ACTION_MEDIA_*)
    fn action(&mut self, name: &str, args: &serde_json::Value) -> Result<(), String>;
    /// 플레이어 변경 시 호출할 콜백 등록(None으로 해제).
    /// 변경 세부 내용은 전달하지 않으며 콜백이 불리면 poll로 다시 읽는다. 시그널을 지원하지 않는 OS는 no-op으로 둔다.
    fn on_change(&mut self, notify: Option<Notify>);
}

/// 덱에 미디어 상태를 재전송해야 하는지 판단한다.
/// sent는 직전 전송 상태, elapsed는 경과 시간, cur는 현재 상태다.
/// 위치 외의 필드가 바뀌었거나, 위치만 바뀐 경우 예상 위치 대비 DRIFT를 초과해 어긋났으면 true를 반환한다.
pub fn changed(sent: Option<&Media>, elapsed: Duration, cur: Option<&Media>) -> bool {
    match (sent, cur) {
        (None, None) => false,
        (None, Some(_)) | (Some(_), None) => true,
        (Some(s), Some(c)) => {
            let rest = |m: &Media| Media { position_ms: 0, ..m.clone() };
            if rest(s) != rest(c) {
                return true;
            }
            let expected = s.position_after(elapsed.as_millis() as u64);
            c.position_ms.abs_diff(expected) > DRIFT.as_millis() as u64
        }
    }
}

/// 현재 OS용 미디어 컨트롤러를 연다. 실패 시 사유를 반환하며 이 경우 미디어 기능 없이 연결한다.
pub fn open() -> Result<Box<dyn MediaControl>, String> {
    #[cfg(target_os = "linux")]
    {
        mpris::Mpris::open().map(|m| Box::new(m) as Box<dyn MediaControl>).map_err(|e| format!("D-Bus 세션에서 플레이어에 연결할 수 없습니다: {e}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("현재 OS의 미디어 제어는 아직 지원되지 않습니다".to_string())
    }
}

#[cfg(target_os = "linux")]
mod mpris {
    //! MPRIS: 리눅스 미디어 플레이어가 D-Bus 세션 버스에 노출하는 표준 인터페이스.
    //! org.mpris.MediaPlayer2.* 버스 이름 중 재생 중인 플레이어를 우선 선택하고, 없으면 기존 선택 또는 첫 번째 플레이어를 사용한다.
    //!
    //! 속성 변경, 탐색, 플레이어 생성/종료 시그널을 구독해 on_change 콜백을 호출하며, 실제 데이터는 poll에서 다시 읽는다.
    //! (MPRIS 스펙상 재생 중 경과 시간 흐름은 시그널로 오지 않는다.)
    //!
    //! 멈춘 플레이어 때문에 전체 덱 세션이 블로킹되는 것을 막기 위해 모든 D-Bus 호출에 CALL_TIMEOUT을 둔다.

    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use toss_proto::{
        Media, ACTION_MEDIA_LOOP, ACTION_MEDIA_NEXT, ACTION_MEDIA_PLAY_PAUSE, ACTION_MEDIA_PREV, ACTION_MEDIA_SEEK, ACTION_MEDIA_SHUFFLE, LOOP_NONE,
        LOOP_PLAYLIST, LOOP_TRACK,
    };
    use zbus::blocking::{connection::Builder, fdo::DBusProxy, Connection, MessageIterator, Proxy};
    use zbus::zvariant::{ObjectPath, OwnedValue};

    use super::{MediaControl, Notify, Now};

    /// D-Bus 호출당 응답 대기 타임아웃
    const CALL_TIMEOUT: Duration = Duration::from_millis(800);
    const PREFIX: &str = "org.mpris.MediaPlayer2.";
    const PATH: &str = "/org/mpris/MediaPlayer2";
    const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

    /// 구독할 D-Bus 시그널 규칙
    const SIGNALS: [&str; 3] = [
        "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/mpris/MediaPlayer2'",
        "type='signal',interface='org.mpris.MediaPlayer2.Player',member='Seeked',path='/org/mpris/MediaPlayer2'",
        "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'",
    ];

    pub struct Mpris {
        conn: Connection,
        /// 현재 추적 중인 플레이어의 D-Bus 이름
        current: Option<String>,
        notify: Arc<Mutex<Option<Notify>>>,
        /// 위치 고정 여부 판별용 상태: (플레이어+곡, 마지막 위치, 해당 위치 최초 관측 시각)
        seen: Option<(String, i64, Instant)>,
    }

    impl Mpris {
        pub fn open() -> zbus::Result<Self> {
            let conn = Builder::session()?.method_timeout(CALL_TIMEOUT).build()?;
            let notify: Arc<Mutex<Option<Notify>>> = Arc::default();
            for rule in SIGNALS {
                let signals = MessageIterator::for_match_rule(rule, &conn, Some(64))?;
                let notify = notify.clone();
                thread::spawn(move || {
                    for _ in signals {
                        if let Some(f) = notify.lock().unwrap().as_ref() {
                            f();
                        }
                    }
                });
            }
            Ok(Self { conn, current: None, notify, seen: None })
        }

        fn player(&self, name: &str) -> zbus::Result<Proxy<'static>> {
            Proxy::new(&self.conn, name.to_string(), PATH, PLAYER)
        }

        /// 활성 플레이어 선택 우선순위: 재생 중 > 직전 선택 > 첫 번째 항목
        fn pick(&mut self) -> Option<String> {
            let names: Vec<String> = DBusProxy::new(&self.conn)
                .ok()
                .and_then(|d| d.list_names().ok())
                .map(|l| l.into_iter().map(|n| n.to_string()).filter(|n| n.starts_with(PREFIX)).collect())
                .unwrap_or_default();
            let playing = names.iter().find(|n| {
                self.player(n).and_then(|p| p.get_property::<String>("PlaybackStatus")).is_ok_and(|s| s == "Playing")
            });
            let chosen = playing
                .or_else(|| self.current.as_ref().filter(|c| names.contains(c)))
                .or_else(|| names.first())
                .cloned();
            self.current = chosen.clone();
            chosen
        }

        /// 재생 중인데 위치 값이 변하지 않는 플레이어(위치 미보고 플레이어) 감지.
        /// 이런 경우 덱이 자체 계산하게 두면 매 폴링마다 위치가 어긋나 덱 화면이 계속 되감기는 현상이 생긴다.
        fn stuck(&mut self, key: String, playing: bool, position_us: i64) -> bool {
            if !playing {
                self.seen = None;
                return false;
            }
            match &self.seen {
                Some((k, pos, since)) if *k == key && *pos == position_us => since.elapsed() >= Duration::from_secs(1),
                _ => {
                    self.seen = Some((key, position_us, Instant::now()));
                    false
                }
            }
        }
    }

    /// 현재 위치에서 offset_us만큼 이동한다.
    /// 상대 Seek는 플레이어마다 곡 범위를 벗어났을 때 동작이 달라서(다음 곡으로 넘어가는 등),
    /// trackid를 알 수 있을 때는 트랙 범위 내로 클램프한 절대 위치를 SetPosition으로 지정한다.
    fn seek(p: &Proxy<'_>, offset_us: i64) -> Result<(), String> {
        let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
        let track = meta.get("mpris:trackid").and_then(|v| {
            ObjectPath::try_from(v.clone()).ok().or_else(|| String::try_from(v.clone()).ok().and_then(|s| ObjectPath::try_from(s).ok()))
        });
        let Some(track) = track else {
            return p.call_method("Seek", &(offset_us,)).map(|_| ()).map_err(|e| e.to_string());
        };
        let length_us = meta
            .get("mpris:length")
            .and_then(|v| i64::try_from(v.clone()).ok().or_else(|| u64::try_from(v.clone()).ok().map(|u| u as i64)))
            .unwrap_or(0);
        let position_us: i64 = p.get_property("Position").map_err(|e| e.to_string())?;
        let mut target = (position_us + offset_us).max(0);
        if length_us > 0 {
            target = target.min((length_us - 500_000).max(0));
        }
        p.call_method("SetPosition", &(track, target)).map(|_| ()).map_err(|e| e.to_string())
    }

    fn text(meta: &HashMap<String, OwnedValue>, key: &str) -> String {
        meta.get(key).and_then(|v| String::try_from(v.clone()).ok()).unwrap_or_default()
    }

    impl MediaControl for Mpris {
        fn poll(&mut self) -> Option<Now> {
            let name = self.pick()?;
            let p = self.player(&name).ok()?;
            let status: String = p.get_property("PlaybackStatus").ok()?;
            let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").unwrap_or_default();
            let artist = meta
                .get("xesam:artist")
                .and_then(|v| <Vec<String>>::try_from(v.clone()).ok())
                .map(|a| a.join(", "))
                .unwrap_or_default();
            let length_us = meta
                .get("mpris:length")
                .and_then(|v| i64::try_from(v.clone()).ok().or_else(|| u64::try_from(v.clone()).ok().map(|u| u as i64)))
                .unwrap_or(0);
            let position_us: i64 = p.get_property("Position").unwrap_or(0);
            let title = text(&meta, "xesam:title");
            let rate: f64 = p.get_property("Rate").ok().filter(|r: &f64| *r > 0.0).unwrap_or(1.0);
            let stuck = self.stuck(format!("{name}\n{title}"), status == "Playing", position_us);
            Some(Now {
                media: Media {
                    player: name.trim_start_matches(PREFIX).to_string(),
                    status: status.to_lowercase(),
                    title,
                    artist,
                    album: text(&meta, "xesam:album"),
                    length_ms: (length_us.max(0) / 1000) as u64,
                    position_ms: (position_us.max(0) / 1000) as u64,
                    rate: if stuck { 0.0 } else { rate },
                    can_next: p.get_property("CanGoNext").unwrap_or(false),
                    can_prev: p.get_property("CanGoPrevious").unwrap_or(false),
                    can_seek: p.get_property("CanSeek").unwrap_or(false),
                    shuffle: p.get_property::<bool>("Shuffle").ok(),
                    loop_mode: p.get_property::<String>("LoopStatus").ok().map(|s| {
                        match s.as_str() {
                            "Track" => LOOP_TRACK,
                            "Playlist" => LOOP_PLAYLIST,
                            _ => LOOP_NONE,
                        }
                        .to_string()
                    }),
                    art: String::new(),
                },
                art_url: text(&meta, "mpris:artUrl"),
            })
        }

        fn action(&mut self, name: &str, args: &serde_json::Value) -> Result<(), String> {
            let player = self.current.clone().or_else(|| self.pick()).ok_or("활성 플레이어가 없습니다")?;
            let p = self.player(&player).map_err(|e| e.to_string())?;
            let call = |method: &str| p.call_method(method, &()).map(|_| ()).map_err(|e| e.to_string());
            match name {
                ACTION_MEDIA_PLAY_PAUSE => call("PlayPause"),
                ACTION_MEDIA_NEXT => call("Next"),
                ACTION_MEDIA_PREV => call("Previous"),
                ACTION_MEDIA_SEEK => {
                    let offset_ms = args["offset_ms"].as_i64().ok_or("offset_ms 인자가 없습니다")?.clamp(-3_600_000, 3_600_000);
                    seek(&p, offset_ms * 1000)
                }
                ACTION_MEDIA_SHUFFLE => {
                    let on = args["on"].as_bool().ok_or("on 인자가 없습니다")?;
                    p.set_property("Shuffle", on).map_err(|e| e.to_string())
                }
                ACTION_MEDIA_LOOP => {
                    let mode = match args["mode"].as_str() {
                        Some(LOOP_NONE) => "None",
                        Some(LOOP_PLAYLIST) => "Playlist",
                        Some(LOOP_TRACK) => "Track",
                        _ => return Err("mode는 none, playlist, track 중 하나여야 합니다".to_string()),
                    };
                    p.set_property("LoopStatus", mode).map_err(|e| e.to_string())
                }
                other => Err(format!("알 수 없는 액션: {other}")),
            }
        }

        fn on_change(&mut self, notify: Option<Notify>) {
            *self.notify.lock().unwrap() = notify;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(position_ms: u64) -> Media {
        Media { status: "playing".into(), title: "곡".into(), length_ms: 200_000, position_ms, ..Media::default() }
    }

    #[test]
    fn playback_flowing_on_is_not_an_event() {
        let sent = playing(10_000);
        assert!(!changed(Some(&sent), Duration::from_secs(30), Some(&playing(40_000))));
        assert!(!changed(Some(&sent), Duration::from_secs(30), Some(&playing(41_000))));
        assert!(!changed(None, Duration::ZERO, None));
    }

    #[test]
    fn seeking_pausing_and_track_changes_are_events() {
        let sent = playing(10_000);
        let after = Duration::from_secs(30);
        assert!(changed(Some(&sent), after, Some(&playing(90_000))));
        assert!(changed(Some(&sent), after, Some(&playing(0))));
        assert!(changed(Some(&sent), after, Some(&Media { status: "paused".into(), ..playing(40_000) })));
        assert!(changed(Some(&sent), after, Some(&Media { title: "다른 곡".into(), ..playing(40_000) })));
        assert!(changed(Some(&sent), after, Some(&Media { art: "a1".into(), ..playing(40_000) })));
        assert!(changed(Some(&sent), after, Some(&Media { rate: 1.5, ..playing(40_000) })));
        assert!(changed(Some(&sent), after, Some(&Media { shuffle: Some(true), ..playing(40_000) })));
        assert!(changed(Some(&sent), after, Some(&Media { loop_mode: Some("track".into()), ..playing(40_000) })));
        assert!(changed(None, after, Some(&sent)));
        assert!(changed(Some(&sent), after, None));
    }

    #[test]
    fn a_paused_player_only_reports_when_moved() {
        let sent = Media { status: "paused".into(), ..playing(10_000) };
        let later = Duration::from_secs(600);
        assert!(!changed(Some(&sent), later, Some(&sent)));
        assert!(changed(Some(&sent), later, Some(&Media { position_ms: 50_000, ..sent.clone() })));
    }

    #[test]
    fn a_player_that_never_moves_its_position_does_not_flap() {
        let sent = Media { rate: 0.0, ..playing(0) };
        assert!(!changed(Some(&sent), Duration::from_secs(120), Some(&sent)));
    }
}
