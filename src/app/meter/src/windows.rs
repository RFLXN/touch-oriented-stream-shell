//! Windows 리소스 측정. 리눅스의 /proc·/sys 대신 다음을 쓴다.
//!
//!   cpu, cpu.N       PDH `\Processor(*)\% Processor Time` (전체는 `_Total`)
//!   cpu.freq         PDH `\Processor Information(_Total)\Processor Frequency`(기준 클럭) × `% Processor Performance`
//!   mem              GlobalMemoryStatusEx (전체 - 사용 가능)
//!   gpu[N], .vram    DXGI 어댑터(전용 메모리가 큰 순서) + PDH `\GPU Engine(*)\Utilization Percentage`, `\GPU Adapter Memory(*)\Dedicated Usage`
//!                    사용률은 작업 관리자처럼 엔진별(3D, 복사, 비디오 등)로 프로세스 합을 구한 뒤 가장 바쁜 엔진의 값을 쓴다
//!   disk.read/write  PDH `\PhysicalDisk(_Total)\Disk Read/Write Bytes/sec`
//!   fs.<드라이브>     고정 드라이브(C:, D: …)의 GetDiskFreeSpaceExW
//!   net.down/up      GetIfTable2의 하드웨어 인터페이스 송수신 바이트 합계 차이
//!   battery          GetSystemPowerStatus (배터리가 있을 때만)
//!
//! 온도·팬·전력은 표준 API가 없어(WMI는 관리자 권한이나 제조사 드라이버가 필요) 제공하지 않는다.
//! PDH 카운터 이름은 OS 언어와 무관하게 영어 이름으로 등록한다(PdhAddEnglishCounterW).
//! 속도·사용률 카운터는 두 번 수집해야 값이 나오므로 첫 측정에서는 0이 된다(리눅스 구현과 같은 동작).

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use toss_proto::MetricInfo;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_FMT_LARGE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::round;

/// GetDriveTypeW의 고정 디스크
const DRIVE_FIXED: u32 = 3;
/// 표시할 드라이브 최대 개수(리눅스 MOUNTS_MAX와 같은 취지)
const DRIVES_MAX: usize = 8;
/// FALSE(PDH_FMT_NOCAP100 없음)이면 100%를 넘는 값을 자른다. 엔진 합계는 자르지 않는다
const PDH_FMT_NOCAP100: u32 = 0x0000_8000;

fn info(id: &str, name: &str, title: &str, unit: &str, max: f64, decimals: u8) -> MetricInfo {
    MetricInfo { id: id.into(), name: name.into(), title: title.into(), unit: unit.into(), max, decimals }
}

/// PDH 쿼리 하나와 그 안의 카운터
struct Pdh {
    query: PDH_HQUERY,
    counters: HashMap<&'static str, PDH_HCOUNTER>,
    /// 마지막 수집 시각(한 번의 sample 호출에서 여러 번 수집하지 않도록)
    collected: Option<Instant>,
}

impl Pdh {
    fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        (unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } == 0).then(|| Self { query, counters: HashMap::new(), collected: None })
    }

    fn add(&mut self, key: &'static str, path: &str) -> bool {
        let mut counter = PDH_HCOUNTER::default();
        let ok = unsafe { PdhAddEnglishCounterW(self.query, &HSTRING::from(path), 0, &mut counter) } == 0;
        if ok {
            self.counters.insert(key, counter);
        }
        ok
    }

    fn collect(&mut self) {
        if self.collected.is_some_and(|t| t.elapsed().as_millis() < 50) {
            return;
        }
        unsafe { PdhCollectQueryData(self.query) };
        self.collected = Some(Instant::now());
    }

    fn value(&self, key: &str) -> Option<f64> {
        let counter = *self.counters.get(key)?;
        let mut v = PDH_FMT_COUNTERVALUE::default();
        let fmt = windows::Win32::System::Performance::PDH_FMT(PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100);
        (unsafe { PdhGetFormattedCounterValue(counter, fmt, None, &mut v) } == 0 && v.CStatus == 0).then(|| unsafe { v.Anonymous.doubleValue })
    }

    /// 와일드카드 카운터의 (인스턴스 이름, 값) 목록. 아직 값이 없는 인스턴스(첫 수집 직후의 사용률 등)는 None
    fn array(&self, key: &str, large: bool) -> Vec<(String, Option<f64>)> {
        let Some(&counter) = self.counters.get(key) else { return Vec::new() };
        let fmt = windows::Win32::System::Performance::PDH_FMT(if large { PDH_FMT_LARGE.0 } else { PDH_FMT_DOUBLE.0 | PDH_FMT_NOCAP100 });
        let (mut size, mut count) = (0u32, 0u32);
        if unsafe { PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, None) } != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // 항목 배열 뒤에 인스턴스 이름 문자열이 이어지므로 바이트 크기만큼 확보한다(정렬을 위해 u64 단위)
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        if unsafe { PdhGetFormattedCounterArrayW(counter, fmt, &mut size, &mut count, Some(items)) } != 0 {
            return Vec::new();
        }
        let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
        items
            .iter()
            .map(|i| {
                let name = unsafe { i.szName.to_string() }.unwrap_or_default();
                let v = (i.FmtValue.CStatus == 0).then(|| unsafe { if large { i.FmtValue.Anonymous.largeValue as f64 } else { i.FmtValue.Anonymous.doubleValue } });
                (name, v)
            })
            .collect()
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query) };
    }
}

struct Gpu {
    id: String,
    /// PDH 인스턴스 이름에 들어가는 어댑터 LUID(`luid_0x00000000_0x0000d1a2`, 소문자)
    luid: String,
}

#[derive(Default)]
pub struct State {
    pdh: Option<Pdh>,
    cores: Vec<String>,
    gpus: Vec<Gpu>,
    drives: Vec<(String, String)>,
    battery: bool,
    /// 직전 네트워크 누적 바이트(수신, 송신)와 측정 시각
    net: Option<(u64, u64, Instant)>,
    list: Vec<MetricInfo>,
}

impl State {
    pub fn catalog(&mut self) -> Vec<MetricInfo> {
        let mut list = Vec::new();
        // 카탈로그는 주기적으로 다시 만들어지므로 PDH 쿼리는 처음 한 번만 연다(다시 열면 사용률·속도가 0부터 다시 시작한다)
        if self.pdh.is_none() {
            self.pdh = Pdh::open().map(|mut p| {
                p.add("cpu", r"\Processor(*)\% Processor Time");
                p.add("freq.base", r"\Processor Information(_Total)\Processor Frequency");
                p.add("freq.perf", r"\Processor Information(_Total)\% Processor Performance");
                p.add("disk.read", r"\PhysicalDisk(_Total)\Disk Read Bytes/sec");
                p.add("disk.write", r"\PhysicalDisk(_Total)\Disk Write Bytes/sec");
                p.add("gpu.engine", r"\GPU Engine(*)\Utilization Percentage");
                p.add("gpu.mem", r"\GPU Adapter Memory(*)\Dedicated Usage");
                p.collect();
                p
            });
        }
        let pdh = self.pdh.take();

        list.push(info("cpu", "CPU", "CPU usage", "%", 100.0, 0));
        // `\Processor(*)`의 인스턴스 이름은 논리 프로세서 번호다(첫 수집 직후에는 PDH가 목록을 주지 않으므로 개수는 OS에서 읽는다)
        let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
        self.cores = (0..cores).map(|i| i.to_string()).collect();
        if pdh.as_ref().is_some_and(|p| p.value("freq.base").is_some_and(|v| v > 0.0)) {
            list.push(info("cpu.freq", "FREQ", "CPU frequency", "GHz", 0.0, 2));
        }

        let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() && mem.ullTotalPhys > 0 {
            list.push(info("mem", "MEM", "Memory", "B", mem.ullTotalPhys as f64, 0));
        }

        self.gpus.clear();
        let mut adapters = adapters();
        adapters.sort_by(|a, b| b.1.cmp(&a.1));
        for (i, (luid, vram, _)) in adapters.into_iter().enumerate() {
            let n = if i == 0 { String::new() } else { (i + 1).to_string() };
            let id = format!("gpu{n}");
            let which = if i == 0 { String::new() } else { format!(" {}", i + 1) };
            let short = |base: &str| if i == 0 { base.to_string() } else { format!("{}{n}", &base[..base.len().min(4)]) };
            list.push(info(&id, &short("GPU"), &format!("GPU{which} usage"), "%", 100.0, 0));
            if vram > 0 {
                list.push(info(&format!("{id}.vram"), &short("VRAM"), &format!("GPU{which} memory"), "B", vram as f64, 0));
            }
            self.gpus.push(Gpu { id, luid });
        }

        if pdh.as_ref().is_some_and(|p| p.counters.contains_key("disk.read")) {
            list.push(info("disk.read", "READ", "Disk read", "B/s", 0.0, 0));
            list.push(info("disk.write", "WRITE", "Disk write", "B/s", 0.0, 0));
        }

        self.drives.clear();
        let mask = unsafe { GetLogicalDrives() };
        for letter in (0..26u8).filter(|i| mask & (1 << i) != 0).map(|i| (b'A' + i) as char) {
            if self.drives.len() >= DRIVES_MAX {
                break;
            }
            let root = format!("{letter}:\\");
            if unsafe { GetDriveTypeW(&HSTRING::from(&root)) } != DRIVE_FIXED {
                continue;
            }
            let Some((_, total)) = disk(&root).filter(|(_, t)| *t > 0.0) else { continue };
            let id = format!("fs.{}", letter.to_ascii_lowercase());
            list.push(info(&id, &format!("{letter}:"), &format!("Storage {root}"), "B", total, 0));
            self.drives.push((id, root));
        }

        if net_bytes().is_some() {
            list.push(info("net.down", "DOWN", "Network download", "B/s", 0.0, 0));
            list.push(info("net.up", "UP", "Network upload", "B/s", 0.0, 0));
        }

        let mut power = SYSTEM_POWER_STATUS::default();
        self.battery = unsafe { GetSystemPowerStatus(&mut power) }.is_ok() && power.BatteryFlag != 128 && power.BatteryLifePercent <= 100;
        if self.battery {
            list.push(info("battery", "BAT", "Battery", "%", 100.0, 0));
        }

        for c in &self.cores {
            list.push(info(&format!("cpu.{c}"), &format!("C{c}"), &format!("CPU core {c}"), "%", 100.0, 0));
        }
        self.pdh = pdh;
        self.list = list.clone();
        list
    }

    pub fn sample<S: AsRef<str>>(&mut self, want: &[S]) -> BTreeMap<String, f64> {
        if self.list.is_empty() {
            self.catalog();
        }
        let want: Vec<&str> = want.iter().map(|s| s.as_ref()).filter(|w| self.list.iter().any(|m| m.id == *w)).collect();
        let mut out = BTreeMap::new();
        let needs_pdh = want.iter().any(|w| w.starts_with("cpu") || w.starts_with("disk.") || w.starts_with("gpu"));
        if needs_pdh {
            if let Some(p) = self.pdh.as_mut() {
                p.collect();
            }
        }
        let pdh = self.pdh.as_ref();

        if want.iter().any(|w| *w == "cpu" || (w.starts_with("cpu.") && *w != "cpu.freq")) {
            let per: HashMap<String, f64> = pdh.map(|p| p.array("cpu", false)).unwrap_or_default().into_iter().filter_map(|(n, v)| Some((n, v?))).collect();
            if want.contains(&"cpu") {
                out.insert("cpu".into(), per.get("_Total").copied().unwrap_or(0.0).clamp(0.0, 100.0));
            }
            for c in &self.cores {
                let id = format!("cpu.{c}");
                if want.contains(&id.as_str()) {
                    out.insert(id, per.get(c).copied().unwrap_or(0.0).clamp(0.0, 100.0));
                }
            }
        }
        if want.contains(&"cpu.freq") {
            if let (Some(base), Some(perf)) = (pdh.and_then(|p| p.value("freq.base")), pdh.and_then(|p| p.value("freq.perf"))) {
                out.insert("cpu.freq".into(), base * perf / 100.0 / 1000.0);
            }
        }
        if want.contains(&"mem") {
            let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() {
                out.insert("mem".into(), (mem.ullTotalPhys - mem.ullAvailPhys) as f64);
            }
        }
        if want.iter().any(|w| w.starts_with("gpu")) {
            let (busy, used) = gpu_usage(pdh);
            for g in &self.gpus {
                if want.contains(&g.id.as_str()) {
                    out.insert(g.id.clone(), busy.get(&g.luid).copied().unwrap_or(0.0).clamp(0.0, 100.0));
                }
                let vram = format!("{}.vram", g.id);
                if want.contains(&vram.as_str()) {
                    out.insert(vram, used.get(&g.luid).copied().unwrap_or(0.0));
                }
            }
        }
        for key in ["disk.read", "disk.write"] {
            if want.contains(&key) {
                out.insert(key.into(), pdh.and_then(|p| p.value(key)).unwrap_or(0.0).max(0.0));
            }
        }
        for (id, root) in &self.drives {
            if want.contains(&id.as_str()) {
                if let Some((used, _)) = disk(root) {
                    out.insert(id.clone(), used);
                }
            }
        }
        if want.contains(&"net.down") || want.contains(&"net.up") {
            let now = net_bytes().map(|(rx, tx)| (rx, tx, Instant::now()));
            let (down, up) = match (self.net, now) {
                (Some((rx0, tx0, t0)), Some((rx1, tx1, t1))) => {
                    let secs = t1.duration_since(t0).as_secs_f64().max(1e-3);
                    (rx1.saturating_sub(rx0) as f64 / secs, tx1.saturating_sub(tx0) as f64 / secs)
                }
                _ => (0.0, 0.0),
            };
            self.net = now;
            if want.contains(&"net.down") {
                out.insert("net.down".into(), down);
            }
            if want.contains(&"net.up") {
                out.insert("net.up".into(), up);
            }
        }
        if self.battery && want.contains(&"battery") {
            let mut power = SYSTEM_POWER_STATUS::default();
            if unsafe { GetSystemPowerStatus(&mut power) }.is_ok() && power.BatteryLifePercent <= 100 {
                out.insert("battery".into(), power.BatteryLifePercent as f64);
            }
        }

        for (id, v) in out.iter_mut() {
            if let Some(m) = self.list.iter().find(|m| m.id == *id) {
                *v = round(m, *v);
            }
        }
        out
    }
}

/// 하드웨어 GPU 어댑터 목록: (LUID 문자열, 전용 메모리 바이트, 이름)
fn adapters() -> Vec<(String, u64, String)> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else { break };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else { continue };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        let luid = format!("luid_0x{:08x}_0x{:08x}", desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart);
        if out.iter().any(|(l, _, _)| *l == luid) {
            continue;
        }
        let name = String::from_utf16_lossy(&desc.Description).trim_end_matches('\0').to_string();
        out.push((luid, desc.DedicatedVideoMemory as u64, name));
    }
    out
}

/// PDH 인스턴스 이름에서 어댑터 LUID 부분(`luid_0x…_0x…`)을 소문자로 꺼낸다
fn luid_of(instance: &str) -> Option<String> {
    let lower = instance.to_ascii_lowercase();
    let start = lower.find("luid_0x")?;
    let rest = &lower[start..];
    let end = rest.match_indices('_').nth(2).map_or(rest.len(), |(i, _)| i);
    Some(rest[..end].to_string())
}

/// 어댑터별 사용률(가장 바쁜 엔진의 프로세스 합)과 전용 메모리 사용량
fn gpu_usage(pdh: Option<&Pdh>) -> (HashMap<String, f64>, HashMap<String, f64>) {
    let Some(p) = pdh else { return Default::default() };
    // 인스턴스 이름: pid_1234_luid_0x00000000_0x0000d1a2_phys_0_eng_0_engtype_3D
    let mut engines: HashMap<(String, String), f64> = HashMap::new();
    for (name, v) in p.array("gpu.engine", false) {
        let (Some(luid), Some(v)) = (luid_of(&name), v) else { continue };
        let lower = name.to_ascii_lowercase();
        let engine = lower.find("_eng_").map(|i| lower[i..].to_string()).unwrap_or_default();
        *engines.entry((luid, engine)).or_insert(0.0) += v;
    }
    let mut busy: HashMap<String, f64> = HashMap::new();
    for ((luid, _), v) in engines {
        let b = busy.entry(luid).or_insert(0.0);
        *b = b.max(v);
    }
    let mut used: HashMap<String, f64> = HashMap::new();
    for (name, v) in p.array("gpu.mem", true) {
        if let (Some(luid), Some(v)) = (luid_of(&name), v) {
            *used.entry(luid).or_insert(0.0) += v;
        }
    }
    (busy, used)
}

/// 드라이브의 (사용량, 전체 용량) 바이트
fn disk(root: &str) -> Option<(f64, f64)> {
    let (mut total, mut free) = (0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(root), None, Some(&mut total), Some(&mut free)) }.ok()?;
    Some(((total - free) as f64, total as f64))
}

/// 하드웨어 네트워크 인터페이스의 누적 (수신, 송신) 바이트
fn net_bytes() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
        return None;
    }
    let (mut rx, mut tx) = (0u64, 0u64);
    unsafe {
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        for row in rows {
            // InterfaceAndOperStatusFlags의 첫 비트가 HardwareInterface(가상 어댑터·루프백·터널 제외)
            if row.InterfaceAndOperStatusFlags._bitfield & 1 != 0 {
                rx += row.InOctets;
                tx += row.OutOctets;
            }
        }
        FreeMibTable(table as *const _);
    }
    Some((rx, tx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_ids_are_read_from_counter_instances() {
        assert_eq!(luid_of("pid_1234_luid_0x00000000_0x0000D1A2_phys_0_eng_0_engtype_3D").as_deref(), Some("luid_0x00000000_0x0000d1a2"));
        assert_eq!(luid_of("luid_0x00000000_0x0000D1A2_phys_0").as_deref(), Some("luid_0x00000000_0x0000d1a2"));
        assert_eq!(luid_of("_Total"), None);
    }
}
