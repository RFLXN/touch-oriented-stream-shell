//! Windows Core Audio(WASAPI) 음량 믹서: 재생 장치(엔드포인트)와 장치별 오디오 세션(앱)을 읽고 음량·음소거·기본 장치를 바꾼다.
//!
//! 이 스레드를 MTA로 초기화하고 모든 COM 객체를 이 스레드에서만 쓴다.
//!   - 상태: READ_EVERY마다 장치와 세션을 다시 읽고, 덱에 보낸 것과 다를 때만 전체를 보낸다(변경 알림 COM 콜백 대신 폴링).
//!   - 레벨: 덱이 구독 중일 때만 TURN마다 세션·장치의 피크 미터(IAudioMeterInformation)를 읽어 구간 최댓값을 모으고,
//!     AUDIO_LEVEL_EVERY마다 바뀐 것만 보낸다.
//!   - 세션은 프로세스마다 생기므로 실행 파일 이름으로 앱을 묶는다(`app_of`). 시스템 소리 세션은 `system` 앱 하나로 둔다.
//!   - 기본 출력 장치 변경은 공개 API가 없어 Windows 소리 설정과 같은 비공개 `IPolicyConfig`를 쓴다(콘솔·멀티미디어 역할).
//!   - 앱별 출력 장치 변경(audio.route)은 공개 API가 없어 지원하지 않는다.

use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::Value;
use toss_proto::{Audio, AudioApp, AudioOutput, ACTION_AUDIO_DEFAULT, ACTION_AUDIO_MUTE, ACTION_AUDIO_ROUTE, ACTION_AUDIO_VOLUME, AUDIO_LEVEL_EVERY};
use windows::core::{Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, S_OK};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

use super::{app_of, changed, round, Cmd, Ended, Event};

/// 장치·세션을 다시 읽는 주기
const READ_EVERY: Duration = Duration::from_millis(500);
/// 메인 루프 한 바퀴(요청 대기 겸 레벨 측정 간격)
const TURN: Duration = Duration::from_millis(25);
/// 덱에 보낼 앱 최대 수
const APPS_MAX: usize = 64;
/// 시스템 소리 세션의 앱 키와 표시 이름
const SYSTEM_APP: (&str, &str) = ("system", "System sounds");

/// 비공개 COM 인터페이스: Windows 소리 설정이 기본 장치를 바꿀 때 쓴다(Windows 7 이후 같은 배치). 앞의 메서드는 순서만 맞추면 된다.
/// 메서드 이름은 COM 관례를 따르므로 이 모듈에서만 이름 경고를 끈다.
mod policy {
    #![allow(non_snake_case)]
    use std::ffi::c_void;
    use windows::core::{HRESULT, PCWSTR};

    #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: windows::core::IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, default_period: *mut i64, min_period: *mut i64) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(&self, device: PCWSTR, store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetPropertyValue(&self, device: PCWSTR, store: i32, key: *const c_void, value: *mut c_void) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: i32) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    /// 매크로가 만든 메서드는 이 모듈 밖에서 보이지 않으므로 필요한 것만 내보낸다
    pub fn set_default_endpoint(config: &IPolicyConfig, device: PCWSTR, role: i32) -> HRESULT {
        unsafe { config.SetDefaultEndpoint(device, role) }
    }
}
use policy::{set_default_endpoint, IPolicyConfig};

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

struct Output {
    id: String,
    title: String,
    volume: f64,
    muted: bool,
    control: IAudioEndpointVolume,
    meter: Option<IAudioMeterInformation>,
}

struct Session {
    app: String,
    name: String,
    /// 이 세션이 나가는 장치(Output.id)
    output: String,
    volume: f64,
    muted: bool,
    /// 앱이 처음 나타난 순서(앱 정렬용)
    order: u64,
    control: ISimpleAudioVolume,
    meter: Option<IAudioMeterInformation>,
}

pub(super) struct Server {
    devices: IMMDeviceEnumerator,
    outputs: Vec<Output>,
    sessions: Vec<Session>,
    default: String,
    /// 앱 키별 처음 본 순서
    seen: HashMap<String, u64>,
    /// 구간 최댓값(앱별, 장치별)
    peaks: (BTreeMap<String, f64>, BTreeMap<String, f64>),
}

/// COM이 할당한 문자열을 String으로 바꾸고 해제한다
fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

/// 프로세스의 실행 파일 이름(확장자 제외). 읽을 수 없으면 빈 문자열
fn exe_name(pid: u32) -> String {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return String::new() };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    let _ = unsafe { CloseHandle(handle) };
    if !ok {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let file = path.rsplit('\\').next().unwrap_or(&path);
    file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string()
}

fn device_name(device: &IMMDevice) -> String {
    unsafe { device.OpenPropertyStore(STGM_READ) }.and_then(|store| unsafe { store.GetValue(&PKEY_Device_FriendlyName) }).map(|v| v.to_string()).unwrap_or_default()
}

impl Server {
    pub(super) fn connect() -> Option<Self> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let devices: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()?;
        let mut me = Self { devices, outputs: Vec::new(), sessions: Vec::new(), default: String::new(), seen: HashMap::new(), peaks: Default::default() };
        me.read().ok()?;
        Some(me)
    }

    pub(super) fn serve(&mut self, rx: &Receiver<Cmd>, watch: &mut bool, on_event: &impl Fn(Event)) -> Ended {
        let mut sent: Option<Audio> = None;
        let mut sent_levels: (BTreeMap<String, f64>, BTreeMap<String, f64>) = Default::default();
        let mut next_read = Instant::now();
        let mut next_levels = Instant::now();
        loop {
            let mut first = Some(rx.recv_timeout(TURN));
            loop {
                let got = match first.take() {
                    Some(Ok(c)) => Ok(c),
                    Some(Err(RecvTimeoutError::Timeout)) => break,
                    Some(Err(RecvTimeoutError::Disconnected)) => return Ended::Stopped,
                    None => rx.try_recv(),
                };
                match got {
                    Ok(Cmd::Watch(on)) => {
                        *watch = on;
                        sent_levels = Default::default();
                        self.peaks = Default::default();
                    }
                    Ok(Cmd::Act(name, args, reply)) => {
                        let _ = reply.send(self.act(&name, &args));
                        next_read = Instant::now();
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ended::Stopped,
                }
            }
            if Instant::now() >= next_read {
                next_read = Instant::now() + READ_EVERY;
                if self.read().is_err() {
                    return Ended::Lost;
                }
                let audio = self.audio();
                if sent.as_ref() != Some(&audio) {
                    on_event(Event::State(Some(audio.clone())));
                    sent = Some(audio);
                }
            }
            if *watch {
                self.measure();
                if Instant::now() >= next_levels {
                    next_levels = Instant::now() + AUDIO_LEVEL_EVERY;
                    let (apps, outputs) = std::mem::take(&mut self.peaks);
                    let apps = changed(&apps, &mut sent_levels.0);
                    let outputs = changed(&outputs, &mut sent_levels.1);
                    if !apps.is_empty() || !outputs.is_empty() {
                        on_event(Event::Levels { apps, outputs });
                    }
                }
            }
        }
    }

    /// 재생 장치, 기본 장치, 장치별 세션을 다시 읽는다. 장치 목록조차 읽을 수 없으면(오디오 서비스 재시작 등) 오류
    fn read(&mut self) -> windows::core::Result<()> {
        let list = unsafe { self.devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }?;
        self.default = unsafe { self.devices.GetDefaultAudioEndpoint(eRender, eMultimedia) }.and_then(|d| unsafe { d.GetId() }).map(take).unwrap_or_default();
        let (mut outputs, mut sessions) = (Vec::new(), Vec::new());
        for i in 0..unsafe { list.GetCount() }? {
            let Ok(device) = (unsafe { list.Item(i) }) else { continue };
            let Ok(id) = (unsafe { device.GetId() }).map(take) else { continue };
            let Ok(control) = (unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }) else { continue };
            let volume = unsafe { control.GetMasterVolumeLevelScalar() }.unwrap_or(0.0) as f64;
            let muted = unsafe { control.GetMute() }.is_ok_and(|m| m.as_bool());
            let meter = unsafe { device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) }.ok();
            if let Ok(manager) = unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) } {
                self.read_sessions(&manager, &id, &mut sessions);
            }
            outputs.push(Output { id, title: device_name(&device), volume: round(volume, 1000.0), muted, control, meter });
        }
        self.outputs = outputs;
        self.sessions = sessions;
        Ok(())
    }

    fn read_sessions(&mut self, manager: &IAudioSessionManager2, output: &str, out: &mut Vec<Session>) {
        let Ok(list) = (unsafe { manager.GetSessionEnumerator() }) else { return };
        for j in 0..unsafe { list.GetCount() }.unwrap_or(0) {
            let Ok(session) = (unsafe { list.GetSession(j) }) else { continue };
            if unsafe { session.GetState() }.is_ok_and(|s| s == AudioSessionStateExpired) {
                continue;
            }
            let Ok(info) = session.cast::<IAudioSessionControl2>() else { continue };
            let Ok(control) = session.cast::<ISimpleAudioVolume>() else { continue };
            let (app, name) = if unsafe { info.IsSystemSoundsSession() } == S_OK {
                (SYSTEM_APP.0.to_string(), SYSTEM_APP.1.to_string())
            } else {
                let pid = unsafe { info.GetProcessId() }.unwrap_or(0);
                // 표시 이름이 `@%SystemRoot%\…,-123` 같은 리소스 참조면 쓰지 않는다
                let shown = unsafe { session.GetDisplayName() }.map(take).unwrap_or_default();
                let shown = if shown.starts_with('@') { String::new() } else { shown };
                app_of(&shown, &exe_name(pid), "", pid)
            };
            let next = self.seen.len() as u64;
            let order = *self.seen.entry(app.clone()).or_insert(next);
            let volume = unsafe { control.GetMasterVolume() }.unwrap_or(0.0) as f64;
            let muted = unsafe { control.GetMute() }.is_ok_and(|m| m.as_bool());
            let meter = session.cast::<IAudioMeterInformation>().ok();
            out.push(Session { app, name, output: output.to_string(), volume: round(volume, 1000.0), muted, order, control, meter });
        }
    }

    /// 덱에 보낼 형태로 바꾼다. 앱은 처음 나타난 순서대로 둔다
    fn audio(&self) -> Audio {
        let outputs = self.outputs.iter().map(|o| AudioOutput { id: o.id.clone(), title: o.title.clone(), volume: o.volume, muted: o.muted, default: o.id == self.default }).collect();
        let mut sessions: Vec<&Session> = self.sessions.iter().collect();
        sessions.sort_by_key(|s| s.order);
        let mut apps: Vec<AudioApp> = Vec::new();
        for s in sessions {
            let room = apps.len() < APPS_MAX;
            match apps.iter_mut().find(|a| a.id == s.app) {
                Some(a) => {
                    a.volume = a.volume.max(s.volume);
                    a.muted &= s.muted;
                    if a.output != s.output {
                        a.output.clear();
                    }
                    a.streams += 1;
                }
                None if room => {
                    apps.push(AudioApp { id: s.app.clone(), name: s.name.clone(), volume: s.volume, muted: s.muted, output: s.output.clone(), streams: 1 })
                }
                None => {}
            }
        }
        Audio { outputs, apps }
    }

    /// 피크 미터를 읽어 구간 최댓값에 더한다
    fn measure(&mut self) {
        let peak = |m: &Option<IAudioMeterInformation>| m.as_ref().and_then(|m| unsafe { m.GetPeakValue() }.ok()).unwrap_or(0.0).clamp(0.0, 1.0) as f64;
        for s in &self.sessions {
            let v = self.peaks.0.entry(s.app.clone()).or_insert(0.0);
            *v = v.max(round(peak(&s.meter), 100.0));
        }
        for o in &self.outputs {
            let v = self.peaks.1.entry(o.id.clone()).or_insert(0.0);
            *v = v.max(round(peak(&o.meter), 100.0));
        }
    }

    fn act(&mut self, name: &str, args: &Value) -> Result<(), String> {
        let app = args["app"].as_str();
        let sessions: Vec<&Session> = app.map(|a| self.sessions.iter().filter(|s| s.app == a).collect()).unwrap_or_default();
        if app.is_some() && sessions.is_empty() {
            return Err("해당 앱이 소리를 내고 있지 않습니다".to_string());
        }
        let output = match args["output"].as_str() {
            Some(id) => Some(self.outputs.iter().find(|o| o.id == id).ok_or("해당 출력 장치가 없습니다")?),
            None => None,
        };
        let failed = |e: windows::core::Error| e.message();
        match (name, app.is_some(), output) {
            (ACTION_AUDIO_VOLUME, _, _) => {
                let v = args["volume"].as_f64().filter(|v| v.is_finite()).ok_or("volume 값이 없습니다")?.clamp(0.0, 1.0) as f32;
                if app.is_some() {
                    for s in sessions {
                        unsafe { s.control.SetMasterVolume(v, std::ptr::null()) }.map_err(failed)?;
                    }
                } else if let Some(o) = output {
                    unsafe { o.control.SetMasterVolumeLevelScalar(v, std::ptr::null()) }.map_err(failed)?;
                } else {
                    return Err("app이나 output이 필요합니다".to_string());
                }
            }
            (ACTION_AUDIO_MUTE, _, _) => {
                let on = args["on"].as_bool().ok_or("on 값이 없습니다")?;
                if app.is_some() {
                    for s in sessions {
                        unsafe { s.control.SetMute(on, std::ptr::null()) }.map_err(failed)?;
                    }
                } else if let Some(o) = output {
                    unsafe { o.control.SetMute(on, std::ptr::null()) }.map_err(failed)?;
                } else {
                    return Err("app이나 output이 필요합니다".to_string());
                }
            }
            (ACTION_AUDIO_ROUTE, true, Some(_)) => return Err("Windows에서는 앱별 출력 장치를 바꿀 수 없습니다".to_string()),
            (ACTION_AUDIO_DEFAULT, false, Some(o)) => set_default(&o.id)?,
            (ACTION_AUDIO_ROUTE | ACTION_AUDIO_DEFAULT, _, _) => return Err("app이나 output 값이 맞지 않습니다".to_string()),
            _ => return Err(format!("알 수 없는 음량 액션: {name}")),
        }
        Ok(())
    }
}

/// 기본 출력 장치를 바꾼다(콘솔·멀티미디어 역할. Windows 소리 설정에서 기본 장치를 고를 때와 같다)
fn set_default(id: &str) -> Result<(), String> {
    let config: IPolicyConfig = unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) }.map_err(|e| format!("기본 장치를 바꿀 수 없습니다: {}", e.message()))?;
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    for role in [eConsole, eMultimedia] {
        set_default_endpoint(&config, PCWSTR(wide.as_ptr()), role.0).ok().map_err(|e| format!("기본 장치를 바꿀 수 없습니다: {}", e.message()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 PC의 실제 장치와 앱을 읽어 출력한다(값은 바꾸지 않는다): `cargo test show_this_pc_audio -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn show_this_pc_audio() {
        let mut s = Server::connect().expect("Core Audio에 연결할 수 없음");
        println!("{:#?}", s.audio());
        for _ in 0..8 {
            s.measure();
            std::thread::sleep(TURN);
        }
        println!("레벨: {:?}", s.peaks);
        // 현재 기본 장치를 다시 기본으로 지정해 IPolicyConfig 호출 경로만 확인한다.
        // 콘솔과 멀티미디어 역할의 기본 장치가 같을 때만 하므로 설정은 바뀌지 않는다
        let current = s.default.clone();
        let console = unsafe { s.devices.GetDefaultAudioEndpoint(eRender, eConsole) }.and_then(|d| unsafe { d.GetId() }).map(take).unwrap_or_default();
        if console == current {
            set_default(&current).unwrap();
            s.read().unwrap();
            assert_eq!(s.default, current);
            println!("IPolicyConfig 호출 확인");
        }
    }
}
