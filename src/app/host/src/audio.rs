//! 음량 믹서: PulseAudio 서버(PipeWire의 pipewire-pulse 포함)에서 출력 장치(sink)와 앱별 재생 스트림(sink input)을 읽어
//! 덱에 보내고, 덱의 요청대로 음량·음소거·출력 장치·기본 출력 장치를 바꾼다.
//!
//! audio.info 권한이 허용된 동안 전용 스레드(Mixer)가 서버에 연결해 동작한다:
//!   - 상태: 서버가 알리는 변경(subscribe)마다 출력 장치·스트림·기본 장치를 다시 읽고, 덱에 보낸 것과 다를 때만 전체를 보낸다.
//!   - 레벨: 덱이 구독(watch의 audio) 중일 때만 스트림과 출력 장치마다 피크 측정 스트림(PEAK_DETECT, 초당 25개)을 열고,
//!     AUDIO_LEVEL_EVERY마다 그 구간의 최댓값 중 바뀐 것만 보낸다. 구독이 끝나면 측정 스트림을 모두 닫는다.
//!   - 서버에 연결하지 못하거나 연결이 끊기면 상태를 null로 알리고 RETRY_EVERY마다 다시 연결한다.
//!
//! 같은 앱의 스트림(브라우저 탭, Discord의 여러 음성 스트림 등)은 하나의 앱으로 묶는다(`app_of`). 묶는 키는 실행 파일 이름이며,
//! NixOS 래퍼 이름(`.Discord-wrapped`)은 원래 이름으로 되돌린다. 앱의 음량을 바꾸면 묶인 스트림마다 채널 사이의 균형을 유지한 채
//! 가장 큰 채널이 그 값이 되도록 맞춘다.
//!
//! PulseAudio 메인 루프와 모든 콜백은 이 스레드 하나에서만 돈다(`pulse.rs`). 세션 스레드의 요청은 채널로 받아 메인 루프 사이사이에 처리한다.
//! 리눅스 외의 OS에서는 서버에 연결하지 않으므로 상태가 항상 null이다. 핸들을 드롭하면 스레드가 종료된다.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use toss_proto::Audio;

#[cfg(target_os = "linux")]
mod pulse;

/// 서버 재연결 간격, 요청 응답 대기 한도
const RETRY_EVERY: Duration = Duration::from_secs(3);
const REPLY_WAIT: Duration = Duration::from_secs(2);

pub enum Event {
    /// 믹서 상태(초기 및 변경 시). 서버에 연결되어 있지 않으면 None
    State(Option<Audio>),
    /// 바뀐 레벨 값(0~1)
    Levels { apps: BTreeMap<String, f64>, outputs: BTreeMap<String, f64> },
}

enum Cmd {
    Watch(bool),
    Act(String, Value, Sender<Result<(), String>>),
}

pub struct Mixer {
    tx: Sender<Cmd>,
}

impl Mixer {
    pub fn start(watch: bool, on_event: impl Fn(Event) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || run(rx, watch, on_event));
        Self { tx }
    }

    /// 덱의 레벨 구독 여부 반영
    pub fn watch(&self, on: bool) {
        let _ = self.tx.send(Cmd::Watch(on));
    }

    /// 덱의 음량 액션 실행. 대상이 있는지까지 확인하고 돌아오며, 실제 반영은 뒤따르는 상태로 확인한다
    pub fn act(&self, name: &str, args: &Value) -> Result<(), String> {
        let (reply, got) = mpsc::channel();
        self.tx.send(Cmd::Act(name.to_string(), args.clone(), reply)).map_err(|_| "음량 믹서가 멈췄습니다".to_string())?;
        got.recv_timeout(REPLY_WAIT).unwrap_or_else(|_| Err("오디오 서버가 응답하지 않습니다".to_string()))
    }
}

fn run(rx: Receiver<Cmd>, mut watch: bool, on_event: impl Fn(Event)) {
    let mut said_none = false;
    loop {
        #[cfg(target_os = "linux")]
        if let Some(mut server) = pulse::Server::connect() {
            eprintln!("음량 믹서: 오디오 서버에 연결됨");
            said_none = false;
            match server.serve(&rx, &mut watch, &on_event) {
                Ended::Stopped => return,
                Ended::Lost => eprintln!("음량 믹서: 오디오 서버 연결이 끊어졌습니다"),
            }
        }
        if !said_none {
            said_none = true;
            on_event(Event::State(None));
        }
        let until = Instant::now() + RETRY_EVERY;
        loop {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(Cmd::Watch(on)) => watch = on,
                Ok(Cmd::Act(_, _, reply)) => {
                    let _ = reply.send(Err("오디오 서버에 연결되어 있지 않습니다".to_string()));
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

enum Ended {
    Stopped,
    Lost,
}

/// 직전에 보낸 값과 다른 항목만 골라 내고, 보낸 값을 갱신한다
fn changed(now: &BTreeMap<String, f64>, sent: &mut BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let out: BTreeMap<String, f64> = now.iter().filter(|(k, v)| sent.get(*k) != Some(v)).map(|(k, v)| (k.clone(), *v)).collect();
    sent.retain(|k, _| now.contains_key(k));
    sent.extend(out.clone());
    out
}

fn round(v: f64, scale: f64) -> f64 {
    (v * scale).round() / scale
}

/// NixOS 래퍼 이름(`.Discord-wrapped`)을 원래 실행 파일 이름으로 되돌린다
fn unwrap_name(name: &str) -> &str {
    let name = name.trim();
    let name = name.strip_prefix('.').unwrap_or(name);
    name.strip_suffix("-wrapped").unwrap_or(name)
}

/// 스트림이 속한 앱의 묶음 키와 표시 이름.
/// 키는 실행 파일 이름(소문자)이다. 표시 이름은 앱이 밝힌 이름이 실행 파일 이름과 겹칠 때만 쓰고(`Firefox`, `Claude`),
/// 겹치지 않으면(`WEBRTC VoiceEngine`처럼 엔진 이름을 밝히는 경우) 실행 파일 이름을 쓴다.
fn app_of(app_name: &str, binary: &str, stream: &str, index: u32) -> (String, String) {
    let (n, b) = (unwrap_name(app_name), unwrap_name(binary));
    let (nl, bl) = (n.to_lowercase(), b.to_lowercase());
    if b.is_empty() {
        let shown = if n.is_empty() { stream.trim() } else { n };
        let key = if shown.is_empty() { format!("stream-{index}") } else { shown.to_lowercase() };
        return (key, if shown.is_empty() { key_name(index) } else { shown.to_string() });
    }
    let shown = if !n.is_empty() && (bl.contains(&nl) || nl.contains(&bl)) { n } else { b };
    (bl, shown.to_string())
}

fn key_name(index: u32) -> String {
    format!("Stream {index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_are_named_after_what_they_say_unless_it_is_an_engine_name() {
        assert_eq!(app_of("Firefox", "firefox", "AudioStream", 1), ("firefox".into(), "Firefox".into()));
        assert_eq!(app_of("WEBRTC VoiceEngine", ".Discord-wrapped", "playback", 2), ("discord".into(), "Discord".into()));
        assert_eq!(app_of(".AyuGram-wrapped", ".AyuGram-wrapped", "Playback Stream", 3), ("ayugram".into(), "AyuGram".into()));
        assert_eq!(app_of("Claude", "claude-desktop", "", 4), ("claude-desktop".into(), "Claude".into()));
        assert_eq!(app_of("mpv", "", "", 5), ("mpv".into(), "mpv".into()));
        assert_eq!(app_of("", "", "", 6), ("stream-6".into(), "Stream 6".into()));
    }

    #[test]
    fn only_changed_levels_go_out() {
        let mut sent = BTreeMap::new();
        let now = BTreeMap::from([("a".to_string(), 0.5), ("b".to_string(), 0.0)]);
        assert_eq!(changed(&now, &mut sent), now);
        let later = BTreeMap::from([("a".to_string(), 0.5), ("b".to_string(), 0.25)]);
        assert_eq!(changed(&later, &mut sent), BTreeMap::from([("b".to_string(), 0.25)]));
        assert_eq!(changed(&BTreeMap::new(), &mut sent), BTreeMap::new());
        assert!(sent.is_empty());
    }
}
