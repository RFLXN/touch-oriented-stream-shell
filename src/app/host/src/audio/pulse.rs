//! PulseAudio 연결: 서버 상태 읽기, 피크 측정 스트림, 음량 액션 실행(`super`의 머리 주석 참고). 리눅스에서만 빌드한다.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use libpulse_binding as pa;
use pa::callbacks::ListResult;
use pa::context::subscribe::InterestMaskSet;
use pa::context::{Context, FlagSet as ContextFlags, State as ContextState};
use pa::def::BufferAttr;
use pa::mainloop::standard::Mainloop;
use pa::proplist::{properties, Proplist};
use pa::sample::{Format, Spec};
use pa::stream::{FlagSet as StreamFlags, PeekResult, Stream};
use pa::time::MicroSeconds;
use pa::volume::{ChannelVolumes, Volume};
use serde_json::Value;
use toss_proto::{Audio, AudioApp, AudioOutput, ACTION_AUDIO_DEFAULT, ACTION_AUDIO_MUTE, ACTION_AUDIO_ROUTE, ACTION_AUDIO_VOLUME, AUDIO_LEVEL_EVERY};

use super::{app_of, changed, round, Cmd, Ended, Event};

/// 연결 대기 한도, 메인 루프 한 바퀴의 최대 대기 시간
const CONNECT_WAIT: Duration = Duration::from_secs(3);
const TURN: MicroSeconds = MicroSeconds(20_000);
/// 피크 측정 스트림의 샘플 수(초당). 서버가 이 간격마다 구간의 최댓값 하나를 보낸다
const PEAK_RATE: u32 = 25;
/// 덱에 보내는 앱 수 상한
const APPS_MAX: usize = 64;

#[derive(Clone)]
struct Sink {
    index: u32,
    name: String,
    title: String,
    volume: ChannelVolumes,
    mute: bool,
    monitor: u32,
}

#[derive(Clone)]
struct Input {
    index: u32,
    /// 묶음 키와 표시 이름(`name_inputs`가 채운다)
    app: String,
    name: String,
    /// 스트림이 밝힌 앱 이름, 실행 파일 이름, 스트림 이름
    said: (String, String, String),
    client: Option<u32>,
    sink: u32,
    volume: ChannelVolumes,
    mute: bool,
}

#[derive(Clone, Default)]
struct Snapshot {
    default_sink: String,
    sinks: Vec<Sink>,
    inputs: Vec<Input>,
    /// 클라이언트가 밝힌 앱 이름과 실행 파일 이름. 스트림에 이름이 없을 때 쓴다(PipeWire의 일부 앱)
    clients: HashMap<u32, (String, String)>,
}

impl Snapshot {
    fn name_inputs(&mut self) {
        for i in &mut self.inputs {
            let (said_name, said_binary, stream) = &i.said;
            let client = i.client.and_then(|c| self.clients.get(&c));
            let pick = |own: &String, theirs: Option<&String>| if own.is_empty() { theirs.cloned().unwrap_or_default() } else { own.clone() };
            let name = pick(said_name, client.map(|c| &c.0));
            let binary = pick(said_binary, client.map(|c| &c.1));
            (i.app, i.name) = app_of(&name, &binary, stream, i.index);
        }
    }
}

/// 콜백이 채우는 서버 상태. `pending`이 0이 되면 `next`가 완성되어 `now`로 옮겨진다
#[derive(Default)]
struct Model {
    /// 서버가 변경을 알렸는지(다시 읽어야 함)
    dirty: bool,
    pending: u8,
    next: Snapshot,
    now: Snapshot,
    /// `now`가 새로 완성됨(아직 덱에 반영하지 않음)
    fresh: bool,
}

impl Model {
    fn done(&mut self) {
        self.pending = self.pending.saturating_sub(1);
        if self.pending == 0 {
            self.next.name_inputs();
            self.now = std::mem::take(&mut self.next);
            self.fresh = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Input(u32),
    Sink(u32),
}

struct Monitor {
    stream: Rc<RefCell<Stream>>,
    source: u32,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        if let Ok(mut s) = self.stream.try_borrow_mut() {
            s.set_read_callback(None);
            let _ = s.disconnect();
        }
    }
}

/// 연결된 서버. 필드는 선언 순서대로 해제되므로 측정 스트림, 컨텍스트, 메인 루프 순으로 둔다
pub(super) struct Server {
    monitors: HashMap<Key, Monitor>,
    context: Context,
    mainloop: Mainloop,
    model: Rc<RefCell<Model>>,
    peaks: Rc<RefCell<HashMap<Key, f32>>>,
}

impl Server {
    pub(super) fn connect() -> Option<Self> {
        let mut props = Proplist::new()?;
        props.set_str(properties::APPLICATION_NAME, "toss-connector").ok()?;
        props.set_str(properties::APPLICATION_ID, "toss-connector").ok()?;
        let mut mainloop = Mainloop::new()?;
        let mut context = Context::new_with_proplist(&mainloop, "toss-connector", &props)?;
        context.connect(None, ContextFlags::NOAUTOSPAWN, None).ok()?;
        let until = Instant::now() + CONNECT_WAIT;
        loop {
            match context.get_state() {
                ContextState::Ready => break,
                ContextState::Failed | ContextState::Terminated => return None,
                _ if Instant::now() >= until => return None,
                _ => {}
            }
            if mainloop.prepare(Some(TURN)).is_err() || mainloop.poll().is_err() || mainloop.dispatch().is_err() {
                return None;
            }
        }
        let model = Rc::new(RefCell::new(Model { dirty: true, ..Model::default() }));
        let m = model.clone();
        context.set_subscribe_callback(Some(Box::new(move |_, _, _| m.borrow_mut().dirty = true)));
        context.subscribe(InterestMaskSet::SINK | InterestMaskSet::SINK_INPUT | InterestMaskSet::CLIENT | InterestMaskSet::SERVER, |_| {});
        Some(Self { monitors: HashMap::new(), context, mainloop, model, peaks: Rc::new(RefCell::new(HashMap::new())) })
    }

    pub(super) fn serve(&mut self, rx: &Receiver<Cmd>, watch: &mut bool, on_event: &impl Fn(Event)) -> Ended {
        let mut sent: Option<Audio> = None;
        let mut sent_levels: (BTreeMap<String, f64>, BTreeMap<String, f64>) = Default::default();
        let mut next_levels = Instant::now();
        loop {
            loop {
                match rx.try_recv() {
                    Ok(Cmd::Watch(on)) => {
                        *watch = on;
                        sent_levels = Default::default();
                        self.reconcile(*watch);
                    }
                    Ok(Cmd::Act(name, args, reply)) => {
                        let _ = reply.send(self.act(&name, &args));
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ended::Stopped,
                }
            }
            if self.mainloop.prepare(Some(TURN)).is_err() || self.mainloop.poll().is_err() || self.mainloop.dispatch().is_err() {
                return Ended::Lost;
            }
            if matches!(self.context.get_state(), ContextState::Failed | ContextState::Terminated) {
                return Ended::Lost;
            }
            let (reread, fresh) = {
                let mut m = self.model.borrow_mut();
                let reread = m.dirty && m.pending == 0;
                if reread {
                    m.dirty = false;
                }
                (reread, std::mem::take(&mut m.fresh))
            };
            if reread {
                self.read();
            }
            if fresh {
                let audio = audio_of(&self.model.borrow().now);
                if sent.as_ref() != Some(&audio) {
                    on_event(Event::State(Some(audio.clone())));
                    sent = Some(audio);
                }
                self.reconcile(*watch);
            }
            if *watch && Instant::now() >= next_levels {
                next_levels = Instant::now() + AUDIO_LEVEL_EVERY;
                let (apps, outputs) = self.levels();
                let apps = changed(&apps, &mut sent_levels.0);
                let outputs = changed(&outputs, &mut sent_levels.1);
                if !apps.is_empty() || !outputs.is_empty() {
                    on_event(Event::Levels { apps, outputs });
                }
            }
        }
    }

    /// 기본 장치, 출력 장치, 스트림 목록을 다시 읽는다(결과는 콜백이 `Model`에 채운다)
    fn read(&mut self) {
        {
            let mut m = self.model.borrow_mut();
            m.pending = 4;
            m.next = Snapshot::default();
        }
        let intro = self.context.introspect();
        let m = self.model.clone();
        intro.get_server_info(move |info| {
            let mut m = m.borrow_mut();
            m.next.default_sink = info.default_sink_name.as_deref().unwrap_or_default().to_string();
            m.done();
        });
        let m = self.model.clone();
        intro.get_sink_info_list(move |got| match got {
            ListResult::Item(s) => {
                let name = s.name.as_deref().unwrap_or_default().to_string();
                let title = s.description.as_deref().filter(|d| !d.is_empty()).unwrap_or(&name).to_string();
                m.borrow_mut().next.sinks.push(Sink { index: s.index, name, title, volume: s.volume, mute: s.mute, monitor: s.monitor_source });
            }
            ListResult::End | ListResult::Error => m.borrow_mut().done(),
        });
        let m = self.model.clone();
        intro.get_sink_input_info_list(move |got| match got {
            ListResult::Item(i) => {
                let said = (
                    i.proplist.get_str(properties::APPLICATION_NAME).unwrap_or_default(),
                    i.proplist.get_str(properties::APPLICATION_PROCESS_BINARY).unwrap_or_default(),
                    i.name.as_deref().unwrap_or_default().to_string(),
                );
                let input = Input { index: i.index, app: String::new(), name: String::new(), said, client: i.client, sink: i.sink, volume: i.volume, mute: i.mute };
                m.borrow_mut().next.inputs.push(input);
            }
            ListResult::End | ListResult::Error => m.borrow_mut().done(),
        });
        let m = self.model.clone();
        intro.get_client_info_list(move |got| match got {
            ListResult::Item(c) => {
                let said = (c.proplist.get_str(properties::APPLICATION_NAME).unwrap_or_default(), c.proplist.get_str(properties::APPLICATION_PROCESS_BINARY).unwrap_or_default());
                m.borrow_mut().next.clients.insert(c.index, said);
            }
            ListResult::End | ListResult::Error => m.borrow_mut().done(),
        });
    }

    /// 구독 여부와 현재 스트림·장치에 맞춰 피크 측정 스트림을 열고 닫는다
    fn reconcile(&mut self, watch: bool) {
        let want: HashMap<Key, u32> = if watch {
            let now = &self.model.borrow().now;
            let monitor_of = |sink: u32| now.sinks.iter().find(|s| s.index == sink).map(|s| s.monitor);
            now.inputs
                .iter()
                .filter_map(|i| Some((Key::Input(i.index), monitor_of(i.sink)?)))
                .chain(now.sinks.iter().map(|s| (Key::Sink(s.index), s.monitor)))
                .collect()
        } else {
            HashMap::new()
        };
        self.monitors.retain(|key, m| want.get(key) == Some(&m.source));
        self.peaks.borrow_mut().retain(|key, _| want.contains_key(key));
        for (key, source) in want {
            if self.monitors.contains_key(&key) {
                continue;
            }
            if let Some(m) = self.open_monitor(key, source) {
                self.monitors.insert(key, m);
            }
        }
    }

    fn open_monitor(&mut self, key: Key, source: u32) -> Option<Monitor> {
        let spec = Spec { format: Format::FLOAT32NE, channels: 1, rate: PEAK_RATE };
        let mut stream = Stream::new(&mut self.context, "toss-connector peak", &spec, None)?;
        if let Key::Input(index) = key {
            stream.set_monitor_stream(index).ok()?;
        }
        let stream = Rc::new(RefCell::new(stream));
        let (weak, peaks) = (Rc::downgrade(&stream), self.peaks.clone());
        stream.borrow_mut().set_read_callback(Some(Box::new(move |_| {
            let Some(s) = weak.upgrade() else { return };
            let Ok(mut s) = s.try_borrow_mut() else { return };
            loop {
                match s.peek() {
                    Ok(PeekResult::Data(data)) => {
                        let peak = data.chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]).abs()).fold(0.0, f32::max);
                        let mut peaks = peaks.borrow_mut();
                        let slot = peaks.entry(key).or_insert(0.0);
                        *slot = slot.max(peak);
                    }
                    Ok(PeekResult::Hole(_)) => {}
                    Ok(PeekResult::Empty) | Err(_) => break,
                }
                if s.discard().is_err() {
                    break;
                }
            }
        })));
        let attr = BufferAttr { maxlength: u32::MAX, tlength: u32::MAX, prebuf: u32::MAX, minreq: u32::MAX, fragsize: 4 };
        let flags = StreamFlags::DONT_MOVE | StreamFlags::PEAK_DETECT | StreamFlags::ADJUST_LATENCY | StreamFlags::DONT_INHIBIT_AUTO_SUSPEND;
        let connected = stream.borrow_mut().connect_record(Some(&source.to_string()), Some(&attr), flags);
        let monitor = Monitor { stream, source };
        connected.ok().map(|()| monitor)
    }

    /// 직전 구간의 레벨(앱별 최댓값, 출력 장치별 값)을 꺼내고 구간을 비운다
    fn levels(&mut self) -> (BTreeMap<String, f64>, BTreeMap<String, f64>) {
        let peaks = std::mem::take(&mut *self.peaks.borrow_mut());
        let now = &self.model.borrow().now;
        let level = |key| round(peaks.get(&key).copied().unwrap_or(0.0).min(1.0) as f64, 100.0);
        let mut apps = BTreeMap::new();
        for i in &now.inputs {
            let v: &mut f64 = apps.entry(i.app.clone()).or_insert(0.0);
            *v = v.max(level(Key::Input(i.index)));
        }
        let outputs = now.sinks.iter().map(|s| (s.name.clone(), level(Key::Sink(s.index)))).collect();
        (apps, outputs)
    }

    fn act(&mut self, name: &str, args: &Value) -> Result<(), String> {
        let now = self.model.borrow().now.clone();
        let app = args["app"].as_str();
        let inputs: Vec<&Input> = app.map(|a| now.inputs.iter().filter(|i| i.app == a).collect()).unwrap_or_default();
        if app.is_some() && inputs.is_empty() {
            return Err("해당 앱이 소리를 내고 있지 않습니다".to_string());
        }
        let sink = match args["output"].as_str() {
            Some(id) => Some(now.sinks.iter().find(|s| s.name == id).ok_or("해당 출력 장치가 없습니다")?),
            None => None,
        };
        let mut intro = self.context.introspect();
        match (name, app.is_some(), sink) {
            (ACTION_AUDIO_VOLUME, _, _) => {
                let v = args["volume"].as_f64().filter(|v| v.is_finite()).ok_or("volume 값이 없습니다")?.clamp(0.0, 1.0);
                let target = Volume((v * Volume::NORMAL.0 as f64).round() as u32);
                if app.is_some() {
                    for i in inputs {
                        let mut cv = i.volume;
                        cv.scale(target);
                        intro.set_sink_input_volume(i.index, &cv, None);
                    }
                } else if let Some(s) = sink {
                    let mut cv = s.volume;
                    cv.scale(target);
                    intro.set_sink_volume_by_name(&s.name, &cv, None);
                } else {
                    return Err("app이나 output이 필요합니다".to_string());
                }
            }
            (ACTION_AUDIO_MUTE, _, _) => {
                let on = args["on"].as_bool().ok_or("on 값이 없습니다")?;
                if app.is_some() {
                    for i in inputs {
                        intro.set_sink_input_mute(i.index, on, None);
                    }
                } else if let Some(s) = sink {
                    intro.set_sink_mute_by_name(&s.name, on, None);
                } else {
                    return Err("app이나 output이 필요합니다".to_string());
                }
            }
            (ACTION_AUDIO_ROUTE, true, Some(s)) => {
                for i in inputs {
                    intro.move_sink_input_by_name(i.index, &s.name, None);
                }
            }
            (ACTION_AUDIO_DEFAULT, false, Some(s)) => {
                self.context.set_default_sink(&s.name, |_| {});
            }
            (ACTION_AUDIO_ROUTE | ACTION_AUDIO_DEFAULT, _, _) => return Err("app이나 output 값이 맞지 않습니다".to_string()),
            _ => return Err(format!("알 수 없는 음량 액션: {name}")),
        }
        self.model.borrow_mut().dirty = true;
        Ok(())
    }
}

fn volume_of(cv: &ChannelVolumes) -> f64 {
    round(cv.max().0 as f64 / Volume::NORMAL.0 as f64, 1000.0)
}

/// 서버 상태를 덱에 보낼 형태로 바꾼다. 앱은 처음 나타난 스트림 순서대로 둔다
fn audio_of(now: &Snapshot) -> Audio {
    let sink_name = |index: u32| now.sinks.iter().find(|s| s.index == index).map(|s| s.name.clone()).unwrap_or_default();
    let outputs = now
        .sinks
        .iter()
        .map(|s| AudioOutput { id: s.name.clone(), title: s.title.clone(), volume: volume_of(&s.volume), muted: s.mute, default: s.name == now.default_sink })
        .collect();
    let mut inputs: Vec<&Input> = now.inputs.iter().collect();
    inputs.sort_by_key(|i| i.index);
    let mut apps: Vec<AudioApp> = Vec::new();
    for i in inputs {
        let output = sink_name(i.sink);
        let room = apps.len() < APPS_MAX;
        match apps.iter_mut().find(|a| a.id == i.app) {
            Some(a) => {
                a.volume = a.volume.max(volume_of(&i.volume));
                a.muted &= i.mute;
                if a.output != output {
                    a.output.clear();
                }
                a.streams += 1;
            }
            None if room => apps.push(AudioApp { id: i.app.clone(), name: i.name.clone(), volume: volume_of(&i.volume), muted: i.mute, output, streams: 1 }),
            None => {}
        }
    }
    Audio { outputs, apps }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cv(v: f64) -> ChannelVolumes {
        let mut cv = ChannelVolumes::default();
        cv.set(2, Volume((v * Volume::NORMAL.0 as f64) as u32));
        cv
    }

    #[test]
    fn a_stream_without_a_name_takes_its_clients() {
        let said = |a: &str, b: &str| (a.to_string(), b.to_string(), "Playback Stream".to_string());
        let input = |index, said, client| Input { index, app: String::new(), name: String::new(), said, client, sink: 1, volume: cv(1.0), mute: false };
        let mut now = Snapshot {
            inputs: vec![input(5, said("", ""), Some(3)), input(6, said("Firefox", "firefox"), Some(4)), input(7, said("", ""), None)],
            clients: HashMap::from([(3, (".AyuGram-wrapped".to_string(), ".AyuGram-wrapped".to_string())), (4, ("Other".to_string(), "other".to_string()))]),
            ..Snapshot::default()
        };
        now.name_inputs();
        let got: Vec<_> = now.inputs.iter().map(|i| (i.app.as_str(), i.name.as_str())).collect();
        assert_eq!(got, [("ayugram", "AyuGram"), ("firefox", "Firefox"), ("playback stream", "Playback Stream")]);
    }

    #[test]
    fn streams_of_one_app_are_shown_as_one() {
        let sink = |index, name: &str| Sink { index, name: name.into(), title: name.to_uppercase(), volume: cv(1.0), mute: false, monitor: 100 + index };
        let input = |index, app: &str, sink, v, mute| Input { index, app: app.into(), name: app.into(), said: Default::default(), client: None, sink, volume: cv(v), mute };
        let now = Snapshot {
            default_sink: "dac".into(),
            sinks: vec![sink(1, "dac"), sink(2, "hdmi")],
            inputs: vec![input(9, "discord", 1, 0.5, true), input(7, "firefox", 1, 0.25, false), input(8, "firefox", 2, 0.75, true), input(10, "discord", 1, 0.25, true)],
            ..Snapshot::default()
        };
        let a = audio_of(&now);
        assert_eq!(a.outputs.iter().map(|o| (o.id.as_str(), o.default, o.volume)).collect::<Vec<_>>(), [("dac", true, 1.0), ("hdmi", false, 1.0)]);
        let apps: Vec<_> = a.apps.iter().map(|a| (a.id.as_str(), a.volume, a.muted, a.output.as_str(), a.streams)).collect();
        assert_eq!(apps, [("firefox", 0.75, false, "", 2), ("discord", 0.5, true, "dac", 2)]);
    }
}
