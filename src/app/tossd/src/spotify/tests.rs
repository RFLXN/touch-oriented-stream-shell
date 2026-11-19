use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;

fn track() -> Value {
    json!({
        "is_playing": true,
        "progress_ms": 61000,
        "shuffle_state": true,
        "repeat_state": "context",
        "actions": { "disallows": { "seeking": true } },
        "device": { "name": "DESKTOP" },
        "item": {
            "type": "track",
            "uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
            "name": "Song",
            "duration_ms": 200000,
            "artists": [{ "name": "A" }, { "name": "B" }],
            "album": { "name": "Album", "images": [
                { "url": "https://i/640", "width": 640 }, { "url": "https://i/300", "width": 300 }, { "url": "https://i/64", "width": 64 }
            ] }
        }
    })
}

#[test]
fn the_player_answer_becomes_media() {
    let (m, art) = media_from(&track()).unwrap();
    assert_eq!((m.title.as_str(), m.artist.as_str(), m.album.as_str(), m.status.as_str()), ("Song", "A, B", "Album", "playing"));
    assert_eq!((m.length_ms, m.position_ms, m.can_seek, m.can_next), (200000, 61000, false, true));
    assert_eq!((m.shuffle, m.loop_mode.as_deref()), (Some(true), Some(LOOP_PLAYLIST)));
    assert_eq!(art, "https://i/300");

    let episode = json!({ "is_playing": false, "progress_ms": 5, "repeat_state": "off",
        "item": { "type": "episode", "name": "Ep", "duration_ms": 10, "show": { "name": "Show", "images": [{ "url": "https://s/64", "width": 64 }] } } });
    let (m, art) = media_from(&episode).unwrap();
    assert_eq!((m.title.as_str(), m.artist.as_str(), m.album.as_str(), m.status.as_str(), m.shuffle, m.loop_mode.as_deref()), ("Ep", "Show", "", "paused", None, Some(LOOP_NONE)));
    assert_eq!(art, "https://s/64");
    assert!(media_from(&json!({ "is_playing": false, "item": null })).is_none());
}

#[test]
fn actions_become_api_requests() {
    let (m, _) = media_from(&track()).unwrap();
    let paused = Media { status: "paused".into(), ..m.clone() };
    assert_eq!(request_for(ACTION_MEDIA_PLAY_PAUSE, &Value::Null, Some((&m, 0))).unwrap(), ("PUT", "/me/player/pause", vec![]));
    assert_eq!(request_for(ACTION_MEDIA_PLAY_PAUSE, &Value::Null, Some((&paused, 0))).unwrap(), ("PUT", "/me/player/play", vec![]));
    assert_eq!(request_for(ACTION_MEDIA_PLAY_PAUSE, &Value::Null, None).unwrap().1, "/me/player/play");
    assert_eq!(request_for(ACTION_MEDIA_PREV, &Value::Null, None).unwrap(), ("POST", "/me/player/previous", vec![]));
    assert_eq!(request_for(ACTION_MEDIA_SEEK, &json!({ "offset_ms": -5000 }), Some((&m, 3000))).unwrap().2, vec![("position_ms", "0".to_string())]);
    assert_eq!(request_for(ACTION_MEDIA_SEEK, &json!({ "offset_ms": 5000 }), Some((&m, 198000))).unwrap().2, vec![("position_ms", "199999".to_string())]);
    assert!(request_for(ACTION_MEDIA_SEEK, &json!({ "offset_ms": 5000 }), None).is_err());
    assert_eq!(request_for(ACTION_MEDIA_SHUFFLE, &json!({ "on": false }), None).unwrap().2, vec![("state", "false".to_string())]);
    assert_eq!(request_for(ACTION_MEDIA_LOOP, &json!({ "mode": "track" }), None).unwrap().2, vec![("state", "track".to_string())]);
    assert!(request_for(ACTION_MEDIA_LOOP, &json!({ "mode": "x; rm" }), None).is_err());
    assert!(request_for("command.run", &Value::Null, None).is_err());
}

#[test]
fn only_real_changes_are_sent_again() {
    let (m, _) = media_from(&track()).unwrap();
    let state = |media: Media| SpotifyState { linked: true, live: true, media: Some(media), ..SpotifyState::default() };
    let a = state(m.clone());
    assert!(same_but_time(&a, &state(Media { position_ms: m.position_ms + 3000, ..m.clone() }), 3000));
    assert!(!same_but_time(&a, &state(Media { position_ms: m.position_ms + 9000, ..m.clone() }), 3000), "탐색한 위치를 보내지 않음");
    assert!(!same_but_time(&a, &state(Media { status: "paused".into(), ..m.clone() }), 0));
    assert!(!same_but_time(&a, &SpotifyState { linked: true, live: true, ..SpotifyState::default() }, 0));
}

/// 요청마다 (메서드, 경로, Authorization 헤더)를 기록하고 `answer`가 정한 응답을 돌려주는 가짜 API 서버
type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

fn fake_api(answer: impl Fn(&str, &str) -> (u16, &'static str, Vec<u8>) + Send + 'static) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 2048];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let Ok(n) = s.read(&mut chunk) else { break };
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf).to_string();
            let mut words = head.split_whitespace();
            let (method, path) = (words.next().unwrap_or_default().to_string(), words.next().unwrap_or_default().to_string());
            let auth = head.lines().find_map(|l| l.strip_prefix("Authorization: ")).unwrap_or_default().to_string();
            log.lock().unwrap().push((method.clone(), path.clone(), auth));
            let (status, kind, body) = answer(&method, &path);
            let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(&body);
        }
    });
    (base, seen)
}

/// 토큰이 거부되면 갱신해서 다시 묻고, 앨범 아트를 받아 붙이고, 제어 결과(Premium 필요 포함)를 위젯 앞으로 남긴다
#[test]
fn the_worker_polls_refreshes_fetches_art_and_reports_actions() {
    let base = Arc::new(Mutex::new(String::new()));
    let first = Arc::new(Mutex::new(true));
    let (api, seen) = fake_api({
        let base = base.clone();
        move |method, path| {
            if std::mem::replace(&mut *first.lock().unwrap(), false) {
                return (401, "application/json", json!({ "error": { "status": 401, "message": "The access token expired" } }).to_string().into_bytes());
            }
            match (method, path) {
                ("GET", p) if p.starts_with("/me/player?") => {
                    let mut player = track();
                    player["item"]["album"]["images"] = json!([{ "url": format!("{}/art.jpg", base.lock().unwrap()), "width": 300 }]);
                    (200, "application/json", player.to_string().into_bytes())
                }
                ("PUT", "/me/player/pause") => (403, "application/json", json!({ "error": { "status": 403, "message": "Player command failed: Premium required", "reason": "PREMIUM_REQUIRED" } }).to_string().into_bytes()),
                ("POST", "/me/player/next") => (204, "text/plain", Vec::new()),
                ("GET", "/me/library/contains?uris=spotify%3Atrack%3A4uLU6hMCjMI75M1A2tKUQC") => (200, "application/json", b"[true]".to_vec()),
                ("DELETE", "/me/library?uris=spotify%3Atrack%3A4uLU6hMCjMI75M1A2tKUQC") => (200, "application/json", Vec::new()),
                ("GET", "/me/player/queue") => (200, "application/json", json!({ "queue": [{ "type": "track", "name": "Next", "artists": [{ "name": "C" }] }] }).to_string().into_bytes()),
                ("GET", "/me/player/devices") => (200, "application/json", json!({ "devices": [
                    { "id": "abc123", "name": "DESKTOP", "type": "Computer", "is_active": true },
                    { "id": "bad/id", "name": "X", "type": "Speaker" },
                    { "id": "def456", "name": "Locked", "type": "TV", "is_restricted": true }
                ] }).to_string().into_bytes()),
                ("PUT", "/me/player") => (204, "text/plain", Vec::new()),
                ("GET", "/art.jpg") => (200, "image/jpeg", vec![0xff, 0xd8, 0xff, 0xe0]),
                _ => (404, "text/plain", Vec::new()),
            }
        }
    });
    *base.lock().unwrap() = api.clone();
    let refreshed = Arc::new(Mutex::new(0));
    let fresh = refreshed.clone();
    let link = Link {
        token: Box::new(move |stale| {
            if stale {
                *fresh.lock().unwrap() += 1;
            }
            Ok(if *fresh.lock().unwrap() > 0 { "T2" } else { "T1" }.into())
        }),
        linked: Box::new(|| true),
    };
    let states = Arc::new(Mutex::new(Vec::<SpotifyState>::new()));
    let results = Arc::new(Mutex::new(Vec::new()));
    let (s, r) = (states.clone(), results.clone());
    let tx = spawn_with(api, link, move |st| s.lock().unwrap().push(st), move |w, ok, e| r.lock().unwrap().push((w, ok, e)));
    tx.send(Cmd::Want(true)).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let got = loop {
        if let Some(st) = states.lock().unwrap().iter().find(|s| s.media.is_some()).cloned() {
            break st;
        }
        assert!(Instant::now() < until, "재생 정보가 오지 않음: {:?}", states.lock().unwrap());
        thread::sleep(Duration::from_millis(20));
    };
    assert!(got.live && got.linked && got.error.is_empty());
    assert_eq!(got.media.as_ref().unwrap().title, "Song");
    let art = got.art.expect("앨범 아트가 없음");
    assert_eq!((art.mime.as_str(), art.data.as_str(), got.media.unwrap().art), ("image/jpeg", "/9j/4A==", art.id.clone()));
    assert_eq!(*refreshed.lock().unwrap(), 1, "거부된 토큰을 갱신하지 않음");
    let until = Instant::now() + Duration::from_secs(5);
    let got = loop {
        if let Some(st) = states.lock().unwrap().iter().rev().find(|s| s.liked.is_some() && !s.queue.is_empty()).cloned() {
            break st;
        }
        assert!(Instant::now() < until, "좋아요·대기열이 오지 않음: {:?}", states.lock().unwrap().last());
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!((got.device.as_str(), got.liked, got.queue.clone()), ("DESKTOP", Some(true), vec![Track { title: "Next".into(), artist: "C".into() }]));

    tx.send(Cmd::Act { name: ACTION_MEDIA_PLAY_PAUSE.into(), args: Value::Null, widget: Some(7) }).unwrap();
    tx.send(Cmd::Act { name: ACTION_MEDIA_NEXT.into(), args: Value::Null, widget: Some(8) }).unwrap();
    tx.send(Cmd::Act { name: ACTION_DEVICES.into(), args: Value::Null, widget: Some(9) }).unwrap();
    tx.send(Cmd::Act { name: ACTION_TRANSFER.into(), args: json!({ "id": "abc123" }), widget: Some(10) }).unwrap();
    tx.send(Cmd::Act { name: ACTION_TRANSFER.into(), args: json!({ "id": "../me" }), widget: Some(11) }).unwrap();
    tx.send(Cmd::Act { name: ACTION_LIKE.into(), args: json!({ "on": false }), widget: Some(12) }).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while results.lock().unwrap().len() < 6 && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        *results.lock().unwrap(),
        [
            (7, false, "Spotify Premium required".to_string()),
            (8, true, String::new()),
            (9, true, String::new()),
            (10, true, String::new()),
            (11, false, "Bad device".to_string()),
            (12, true, String::new())
        ]
    );
    let last = states.lock().unwrap().iter().rev().find(|s| !s.devices.is_empty()).cloned().expect("기기 목록이 오지 않음");
    assert_eq!(last.devices, vec![Device { id: "abc123".into(), name: "DESKTOP".into(), kind: "Computer".into(), active: true }], "형식이 틀리거나 제어할 수 없는 기기를 넣음");
    assert_eq!(states.lock().unwrap().last().unwrap().liked, Some(false));
    let log = seen.lock().unwrap().clone();
    assert!(log.iter().any(|(m, p, a)| m == "GET" && p.starts_with("/me/player?additional_types=") && a == "Bearer T2"), "{log:?}");
    assert!(log.iter().any(|(m, p, _)| m == "PUT" && p == "/me/player/pause"), "{log:?}");
    assert_eq!(log.iter().filter(|(m, p, _)| m == "PUT" && p == "/me/player").count(), 1, "형식이 틀린 기기로 옮김: {log:?}");

    tx.send(Cmd::Want(false)).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while states.lock().unwrap().last().is_some_and(|s| s.media.is_some()) && Instant::now() < until {
        thread::sleep(Duration::from_millis(20));
    }
    let idle = states.lock().unwrap().last().unwrap().clone();
    assert!(idle.linked && idle.media.is_none() && idle.queue.is_empty() && idle.liked.is_none(), "{idle:?}");
}

#[test]
fn uris_and_device_ids_are_checked_before_use() {
    assert!(library_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC") && library_uri("spotify:episode:4uLU6hMCjMI75M1A2tKUQC"));
    assert!(!library_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQ&") && !library_uri("spotify:album:4uLU6hMCjMI75M1A2tKUQC") && !library_uri(""));
    assert!(device_id("0d1841b0976bae2a3a310dd74c0f3df354899bc8") && !device_id("a b") && !device_id(""));
    let q = queue_from(&json!({ "queue": [{ "type": "episode", "name": "Ep", "show": { "name": "Show" } }, { "name": null }] }));
    assert_eq!(q, vec![Track { title: "Ep".into(), artist: "Show".into() }]);
}

#[test]
fn the_main_color_leans_to_the_vivid_part_of_the_cover() {
    let mut img = image::RgbImage::from_pixel(8, 8, image::Rgb([128, 128, 128]));
    for x in 0..4 {
        for y in 0..8 {
            img.put_pixel(x, y, image::Rgb([30, 215, 96]));
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let c = main_color(png.get_ref()).unwrap();
    let g = u8::from_str_radix(&c[3..5], 16).unwrap();
    let r = u8::from_str_radix(&c[1..3], 16).unwrap();
    assert!(c.len() == 7 && g > 160 && r < 90, "{c}");
    assert!(main_color(b"not an image").is_none());
}
