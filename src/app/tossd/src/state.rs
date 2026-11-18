//! 데몬 상태 관리: 모든 UI 클라이언트가 공유하는 `Shared` 상태, 수집 스레드, UI·호스트 요청 처리.
//!
//! 상태 필드마다 버전 번호(`Versioned`)가 붙는다. 값이 바뀌면 버전이 증가하고, UI 서버(`server.rs`)는
//! 마지막으로 전송한 버전과 다를 때만 해당 필드를 다시 푸시한다. 상태를 채우는 측과 보내는 측이 완전히 분리돼 있다.
//!
//! 수집 스레드 (`spawn_feeds`):
//!   시스템·덱 상태   1초마다 수집 (`sys.rs`, `device.rs`)
//!   덱 자체 리소스   1초마다 리소스 미터 위젯이 구독 중인 항목만 측정 (`toss-meter`)
//!   Wi-Fi            `wifi.rs` 스레드가 상태 변경 시마다 갱신
//!   호스트           `hosts` 스레드들이 변경 시마다 갱신 (`hosts::Events`)

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use toss_proto::MetricInfo;

use crate::device::{Device, DeviceState};
use crate::hosts::{self, Hosts, HostsState};
use crate::layout::{self, Edit, Layout, SELF_HOST};
use crate::sys::{collect_sys, SysState};
use crate::wifi::{self, WifiCmd, WifiState};

/// 수집 가능한 메트릭 목록: 대상(`SELF_HOST` 또는 호스트 ID) → 목록
pub type MeterInfos = BTreeMap<String, Vec<MetricInfo>>;
/// 메트릭 현재 값: 대상 → 메트릭 ID → 값
pub type MeterValues = BTreeMap<String, BTreeMap<String, f64>>;

/// 덱 자체 메트릭 목록을 재탐색하는 주기 (디스크 탈착 등 반영)
const CATALOG_EVERY: Duration = Duration::from_secs(30);
/// SSH 토글 요청 후 실제 상태 전환을 폴링하는 횟수와 간격
const SSH_LOOKS: u32 = 30;
const SSH_LOOK_EVERY: Duration = Duration::from_millis(100);

/// 버전 번호가 붙은 상태 컨테이너. 값의 락을 잡은 상태에서 버전을 올리므로,
/// 버전만 먼저 올라가서 `fresh()`가 새 값을 놓치는 경쟁 상태가 발생하지 않는다.
#[derive(Default)]
pub struct Versioned<T> {
    value: Mutex<T>,
    ver: AtomicU64,
}

impl<T> Versioned<T> {
    pub fn new(value: T) -> Self {
        Self { value: Mutex::new(value), ver: AtomicU64::new(0) }
    }

    /// 내부 값의 잠금을 획득한다.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.value.lock().unwrap()
    }

    /// 값을 갱신한다. `change`가 `true`를 반환하면 변경된 것으로 간주해 버전을 올린다.
    pub fn update(&self, change: impl FnOnce(&mut T) -> bool) {
        self.update_with(|value| (change(value), ()));
    }

    /// `update`와 같되 `change`가 `(변경 여부, 부가 반환값)`을 반환한다.
    pub fn update_with<R>(&self, change: impl FnOnce(&mut T) -> (bool, R)) -> R {
        let mut value = self.value.lock().unwrap();
        let (changed, out) = change(&mut value);
        if changed {
            self.ver.fetch_add(1, Ordering::SeqCst);
        }
        out
    }

    /// `seen`이 현재 버전과 다르면(이후 값이 변경됐으면) 클론한 값을 반환하고 `seen`을 최신 버전으로 갱신한다.
    pub fn fresh(&self, seen: &mut u64) -> Option<T>
    where
        T: Clone,
    {
        if self.ver.load(Ordering::SeqCst) == *seen {
            return None;
        }
        let value = self.value.lock().unwrap();
        *seen = self.ver.load(Ordering::SeqCst);
        Some(value.clone())
    }

    /// 값을 바꿔 넣는다. 같은 값이면 번호를 올리지 않는다
    pub fn set(&self, value: T)
    where
        T: PartialEq,
    {
        self.update(|cur| {
            let changed = *cur != value;
            if changed {
                *cur = value;
            }
            changed
        });
    }
}

/// 위젯이 호스트에 요청한 액션 결과: 위젯마다 마지막 결과 1건을 유지하며, `seq`는 결과마다 증가한다.
#[derive(Clone, Default)]
pub struct Results {
    seq: u64,
    last: HashMap<u32, (u64, bool, String)>,
}

impl Results {
    /// 마지막 결과의 시퀀스 번호
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// `seq` 이후에 기록된 결과를 발생 순서대로 반환: `(widget, seq, ok, error)`
    pub fn since(&self, seq: u64) -> Vec<(u32, u64, bool, &str)> {
        let mut fresh: Vec<_> = self.last.iter().filter(|(_, r)| r.0 > seq).map(|(widget, r)| (*widget, r.0, r.1, r.2.as_str())).collect();
        fresh.sort_by_key(|r| r.1);
        fresh
    }
}

/// 모든 UI 연결이 공유하는 전역 상태
#[derive(Default)]
pub struct Shared {
    pub sys: Versioned<SysState>,
    pub wifi: Versioned<WifiState>,
    /// Wi-Fi 제어 스레드에 명령을 전달하는 채널
    wifi_tx: OnceLock<Sender<WifiCmd>>,
    /// 페어링된 호스트(PC) 목록과 페어링 진행 상태
    pub hosts: Versioned<HostsState>,
    /// 호스트 관리 핸들(호스트 수신을 비활성화한 경우 `None`)
    hosts_ctl: OnceLock<Arc<Hosts>>,
    /// 호스트에 등록된 스위치/노브의 현재 값
    pub values: Versioned<hosts::Values>,
    /// 시스템 자원: 측정 가능한 메트릭 목록과 현재 값. 키는 덱 자신(`SELF_HOST`) 또는 호스트 ID다.
    pub meter_infos: Versioned<MeterInfos>,
    pub meter_values: Versioned<MeterValues>,
    /// 덱 자체 자원 중 자원 미터 위젯이 현재 표시 중인 항목(이 항목만 측정한다)
    own_watch: Mutex<Vec<String>>,
    /// 위젯 배치. UI 편집 모드에서 수정한다(`edit_layout`)
    pub layout: Versioned<Layout>,
    layout_store: layout::Store,
    /// 배치 편집 직렬화용 뮤텍스: 편집, 저장, watch 목록 갱신을 한 번에 하나씩 순서대로 처리한다.
    editing: Mutex<()>,
    /// 기기 제어 핸들(화면 밝기, 전원, SSH)
    device: Device,
    pub device_state: Versioned<DeviceState>,
    /// 위젯이 호스트에 요청한 액션 결과
    pub results: Versioned<Results>,
}

impl Shared {
    pub fn new(layout: Layout, layout_store: layout::Store, device: Device) -> Self {
        Self { layout: Versioned::new(layout), layout_store, device, ..Self::default() }
    }

    /// 배치를 수정한다. 규칙에 맞으면 반영해 저장하고, 위반이면 거절한다. 어느 쪽이든 모든 UI 클라이언트에 현재 배치를 전송해
    /// 클라이언트 측 임시 UI 상태를 바로잡는다.
    /// `layout` 락은 실제 수정 순간에만 짧게 잡고, 파일 저장이나 호스트 알림 동안 다른 클라이언트가 대기하지 않게 한다.
    /// 다만 여러 클라이언트가 동시에 편집할 때 저장/알림 순서가 뒤바뀌지 않도록 작업 전체를 `editing` 뮤텍스로 직렬화한다.
    pub fn edit_layout(&self, edit: Edit) {
        let _editing = self.editing.lock().unwrap();
        let (accepted, watched) = self.layout.update_with(|layout| {
            let accepted = match layout.edit(&edit) {
                Ok(()) => Some(layout.clone()),
                Err(why) => {
                    eprintln!("layout: 편집 거절({why}): {edit:?}");
                    None
                }
            };
            (true, (accepted, layout.watched()))
        });
        if let Some(layout) = accepted {
            self.layout_store.save(&layout);
            eprintln!("layout: 편집 반영: {edit:?}");
        }
        self.watch(watched);
    }

    /// 현재 배치를 모든 UI 클라이언트에 다시 전송한다(알 수 없는 편집 메시지를 받았을 때 임시 UI 상태를 되돌리기 위함).
    pub fn resend_layout(&self) {
        self.layout.update(|_| true);
    }

    /// 자원 미터 위젯이 표시 중인 항목을 갱신한다: 덱 항목은 자체 측정 스레드에, 호스트 항목은 해당 호스트에 전달한다.
    fn watch(&self, mut watched: BTreeMap<String, Vec<String>>) {
        *self.own_watch.lock().unwrap() = watched.remove(SELF_HOST).unwrap_or_default();
        if let Some(hosts) = self.hosts_ctl.get() {
            hosts.set_watch(watched);
        }
    }

    /// 위젯 액션 결과를 기록한다(모든 UI 클라이언트에 전파된다).
    pub fn set_result(&self, widget: u32, ok: bool, error: String) {
        self.results.update(|r| {
            r.seq += 1;
            r.last.insert(widget, (r.seq, ok, error));
            true
        });
    }

    /// 호스트 제어 핸들 반환
    pub fn hosts_ctl(&self) -> Option<&Arc<Hosts>> {
        self.hosts_ctl.get()
    }

    /// 위젯이 호스트에 액션을 요청한다. 대상 호스트가 연결되어 있지 않아 전송하지 못하면 해당 위젯 앞으로 실패 결과를 기록한다.
    pub fn action(&self, host: &str, name: &str, args: serde_json::Value, widget: Option<u32>) {
        let sent = self.hosts_ctl.get().is_some_and(|hosts| hosts.action(host, name, args, widget));
        if let (false, Some(widget)) = (sent, widget) {
            self.set_result(widget, false, "no_host".into());
        }
    }

    /// 기기 상태를 다시 읽어 변경이 있으면 UI 클라이언트에 알린다.
    pub fn refresh_device(&self) {
        self.device_state.set(self.device.state());
    }

    pub fn set_brightness(&self, value: u32, keep: bool) {
        if let Err(e) = self.device.set_brightness(value, keep) {
            eprintln!("device: {e}");
        }
        self.refresh_device();
    }

    pub fn power_off(&self) {
        match self.device.power_off() {
            Ok(()) => eprintln!("device: 전원 종료 요청(UI)"),
            Err(e) => eprintln!("device: {e}"),
        }
    }

    /// SSH를 켜거나 끈다. 도우미가 처리를 마칠 때까지 잠시 대기 시간이 걸리므로 실제 상태가 반영될 때까지 짧게 폴링한다(UI 토글 반영용).
    pub fn set_ssh(self: &Arc<Self>, on: bool) {
        if let Err(e) = self.device.set_ssh(on) {
            eprintln!("device: {e}");
            return;
        }
        eprintln!("device: SSH {} 요청(UI)", if on { "활성화" } else { "비활성화" });
        let shared = self.clone();
        thread::spawn(move || {
            for _ in 0..SSH_LOOKS {
                thread::sleep(SSH_LOOK_EVERY);
                shared.refresh_device();
                if shared.device_state.lock().ssh == on {
                    break;
                }
            }
        });
    }

    /// 덱을 절전 상태로 전환한다. Wi-Fi 매직 패킷으로만 깨울 수 있으므로 Wi-Fi에 연결된 상태에서만 허용한다.
    /// 실패 시 사유 코드(`toss-proto`의 `SLEEP_*`)를 반환한다.
    pub fn try_sleep(&self) -> Result<(), String> {
        if self.wifi.lock().phase != "connected" {
            return Err(toss_proto::SLEEP_NO_WIFI.into());
        }
        self.device.sleep().map_err(|e| {
            eprintln!("device: {e}");
            toss_proto::SLEEP_UNAVAILABLE.to_string()
        })?;
        eprintln!("device: 절전 진입");
        Ok(())
    }

    pub fn wifi_cmd(&self, cmd: WifiCmd) {
        if let Some(tx) = self.wifi_tx.get() {
            let _ = tx.send(cmd);
        }
    }
}

/// 덱 자체(`SELF_HOST`) 항목은 유지한 채 호스트 항목만 일괄 교체한다. 변경되었으면 `true`.
fn replace_hosts<V: PartialEq>(cur: &mut BTreeMap<String, V>, hosts: BTreeMap<String, V>) -> bool {
    let own = usize::from(cur.contains_key(SELF_HOST));
    let changed = cur.len() != hosts.len() + own || hosts.iter().any(|(host, v)| cur.get(host) != Some(v));
    if changed {
        cur.retain(|host, _| host == SELF_HOST);
        cur.extend(hosts);
    }
    changed
}

/// 덱 자체(`SELF_HOST`) 항목을 교체한다. 변경되었으면 `true`.
fn replace_own<V: PartialEq>(cur: &mut BTreeMap<String, V>, own: V) -> bool {
    let changed = cur.get(SELF_HOST) != Some(&own);
    if changed {
        cur.insert(SELF_HOST.to_string(), own);
    }
    changed
}

impl hosts::Events for Shared {
    fn hosts(&self, state: HostsState) {
        self.hosts.set(state);
    }

    fn values(&self, values: hosts::Values) {
        self.values.set(values);
    }

    fn meters(&self, meters: hosts::Meters) {
        let (mut infos, mut values) = (MeterInfos::new(), MeterValues::new());
        for (host, (list, now)) in meters {
            infos.insert(host.clone(), list);
            values.insert(host, now);
        }
        self.meter_infos.update(|cur| replace_hosts(cur, infos));
        self.meter_values.update(|cur| replace_hosts(cur, values));
    }

    fn result(&self, widget: u32, ok: bool, error: String) {
        self.set_result(widget, ok, error);
    }

    fn sleep(&self) -> Result<(), String> {
        self.try_sleep()
    }
}

/// 1초마다 시스템 상태와 기기 상태를 갱신한다.
fn feed_sys(shared: Arc<Shared>) {
    thread::spawn(move || loop {
        shared.sys.set(collect_sys());
        shared.refresh_device();
        thread::sleep(Duration::from_secs(1));
    });
}

/// 덱 자체 자원 측정: 측정 가능한 항목 목록을 주기적으로 갱신하고, 1초마다 자원 미터 위젯이 표시 중인 항목만 샘플링한다.
fn feed_own_meters(shared: Arc<Shared>) {
    thread::spawn(move || {
        let mut sampler = toss_meter::Sampler::new();
        let mut listed: Option<Instant> = None;
        loop {
            if listed.is_none_or(|at| at.elapsed() >= CATALOG_EVERY) {
                listed = Some(Instant::now());
                let list = sampler.catalog();
                shared.meter_infos.update(|infos| replace_own(infos, list));
            }
            let watch = shared.own_watch.lock().unwrap().clone();
            let now = sampler.sample(&watch);
            shared.meter_values.update(|values| replace_own(values, now));
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// 백그라운드 상태 수집 스레드들을 시작한다.
/// `wpa_ctrl`은 `wpa_supplicant` 제어 소켓 경로(`wifi.rs`), `host`는 호스트 수신 주소와 덱 키/페어링 저장 디렉터리다
/// (`None`이면 호스트 수신을 비활성화한다).
pub fn spawn_feeds(shared: &Arc<Shared>, wpa_ctrl: PathBuf, host: Option<(SocketAddr, PathBuf)>) {
    shared.refresh_device();
    feed_sys(shared.clone());
    feed_own_meters(shared.clone());
    let wifi_state = shared.clone();
    let _ = shared.wifi_tx.set(wifi::spawn(wpa_ctrl, move |state| wifi_state.wifi.set(state)));
    if let Some((addr, data_dir)) = host {
        let _ = shared.hosts_ctl.set(hosts::spawn(addr, data_dir, crate::sys::hostname(), shared.clone()));
    }
    let watched = shared.layout.lock().watched();
    shared.watch(watched);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_moves_only_when_the_value_changes() {
        let v = Versioned::new(1u32);
        let mut seen = u64::MAX;
        assert_eq!(v.fresh(&mut seen), Some(1));
        assert_eq!(v.fresh(&mut seen), None);
        v.set(1);
        assert_eq!(v.fresh(&mut seen), None);
        v.set(2);
        assert_eq!(v.fresh(&mut seen), Some(2));
        assert_eq!(v.update_with(|n| (true, *n * 10)), 20);
        assert_eq!(v.fresh(&mut seen), Some(2));
        v.update(|n| {
            *n += 1;
            false
        });
        assert_eq!((v.fresh(&mut seen), *v.lock()), (None, 3));
    }

    /// 여러 스레드가 결과를 동시에 기록하는 중에도 구독 측은 누락 없이 시퀀스 순서대로 수신한다.
    #[test]
    fn no_result_is_skipped_while_others_are_being_written() {
        const WRITERS: u32 = 4;
        const EACH: u32 = 500;
        let shared = Arc::new(Shared::default());
        let writers: Vec<_> = (0..WRITERS)
            .map(|w| {
                let shared = shared.clone();
                thread::spawn(move || {
                    for i in 0..EACH {
                        shared.set_result(w * EACH + i, i.is_multiple_of(2), String::new());
                    }
                })
            })
            .collect();
        let (mut ver, mut seq, mut got) = (u64::MAX, 0u64, Vec::new());
        while got.len() < (WRITERS * EACH) as usize {
            if let Some(results) = shared.results.fresh(&mut ver) {
                for (widget, n, ok, _) in results.since(seq) {
                    assert!(n > seq, "시퀀스 번호 순서 역전");
                    assert_eq!(ok, (widget % EACH).is_multiple_of(2));
                    seq = n;
                    got.push(widget);
                }
            }
        }
        writers.into_iter().for_each(|w| w.join().unwrap());
        got.sort_unstable();
        assert_eq!(got, (0..WRITERS * EACH).collect::<Vec<_>>());
        assert_eq!(shared.results.lock().seq(), u64::from(WRITERS * EACH));
    }

    /// 호스트 메트릭이 교체되어도 덱 자체 항목은 유지되며, 동일 값 재적용 시 변경되지 않은 것으로 판단한다.
    #[test]
    fn host_meters_replace_each_other_but_not_the_decks_own() {
        let mut cur: BTreeMap<String, u32> = BTreeMap::new();
        assert!(replace_own(&mut cur, 1));
        assert!(!replace_own(&mut cur, 1));
        assert!(replace_hosts(&mut cur, [("a".to_string(), 2), ("b".to_string(), 3)].into()));
        assert!(!replace_hosts(&mut cur, [("a".to_string(), 2), ("b".to_string(), 3)].into()));
        assert!(replace_hosts(&mut cur, [("b".to_string(), 4)].into()));
        assert_eq!(cur, [(SELF_HOST.to_string(), 1), ("b".to_string(), 4)].into());
        assert!(replace_hosts(&mut cur, BTreeMap::new()));
        assert_eq!(cur.len(), 1);
    }
}
