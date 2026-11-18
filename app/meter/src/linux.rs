//! 리눅스 리소스 수집 구현(/proc 및 /sys 기반). 전체 항목 목록은 lib.rs 상단 참고.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use toss_proto::MetricInfo;

/// 표시할 파일시스템 및 시스템 팬 최대 개수
const MOUNTS_MAX: usize = 8;
const FANS_MAX: usize = 6;
/// 외부 명령(nvidia-smi) 응답 대기 타임아웃. 초과 시 프로세스를 종료하고 이번 주기는 건너뛴다.
const TOOL_TIMEOUT: Duration = Duration::from_secs(2);
/// 목록에 포함할 물리 저장장치 파일시스템 종류
const REAL_FS: &[&str] = &["ext4", "ext3", "ext2", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "zfs", "bcachefs"];

enum GpuKind {
    /// AMD: /sys/class/drm/cardN/device 및 하위 hwmon
    Amd { dev: PathBuf, hwmon: Option<PathBuf> },
    /// NVIDIA: nvidia-smi로 조회 (GPU 인덱스)
    Nvidia { index: u32 },
}

struct Gpu {
    /// "gpu", "gpu2", ...
    id: String,
    kind: GpuKind,
}

#[derive(Default)]
pub struct State {
    list: Vec<MetricInfo>,
    /// /proc/stat에서 직전에 읽은 (전체 시간, idle 시간) — 항목 ID별("cpu", "cpu.3")
    cpu_prev: BTreeMap<String, (u64, u64)>,
    disk_prev: Option<(Instant, u64, u64)>,
    net_prev: Option<(Instant, u64, u64)>,
    rapl_prev: Option<(Instant, f64)>,
    cpu_temp: Option<PathBuf>,
    rapl: Option<PathBuf>,
    gpus: Vec<Gpu>,
    /// 별도 GPU 장치는 없지만 GPU 온도 센서가 노출되는 경우(ARM 보드의 thermal_zone)
    gpu_zone: Option<PathBuf>,
    ssds: Vec<PathBuf>,
    fans: Vec<PathBuf>,
    /// (메트릭 ID, 마운트 경로)
    mounts: Vec<(String, String)>,
    battery: Option<PathBuf>,
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn num(path: impl AsRef<Path>) -> Option<f64> {
    read(path)?.parse().ok()
}

/// 디렉터리 내 항목을 자연 정렬(숫자 순서 고려)로 반환
fn entries(dir: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = fs::read_dir(dir).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
    list.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let digits: String = name.chars().rev().take_while(char::is_ascii_digit).collect::<String>().chars().rev().collect();
        (name.trim_end_matches(|c: char| c.is_ascii_digit()).to_string(), digits.parse::<u64>().unwrap_or(0))
    });
    list
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn info(id: &str, name: &str, title: &str, unit: &str, max: f64, decimals: u8) -> MetricInfo {
    MetricInfo { id: id.into(), name: name.into(), title: title.into(), unit: unit.into(), max, decimals }
}

/// hwmon 칩에서 지정된 라벨과 일치하는 온도 입력 파일을 찾는다. 없으면 temp1_input으로 폴백.
fn hwmon_temp(chip: &Path, labels: &[&str]) -> Option<PathBuf> {
    for entry in entries(chip) {
        let name = file_name(&entry);
        if name.starts_with("temp") && name.ends_with("_label") && read(&entry).is_some_and(|l| labels.iter().any(|want| l.eq_ignore_ascii_case(want))) {
            let input = chip.join(name.replace("_label", "_input"));
            if input.exists() {
                return Some(input);
            }
        }
    }
    Some(chip.join("temp1_input")).filter(|p| p.exists())
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// 외부 명령을 실행해 stdout을 읽는다. 제한 시간 내에 끝나지 않으면 프로세스를 종료하고 None을 반환한다. 종료 코드가 실패일 때도 None이다.
/// 출력이 짧은 명령 전용(종료 후 일괄 읽기).
fn output_within(command: &mut Command, limit: Duration) -> Option<String> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let until = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut text = String::new();
                child.stdout.take()?.read_to_string(&mut text).ok()?;
                return status.success().then_some(text);
            }
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// nvidia-smi 질의 결과를 줄별 쉼표 구분 값으로 반환. 타임아웃 시 빈 목록.
fn nvidia(query: &str) -> Vec<Vec<String>> {
    let out = output_within(Command::new("nvidia-smi").arg(format!("--query-gpu={query}")).arg("--format=csv,noheader,nounits"), TOOL_TIMEOUT);
    out.map(|text| text.lines().map(|l| l.split(',').map(|v| v.trim().to_string()).collect()).collect()).unwrap_or_default()
}

/// /proc/stat의 라벨("cpu", "cpu3")을 메트릭 ID("cpu", "cpu.3")로 변환. CPU 행이 아니면 None.
fn cpu_id(label: &str) -> Option<String> {
    let n = label.strip_prefix("cpu")?;
    if n.is_empty() {
        return Some("cpu".to_string());
    }
    n.parse::<u32>().ok().map(|n| format!("cpu.{n}"))
}

/// /proc/stat에서 CPU별 (전체 시간, idle 시간) 파싱.
/// 오프라인 코어가 빠져도 행에 명시된 번호를 그대로 사용하므로 나머지 코어 번호가 밀리지 않는다.
fn cpu_times(stat: &str) -> BTreeMap<String, (u64, u64)> {
    let mut out = BTreeMap::new();
    for line in stat.lines() {
        let mut fields = line.split_whitespace();
        let Some(id) = fields.next().and_then(cpu_id) else { continue };
        let t: Vec<u64> = fields.take(8).filter_map(|v| v.parse().ok()).collect();
        out.insert(id, (t.iter().sum(), t.get(3).copied().unwrap_or(0) + t.get(4).copied().unwrap_or(0)));
    }
    out
}

/// 파일시스템의 (사용 바이트, 전체 바이트) 조회
fn usage(mount: &str) -> Option<(f64, f64)> {
    let path = std::ffi::CString::new(mount).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let unit = st.f_frsize as f64;
    Some(((st.f_blocks.saturating_sub(st.f_bfree)) as f64 * unit, st.f_blocks as f64 * unit))
}

impl State {
    /// 측정 가능한 리소스 목록을 수집해 반환
    pub fn catalog(&mut self) -> Vec<MetricInfo> {
        let mut list = Vec::new();
        let mut cores: Vec<u32> = cpu_times(&fs::read_to_string("/proc/stat").unwrap_or_default()).keys().filter_map(|id| id.strip_prefix("cpu.")?.parse().ok()).collect();
        cores.sort_unstable();

        if !cores.is_empty() {
            list.push(info("cpu", "CPU", "CPU usage", "%", 100.0, 0));
        }
        let chips: Vec<(PathBuf, String)> = entries("/sys/class/hwmon").into_iter().filter_map(|p| read(p.join("name")).map(|n| (p, n))).collect();
        let zones: Vec<(PathBuf, String)> = entries("/sys/class/thermal").into_iter().filter(|p| file_name(p).starts_with("thermal_zone")).filter_map(|p| read(p.join("type")).map(|t| (p, t))).collect();
        self.cpu_temp = [("k10temp", &["Tctl", "Tdie"][..]), ("zenpower", &["Tdie", "Tctl"]), ("coretemp", &["Package id 0"]), ("cpu_thermal", &[])]
            .iter()
            .find_map(|(chip, labels)| chips.iter().find(|(_, n)| n == chip).and_then(|(p, _)| hwmon_temp(p, labels)))
            .or_else(|| zones.iter().find(|(_, t)| t.to_ascii_lowercase().contains("cpu")).or(zones.first()).map(|(p, _)| p.join("temp")));
        if self.cpu_temp.is_some() {
            list.push(info("cpu.temp", "TEMP", "CPU temperature", "°C", 100.0, 0));
        }
        let freq_max = entries("/sys/devices/system/cpu").iter().filter_map(|p| num(p.join("cpufreq/cpuinfo_max_freq"))).fold(0.0, f64::max);
        if freq_max > 0.0 {
            list.push(info("cpu.freq", "FREQ", "CPU frequency", "GHz", (freq_max / 1e6 * 100.0).round() / 100.0, 2));
        }
        self.rapl = Some(PathBuf::from("/sys/class/powercap/intel-rapl:0/energy_uj")).filter(|p| num(p).is_some());
        if self.rapl.is_some() {
            list.push(info("cpu.power", "CPWR", "CPU power", "W", 0.0, 0));
        }
        if !cores.is_empty() {
            list.push(info("load", "LOAD", "Load average (1 min)", "", cores.len() as f64, 2));
        }

        let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let total = meminfo(&mem, "MemTotal");
        if total > 0.0 {
            list.push(info("mem", "MEM", "Memory", "B", total, 0));
        }
        let swap = meminfo(&mem, "SwapTotal");
        if swap > 0.0 {
            list.push(info("swap", "SWAP", "Swap", "B", swap, 0));
        }

        self.gpus.clear();
        for card in entries("/sys/class/drm").into_iter().filter(|p| file_name(p).strip_prefix("card").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))) {
            let dev = card.join("device");
            if dev.join("gpu_busy_percent").exists() {
                let hwmon = entries(dev.join("hwmon")).into_iter().next();
                self.gpus.push(Gpu { id: String::new(), kind: GpuKind::Amd { dev, hwmon } });
            }
        }
        self.gpus.sort_by(|a, b| {
            let vram = |g: &Gpu| match &g.kind {
                GpuKind::Amd { dev, .. } => num(dev.join("mem_info_vram_total")).unwrap_or(0.0),
                GpuKind::Nvidia { .. } => 0.0,
            };
            vram(b).total_cmp(&vram(a))
        });
        let nvidia_gpus = if in_path("nvidia-smi") { nvidia("index,memory.total") } else { Vec::new() };
        for row in &nvidia_gpus {
            if let Some(index) = row.first().and_then(|v| v.parse().ok()) {
                self.gpus.push(Gpu { id: String::new(), kind: GpuKind::Nvidia { index } });
            }
        }
        for (i, gpu) in self.gpus.iter_mut().enumerate() {
            let n = if i == 0 { String::new() } else { (i + 1).to_string() };
            gpu.id = format!("gpu{n}");
            let which = if i == 0 { String::new() } else { format!(" {}", i + 1) };
            let short = |base: &str| if i == 0 { base.to_string() } else { format!("{}{n}", &base[..base.len().min(4)]) };
            list.push(info(&gpu.id, &short("GPU"), &format!("GPU{which} usage"), "%", 100.0, 0));
            match &gpu.kind {
                GpuKind::Amd { dev, hwmon } => {
                    if let Some(total) = num(dev.join("mem_info_vram_total")).filter(|t| *t > 0.0) {
                        list.push(info(&format!("{}.vram", gpu.id), &short("VRAM"), &format!("GPU{which} memory"), "B", total, 0));
                    }
                    if let Some(h) = hwmon {
                        if h.join("temp1_input").exists() {
                            list.push(info(&format!("{}.temp", gpu.id), &short("GTEMP"), &format!("GPU{which} temperature"), "°C", 100.0, 0));
                        }
                        if h.join("power1_average").exists() || h.join("power1_input").exists() {
                            let cap = num(h.join("power1_cap")).map(|c| (c / 1e6).round()).unwrap_or(0.0);
                            list.push(info(&format!("{}.power", gpu.id), &short("GPWR"), &format!("GPU{which} power"), "W", cap, 0));
                        }
                        if h.join("fan1_input").exists() {
                            list.push(info(&format!("{}.fan", gpu.id), &short("GFAN"), &format!("GPU{which} fan"), "RPM", num(h.join("fan1_max")).unwrap_or(0.0), 0));
                        }
                    }
                }
                GpuKind::Nvidia { index } => {
                    let total = nvidia_gpus.iter().find(|r| r.first().and_then(|v| v.parse::<u32>().ok()) == Some(*index)).and_then(|r| r.get(1)?.parse::<f64>().ok()).unwrap_or(0.0);
                    list.push(info(&format!("{}.vram", gpu.id), &short("VRAM"), &format!("GPU{which} memory"), "B", total * 1024.0 * 1024.0, 0));
                    list.push(info(&format!("{}.temp", gpu.id), &short("GTEMP"), &format!("GPU{which} temperature"), "°C", 100.0, 0));
                    list.push(info(&format!("{}.power", gpu.id), &short("GPWR"), &format!("GPU{which} power"), "W", 0.0, 0));
                    list.push(info(&format!("{}.fan", gpu.id), &short("GFAN"), &format!("GPU{which} fan"), "%", 100.0, 0));
                }
            }
        }
        self.gpu_zone = if self.gpus.is_empty() { zones.iter().find(|(_, t)| t.to_ascii_lowercase().contains("gpu")).map(|(p, _)| p.join("temp")) } else { None };
        if self.gpu_zone.is_some() {
            list.push(info("gpu.temp", "GTEMP", "GPU temperature", "°C", 100.0, 0));
        }

        if disk_totals().is_some() {
            list.push(info("disk.read", "READ", "Disk read", "B/s", 0.0, 0));
            list.push(info("disk.write", "WRITE", "Disk write", "B/s", 0.0, 0));
        }
        self.mounts.clear();
        let mut devices = HashSet::new();
        let mut names = HashSet::new();
        for line in fs::read_to_string("/proc/mounts").unwrap_or_default().lines() {
            let mut f = line.split_whitespace();
            let (Some(dev), Some(mount), Some(kind)) = (f.next(), f.next(), f.next()) else { continue };
            if !REAL_FS.contains(&kind) || !devices.insert(dev.to_string()) || self.mounts.len() >= MOUNTS_MAX {
                continue;
            }
            let Some((_, total)) = usage(mount).filter(|(_, t)| *t > 0.0) else { continue };
            let last = mount.rsplit('/').find(|p| !p.is_empty()).unwrap_or("root");
            let slug: String = last.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            let mut slug = if slug.is_empty() { "disk".to_string() } else { slug };
            while !names.insert(slug.clone()) {
                slug.push('x');
            }
            let short: String = slug.to_ascii_uppercase().chars().take(5).collect();
            list.push(info(&format!("fs.{slug}"), &short, &format!("Storage {mount}"), "B", total, 0));
            self.mounts.push((format!("fs.{slug}"), mount.to_string()));
        }

        if net_totals().is_some() {
            list.push(info("net.down", "DOWN", "Network download", "B/s", 0.0, 0));
            list.push(info("net.up", "UP", "Network upload", "B/s", 0.0, 0));
        }

        self.ssds = chips.iter().filter(|(_, n)| n == "nvme").filter_map(|(p, _)| hwmon_temp(p, &["Composite"])).collect();
        for i in 0..self.ssds.len() {
            let n = if i == 0 { String::new() } else { (i + 1).to_string() };
            list.push(info(&format!("temp.ssd{n}"), &format!("SSD{n}"), &format!("SSD{} temperature", if i == 0 { String::new() } else { format!(" {}", i + 1) }), "°C", 90.0, 0));
        }
        self.fans = chips
            .iter()
            .filter(|(_, n)| !matches!(n.as_str(), "amdgpu" | "nouveau" | "nvidia" | "radeon"))
            .flat_map(|(p, _)| entries(p).into_iter().filter(|f| file_name(f).starts_with("fan") && file_name(f).ends_with("_input")))
            .filter(|f| num(f).is_some())
            .take(FANS_MAX)
            .collect();
        for (i, fan) in self.fans.iter().enumerate() {
            let label = read(fan.with_file_name(file_name(fan).replace("_input", "_label"))).map(|l| format!(" ({l})")).unwrap_or_default();
            let max = num(fan.with_file_name(file_name(fan).replace("_input", "_max"))).unwrap_or(0.0);
            list.push(info(&format!("fan.{}", i + 1), &format!("FAN{}", i + 1), &format!("Fan {}{label}", i + 1), "RPM", max, 0));
        }

        self.battery = entries("/sys/class/power_supply").into_iter().map(|p| p.join("capacity")).find(|p| num(p).is_some());
        if self.battery.is_some() {
            list.push(info("battery", "BAT", "Battery", "%", 100.0, 0));
        }

        for i in cores {
            list.push(info(&format!("cpu.{i}"), &format!("C{i}"), &format!("CPU core {i}"), "%", 100.0, 0));
        }
        self.list = list.clone();
        list
    }

    pub fn sample<S: AsRef<str>>(&mut self, want: &[S]) -> BTreeMap<String, f64> {
        let mut out = BTreeMap::new();
        if want.is_empty() {
            return out;
        }
        if self.list.is_empty() {
            self.catalog();
        }
        let wants = |id: &str| want.iter().any(|w| w.as_ref() == id);
        let any = |prefix: &str| want.iter().any(|w| w.as_ref().starts_with(prefix));

        if wants("cpu") || want.iter().any(|w| w.as_ref().strip_prefix("cpu.").is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()))) {
            let now = cpu_times(&fs::read_to_string("/proc/stat").unwrap_or_default());
            for (id, (total, idle)) in &now {
                out.insert(id.clone(), cpu_used(self.cpu_prev.get(id), (*total, *idle)));
            }
            self.cpu_prev = now;
        }
        if wants("cpu.temp") {
            if let Some(t) = self.cpu_temp.as_ref().and_then(num) {
                out.insert("cpu.temp".into(), t / 1000.0);
            }
        }
        if wants("cpu.freq") {
            let khz: Vec<f64> = entries("/sys/devices/system/cpu").iter().filter_map(|p| num(p.join("cpufreq/scaling_cur_freq"))).collect();
            if !khz.is_empty() {
                out.insert("cpu.freq".into(), khz.iter().sum::<f64>() / khz.len() as f64 / 1e6);
            }
        }
        if wants("cpu.power") {
            if let Some(uj) = self.rapl.as_ref().and_then(num) {
                let now = Instant::now();
                let watts = match self.rapl_prev {
                    Some((at, before)) if uj >= before => (uj - before) / 1e6 / now.duration_since(at).as_secs_f64().max(0.001),
                    _ => 0.0,
                };
                self.rapl_prev = Some((now, uj));
                out.insert("cpu.power".into(), watts);
            }
        }
        if wants("load") {
            if let Some(l) = read("/proc/loadavg").and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok()) {
                out.insert("load".into(), l);
            }
        }

        if wants("mem") || wants("swap") {
            let mem = fs::read_to_string("/proc/meminfo").unwrap_or_default();
            out.insert("mem".into(), meminfo(&mem, "MemTotal") - meminfo(&mem, "MemAvailable"));
            out.insert("swap".into(), meminfo(&mem, "SwapTotal") - meminfo(&mem, "SwapFree"));
        }

        if any("gpu") {
            let rows = if self.gpus.iter().any(|g| matches!(g.kind, GpuKind::Nvidia { .. })) { nvidia("index,utilization.gpu,memory.used,temperature.gpu,power.draw,fan.speed") } else { Vec::new() };
            for gpu in &self.gpus {
                let key = |what: &str| if what.is_empty() { gpu.id.clone() } else { format!("{}.{what}", gpu.id) };
                let mut put = |what: &str, v: Option<f64>| {
                    if let Some(v) = v {
                        out.insert(key(what), v);
                    }
                };
                match &gpu.kind {
                    GpuKind::Amd { dev, hwmon } => {
                        put("", num(dev.join("gpu_busy_percent")));
                        put("vram", num(dev.join("mem_info_vram_used")));
                        if let Some(h) = hwmon {
                            put("temp", num(h.join("temp1_input")).map(|t| t / 1000.0));
                            put("power", num(h.join("power1_average")).or_else(|| num(h.join("power1_input"))).map(|p| p / 1e6));
                            put("fan", num(h.join("fan1_input")));
                        }
                    }
                    GpuKind::Nvidia { index } => {
                        if let Some(row) = rows.iter().find(|r| r.first().and_then(|v| v.parse::<u32>().ok()) == Some(*index)) {
                            let at = |i: usize| row.get(i).and_then(|v| v.parse::<f64>().ok());
                            put("", at(1));
                            put("vram", at(2).map(|mib| mib * 1024.0 * 1024.0));
                            put("temp", at(3));
                            put("power", at(4));
                            put("fan", at(5));
                        }
                    }
                }
            }
            if let Some(t) = self.gpu_zone.as_ref().and_then(num) {
                out.insert("gpu.temp".into(), t / 1000.0);
            }
        }

        if any("disk.") {
            if let Some((read, written)) = disk_totals() {
                let now = Instant::now();
                let (r, w) = match self.disk_prev {
                    Some((at, r0, w0)) => {
                        let dt = now.duration_since(at).as_secs_f64().max(0.001);
                        (read.saturating_sub(r0) as f64 / dt, written.saturating_sub(w0) as f64 / dt)
                    }
                    None => (0.0, 0.0),
                };
                self.disk_prev = Some((now, read, written));
                out.insert("disk.read".into(), r);
                out.insert("disk.write".into(), w);
            }
        }
        if any("net.") {
            if let Some((rx, tx)) = net_totals() {
                let now = Instant::now();
                let (down, up) = match self.net_prev {
                    Some((at, rx0, tx0)) => {
                        let dt = now.duration_since(at).as_secs_f64().max(0.001);
                        (rx.saturating_sub(rx0) as f64 / dt, tx.saturating_sub(tx0) as f64 / dt)
                    }
                    None => (0.0, 0.0),
                };
                self.net_prev = Some((now, rx, tx));
                out.insert("net.down".into(), down);
                out.insert("net.up".into(), up);
            }
        }
        if any("fs.") {
            for (id, mount) in &self.mounts {
                if wants(id) {
                    if let Some((used, _)) = usage(mount) {
                        out.insert(id.clone(), used);
                    }
                }
            }
        }

        for (i, path) in self.ssds.iter().enumerate() {
            let id = if i == 0 { "temp.ssd".to_string() } else { format!("temp.ssd{}", i + 1) };
            if wants(&id) {
                if let Some(t) = num(path) {
                    out.insert(id, t / 1000.0);
                }
            }
        }
        for (i, path) in self.fans.iter().enumerate() {
            let id = format!("fan.{}", i + 1);
            if wants(&id) {
                if let Some(rpm) = num(path) {
                    out.insert(id, rpm);
                }
            }
        }
        if wants("battery") {
            if let Some(p) = self.battery.as_ref().and_then(num) {
                out.insert("battery".into(), p);
            }
        }

        out.retain(|id, v| match self.list.iter().find(|m| m.id == *id) {
            Some(m) if wants(id) => {
                *v = crate::round(m, *v);
                true
            }
            _ => false,
        });
        out
    }
}

/// 직전 샘플 대비 CPU 사용률(%) 계산. 직전 값이 없으면 0.
fn cpu_used(before: Option<&(u64, u64)>, (total, idle): (u64, u64)) -> f64 {
    match before {
        Some((t0, i0)) if total > *t0 => (100.0 * (1.0 - idle.saturating_sub(*i0) as f64 / (total - t0) as f64)).clamp(0.0, 100.0),
        _ => 0.0,
    }
}

/// /proc/meminfo의 "키:   값 kB" 행을 바이트 단위로 변환
fn meminfo(text: &str, key: &str) -> f64 {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')).and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok()).map(|kb| kb * 1024.0).unwrap_or(0.0)
}

/// 디스크 전체 누적 (읽기 바이트, 쓰기 바이트). 물리 블록 장치만 집계한다(파티션, loop, dm, zram 제외).
fn disk_totals() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/diskstats").ok()?;
    let (mut read, mut written, mut found) = (0u64, 0u64, false);
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        let Some(name) = f.get(2) else { continue };
        if !Path::new("/sys/block").join(name).join("device").exists() {
            continue;
        }
        read += f.get(5).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 512;
        written += f.get(9).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 512;
        found = true;
    }
    found.then_some((read, written))
}

/// 네트워크 전체 누적 (수신 바이트, 송신 바이트). 물리 장치가 연결된 인터페이스만 집계한다(lo 및 가상 인터페이스 제외).
/// 물리 인터페이스가 하나도 없으면 lo를 제외한 모든 인터페이스를 합산한다.
fn net_totals() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/net/dev").ok()?;
    let rows: Vec<(String, u64, u64)> = text
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter_map(|(name, rest)| {
            let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            Some((name.trim().to_string(), *f.first()?, *f.get(8)?))
        })
        .filter(|(name, _, _)| name != "lo")
        .collect();
    let physical: Vec<&(String, u64, u64)> = rows.iter().filter(|(name, _, _)| Path::new("/sys/class/net").join(name).join("device").exists()).collect();
    let used: Vec<&(String, u64, u64)> = if physical.is_empty() { rows.iter().collect() } else { physical };
    (!used.is_empty()).then(|| used.iter().fold((0, 0), |(rx, tx), (_, r, t)| (rx + r, tx + t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 코어 번호는 /proc/stat에 명시된 번호를 그대로 따른다(오프라인 코어 cpu2가 빠져도 cpu3은 cpu.3으로 유지).
    #[test]
    fn cores_keep_their_own_numbers_when_one_is_offline() {
        let before = cpu_times("cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 10 0 10 80 0 0 0 0 0 0\ncpu1 10 0 10 80 0 0 0 0 0 0\ncpu3 0 0 0 100 0 0 0 0 0 0\nintr 1 2 3\ncpufreq 9\n");
        assert_eq!(before.keys().collect::<Vec<_>>(), ["cpu", "cpu.0", "cpu.1", "cpu.3"]);
        assert_eq!(before["cpu.3"], (100, 100));
        let now = cpu_times("cpu  200 0 100 900 0 0 0 0 0 0\ncpu0 10 0 10 180 0 0 0 0 0 0\ncpu3 50 0 50 100 0 0 0 0 0 0\n");
        assert_eq!(cpu_used(before.get("cpu.3"), now["cpu.3"]), 100.0);
        assert_eq!(cpu_used(before.get("cpu.0"), now["cpu.0"]), 0.0);
        assert_eq!(cpu_used(before.get("cpu"), now["cpu"]), 50.0);
        assert_eq!(cpu_used(before.get("cpu.2"), (5, 5)), 0.0);
    }

    /// 응답하지 않는 외부 명령은 제한 시간 후 강제 종료해 측정 스레드가 멈추지 않게 한다.
    #[test]
    fn a_tool_that_hangs_is_given_up_on() {
        assert_eq!(output_within(Command::new("sh").args(["-c", "echo 1, 2"]), Duration::from_secs(5)).as_deref(), Some("1, 2\n"));
        assert_eq!(output_within(Command::new("sh").args(["-c", "exit 3"]), Duration::from_secs(5)), None);
        let started = Instant::now();
        assert_eq!(output_within(Command::new("sh").args(["-c", "sleep 30"]), Duration::from_millis(100)), None);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(output_within(&mut Command::new("/nonexistent/tool"), Duration::from_secs(1)), None);
    }
}
