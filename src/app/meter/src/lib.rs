//! 시스템 리소스 사용량 측정: CPU, 메모리, GPU, 디스크, 네트워크, 온도, 팬, 배터리.
//! 덱 데몬(tossd — 덱 자체 리소스)과 PC 호스트(toss-connector — PC 리소스)가 공용으로 사용한다.
//!
//!   let mut s = Sampler::new();
//!   let list = s.catalog();              // 이 기기에서 측정 가능한 항목 목록 (toss_proto::MetricInfo)
//!   let now = s.sample(&["cpu", "mem"]); // 지정한 항목만 측정: id → 값
//!
//! 요청된 항목만 측정한다. 카테고리(CPU, 메모리, GPU 등)별로 필요한 파일만 읽으므로, 구독 항목이 없으면 아무것도 읽지 않는다.
//! 값은 각 항목의 소수점 자릿수(MetricInfo.decimals)에 맞춰 반올림해 반환한다(눈에 보이지 않는 미세 변화로 매번 전송되는 것을 방지).
//! 속도(디스크, 네트워크)와 CPU 사용률은 이전 측정치와의 차이로 계산하므로 첫 호출 시에는 0이 반환된다.
//!
//! 리눅스에서만 동작한다(/proc, /sys 기반). 다른 OS에서는 빈 목록을 반환한다.
//!
//! 측정 항목 (실제 존재하는 항목만 catalog에 포함됨)
//!   cpu, cpu.N       전체 및 코어별 CPU 사용률 (%)               /proc/stat
//!   cpu.freq         평균 CPU 클럭 (GHz)                         cpufreq scaling_cur_freq
//!   cpu.temp         CPU 온도 (°C)                               hwmon(k10temp·coretemp·zenpower), 없으면 thermal_zone
//!   cpu.power        CPU 소비전력 (W)                            powercap(RAPL). 읽기 권한이 있을 때만
//!   load             1분 평균 부하 (load average)                /proc/loadavg
//!   mem, swap        사용 중인 메모리 및 스왑 (B)                /proc/meminfo
//!   gpu[N], gpu[N].vram, .temp, .power, .fan   GPU 사용률, 메모리, 온도, 전력, 팬
//!                    AMD는 /sys/class/drm과 hwmon, NVIDIA는 nvidia-smi(설치된 경우)
//!   disk.read, disk.write   디스크 전체 읽기/쓰기 속도 (B/s)     /proc/diskstats
//!   fs.<이름>        파일시스템별 사용량 (B)                     /proc/mounts와 statvfs
//!   net.down, net.up 네트워크 전체 수신/송신 속도 (B/s)          /proc/net/dev
//!   temp.ssd[N]      SSD 온도 (°C)                               hwmon nvme
//!   fan.N            시스템 팬 (RPM)                             hwmon fan*_input (GPU 팬은 gpu.fan)
//!   battery          배터리 잔량 (%)                             /sys/class/power_supply

use std::collections::BTreeMap;

pub use toss_proto::MetricInfo;

#[cfg(target_os = "linux")]
mod linux;

/// 리소스 샘플러. 속도·사용률 계산을 위해 직전 측정값을 보관한다.
#[derive(Default)]
pub struct Sampler {
    #[cfg(target_os = "linux")]
    inner: linux::State,
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 현재 기기에서 측정 가능한 항목 목록을 반환한다(디스크 마운트 변화 등이 있으므로 주기적으로 재조회).
    pub fn catalog(&mut self) -> Vec<MetricInfo> {
        #[cfg(target_os = "linux")]
        {
            self.inner.catalog()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }
    }

    /// 지정된 항목만 측정한다. catalog에 없거나 현재 읽을 수 없는 항목은 결과에서 빠진다.
    pub fn sample<S: AsRef<str>>(&mut self, want: &[S]) -> BTreeMap<String, f64> {
        #[cfg(target_os = "linux")]
        {
            self.inner.sample(want)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = want;
            BTreeMap::new()
        }
    }
}

/// 항목의 decimals 설정에 맞춰 값을 반올림한다. 바이트 단위는 크기에 맞는 단위(1024의 거듭제곱)의 1/100로 반올림한다.
pub(crate) fn round(info: &MetricInfo, value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    if info.unit == "B" || info.unit == "B/s" {
        let mut unit = 1.0f64;
        while value.abs() >= unit * 1024.0 && unit < 1e15 {
            unit *= 1024.0;
        }
        let step = unit / 100.0;
        return (value / step).round() * step;
    }
    let scale = 10f64.powi(info.decimals as i32);
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(unit: &str, decimals: u8) -> MetricInfo {
        MetricInfo { id: "x".into(), name: "X".into(), title: "x".into(), unit: unit.into(), max: 0.0, decimals }
    }

    #[test]
    fn values_are_rounded_to_what_is_shown() {
        assert_eq!(round(&info("%", 0), 41.6), 42.0);
        assert_eq!(round(&info("GHz", 2), 1.23456), 1.23);
        assert_eq!(round(&info("", 2), f64::NAN), 0.0);
        let gib = 1024.0f64.powi(3);
        assert_eq!(round(&info("B", 0), 12.3456 * gib), 12.35 * gib);
        assert_eq!(round(&info("B/s", 0), 0.0), 0.0);
        assert_eq!(round(&info("B", 0), 8.001 * gib), round(&info("B", 0), 8.004 * gib));
    }

    /// 실제 측정 테스트: 리눅스 환경이라면 CPU와 메모리는 반드시 존재하며, 값은 정상 범위 내에 있어야 한다.
    #[cfg(target_os = "linux")]
    #[test]
    fn this_machine_can_be_measured() {
        let mut s = Sampler::new();
        let list = s.catalog();
        let ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
        for must in ["cpu", "cpu.0", "load", "mem"] {
            assert!(ids.contains(&must), "{must} 누락: {ids:?}");
        }
        let mut seen = std::collections::HashSet::new();
        for m in &list {
            assert!(seen.insert(&m.id), "{} 중복 항목", m.id);
            assert!(!m.name.is_empty() && m.name.chars().count() <= 5, "{}: 짧은 이름 길이 초과 ({})", m.id, m.name);
            assert!(!m.title.is_empty());
        }
        let first = s.sample(&ids);
        assert_eq!(first.get("cpu"), Some(&0.0));
        std::thread::sleep(std::time::Duration::from_millis(120));
        let now = s.sample(&ids);
        let cpu = now["cpu"];
        assert!((0.0..=100.0).contains(&cpu), "cpu {cpu}");
        let mem = list.iter().find(|m| m.id == "mem").unwrap();
        assert!(now["mem"] > 0.0 && now["mem"] <= mem.max, "mem {} / {}", now["mem"], mem.max);
        for m in &list {
            if let Some(v) = now.get(&m.id) {
                assert!(v.is_finite() && *v >= 0.0, "{} = {v}", m.id);
                if m.max > 0.0 && m.unit == "%" {
                    assert!(*v <= m.max, "{} = {v} > {}", m.id, m.max);
                }
            }
        }
        let few = s.sample(&["mem", "nope"]);
        assert_eq!(few.keys().collect::<Vec<_>>(), ["mem"]);
        let asked = s.sample(&["mem", "swap", "cpu.9999", "gpu.temp"]);
        assert!(asked.contains_key("mem") && asked.keys().all(|id| ids.contains(&id.as_str())), "요청 목록에 없는 항목이 결과에 포함됨: {asked:?}");
        assert!(s.sample::<&str>(&[]).is_empty());
    }
}
