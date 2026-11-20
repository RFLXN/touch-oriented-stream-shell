//! 기기 자체 제어: 화면 밝기, 전원 종료, 절전, SSH 활성화/비활성화, 화면 테마. 화면 설정에서 호출한다.
//!
//! 데몬은 권한 없는 사용자로 실행되므로 각 작업은 다음 방식으로 처리한다.
//!   밝기   서비스 시작 시 `src/app/rootfs/etc/init.d/tossd`의 `start_pre`에서 `/sys/class/backlight/*/brightness`의
//!          소유 그룹을 `tossd`로 열어 주며, 데몬이 해당 파일에 직접 값을 쓴다.
//!   전원   root로 실행되는 도우미(`src/app/rootfs/usr/libexec/toss-helper`)의 FIFO(`/run/toss-helper`)에 `"poweroff"`를 쓴다.
//!          도우미가 이를 읽어 일반 종료 절차대로 서비스를 순차 종료한 뒤 전원을 끈다.
//!   절전   같은 FIFO에 `"sleep"`을 쓴다. 도우미가 WoWLAN(Wi-Fi 매직 패킷)을 활성화하고 얕은 절전(`s2idle`, `/sys/power/state`의 `freeze`)으로 진입한다.
//!          절전 상태에서 깨우는 수단이 Wi-Fi 매직 패킷뿐이므로 Wi-Fi에 연결되어 있을 때만 허용한다(`state.rs`에서 판단).
//!   SSH    같은 FIFO에 `"ssh on"` / `"ssh off"`를 쓴다. 도우미가 SSH 서버(`dropbear`)를 시작하거나 중지하고 기존 세션도 끊는다.
//!          비활성화하면 22번 포트를 완전히 닫는다(커널에 방화벽이 없어 LAN만 선택적으로 막을 수 없으므로 USB 쪽도 함께 닫힌다).
//!          유지 여부는 `settings.json`의 `"ssh"` 필드에 저장되며, 부팅 시 `dropbear` 시작 여부도 이 값을 따른다
//!          (`nix/build-rootfs.sh`에서 `/etc/conf.d/dropbear`의 `start_pre`에 추가한 검사 참고). 값이 없으면 기본으로 열어 둔다.
//!          현재 열려 있는지 여부는 설정 파일이 아니라 `dropbear` 프로세스가 실제로 동작 중인지로 판별한다.
//!   테마   UI가 그리는 방식(다크 모드, 유리 효과). 데몬은 값을 저장하고 `device` 상태의 `look`으로 알려 주기만 하며, 실제로 그리는 것은 UI다.
//!
//! 도우미는 FIFO를 열어 둔 채 요청을 순차 처리하므로, 이전 작업이 진행 중일 때 보낸 명령도 누락되지 않고 대기열에서 처리된다.
//!
//! 밝기, SSH, 테마 설정은 기기에 영구 저장(`data_dir/settings.json`)되며, 밝기는 부팅 시 복원된다.
//! 테마는 데몬이 시작할 때 읽어 두고, UI가 접속하면 `device` 상태로 받아 그대로 적용한다.
//! 화면이 완전히 꺼지면 다시 켤 방법이 없으므로 최저 밝기 하한(`BRIGHTNESS_FLOOR`)을 둔다.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::persist;

/// 최저 밝기 비율: 최댓값 대비 비율(최소 1). 이보다 낮게 내려가지 않도록 제한한다.
const BRIGHTNESS_FLOOR: f32 = 0.04;
/// 절전 직후 이 시간 내에 들어온 중복 절전 요청은 무시한다. 도우미가 FIFO 명령을 순차 처리하므로,
/// 연속 입력으로 인해 깨어난 직후 다시 잠드는 현상을 막기 위한 값이다(이 타이머는 절전 중 멈춘다).
const SLEEP_GAP: Duration = Duration::from_secs(5);

/// UI에 전달하는 기기 상태
#[derive(Serialize, Clone, PartialEq, Debug, Default)]
pub struct DeviceState {
    /// 현재 밝기 및 허용 범위(백라이트 원시 값 기준). 밝기 제어가 불가능하면 최댓값이 0이다.
    pub brightness: u32,
    pub brightness_min: u32,
    pub brightness_max: u32,
    /// 전원 종료 및 절전 제어 가능 여부(도우미 FIFO 존재 여부)
    pub can_power: bool,
    /// SSH 서버 동작 여부
    pub ssh: bool,
    /// SSH 활성화/비활성화 제어 가능 여부(도우미 FIFO 및 SSH 서비스 스크립트 존재 여부)
    pub can_ssh: bool,
    /// 저장된 화면 테마
    pub look: Look,
}

/// 화면 테마: UI의 다크 모드와 디스플레이 유리 반사광 효과. 저장된 값이 없으면 라이트 모드에 유리 효과를 켠다.
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub dark: bool,
    pub glass: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self { dark: false, glass: true }
    }
}

/// 영구 저장용 구조체. 지정하지 않은 항목은 직렬화에서 생략한다.
#[derive(Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brightness: Option<u32>,
    /// SSH 활성화 여부. 없으면 기본값 true(열림). **필드명과 형식을 바꾸면 `nix/build-rootfs.sh`의 검사도 함께 수정해야 한다.**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ssh: Option<bool>,
    /// 화면 테마(`Look`). 바꾼 적 없는 항목은 저장하지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dark: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glass: Option<bool>,
}

/// 기본값(`Device::default()`)은 아무 장치도 제어하지 않는다(테스트 환경 또는 백라이트 미지원 환경용).
#[derive(Default)]
pub struct Device {
    /// 백라이트 sysfs 디렉터리(`brightness`, `max_brightness` 포함)
    backlight: Option<PathBuf>,
    /// 도우미(`toss-helper`) FIFO 경로
    helper: Option<PathBuf>,
    /// 설정 저장 파일 경로
    settings: Option<PathBuf>,
    /// SSH 서버가 실행 중일 때 있는 PID 파일(열려 있는지 판별할 때만 사용)과 서비스 스크립트 경로(있어야 제어 가능)
    ssh_pid: Option<PathBuf>,
    ssh_service: Option<PathBuf>,
    /// `settings.json` 읽기/수정 동기화용 뮤텍스
    writing: Mutex<()>,
    /// 마지막으로 절전 진입을 요청한 시각(`SLEEP_GAP` 이내 중복 요청 방지)
    slept: Mutex<Option<Instant>>,
    /// 현재 화면 테마(시작할 때 `settings.json`에서 읽고, 바꿀 때마다 저장한다)
    look: Mutex<Look>,
}

impl Device {
    /// 기기 장치 경로를 초기화한다: 첫 번째 백라이트, 도우미 FIFO, `data_dir/settings.json`
    pub fn find(data_dir: &Path) -> Self {
        let backlight = fs::read_dir("/sys/class/backlight").ok().and_then(|mut d| d.next()?.ok()).map(|e| e.path());
        Self {
            backlight,
            helper: Some("/run/toss-helper".into()),
            settings: Some(data_dir.join("settings.json")),
            ssh_pid: Some("/run/dropbear.pid".into()),
            ssh_service: Some("/etc/init.d/dropbear".into()),
            ..Self::default()
        }
        .load_look()
    }

    #[cfg(test)]
    fn at(backlight: &Path, helper: &Path, settings: &Path) -> Self {
        let dir = settings.parent().unwrap();
        Self {
            backlight: Some(backlight.into()),
            helper: Some(helper.into()),
            settings: Some(settings.into()),
            ssh_pid: Some(dir.join("ssh.pid")),
            ssh_service: Some(dir.join("ssh.service")),
            ..Self::default()
        }
        .load_look()
    }

    /// 저장된 화면 테마를 읽어 현재 값으로 둔다(저장하지 않은 항목은 기본값).
    fn load_look(self) -> Self {
        let saved = self.saved();
        let base = Look::default();
        *self.look.lock().unwrap() = Look { dark: saved.dark.unwrap_or(base.dark), glass: saved.glass.unwrap_or(base.glass) };
        self
    }

    /// 저장된 설정을 읽는다. 파일이 없으면 기본값을 반환하고, 파싱할 수 없으면 백업해 둔 뒤(`persist.rs`) 기본값을 쓴다.
    fn saved(&self) -> Saved {
        self.settings.as_ref().and_then(|p| persist::read_json(p, "device", Ok)).unwrap_or_default()
    }

    /// 저장된 설정 중 일부를 수정해 다시 쓴다(나머지 필드는 유지). 수정 중 다른 스레드와 겹치지 않도록 락을 잡는다.
    fn save(&self, change: impl FnOnce(&mut Saved)) -> Result<(), String> {
        let path = self.settings.as_ref().ok_or("설정 저장 경로가 없습니다")?;
        let _writing = self.writing.lock().unwrap();
        let mut saved = self.saved();
        change(&mut saved);
        persist::write_json(path, &saved).map_err(|e| format!("설정을 저장하지 못했습니다({}): {e}", path.display()))
    }

    /// SSH 서버 실행 여부: PID 파일이 존재하고 해당 PID의 프로세스가 살아 있는지 확인한다.
    fn ssh_running(&self) -> bool {
        let Some(pid) = self.ssh_pid.as_ref().and_then(|p| fs::read_to_string(p).ok()) else { return false };
        let pid = pid.trim();
        !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()) && Path::new("/proc").join(pid).exists()
    }

    fn read(&self, name: &str) -> Option<u32> {
        fs::read_to_string(self.backlight.as_ref()?.join(name)).ok()?.trim().parse().ok()
    }

    pub fn state(&self) -> DeviceState {
        let max = self.read("max_brightness").unwrap_or(0);
        DeviceState {
            brightness: self.read("brightness").unwrap_or(0),
            brightness_min: if max == 0 { 0 } else { ((max as f32 * BRIGHTNESS_FLOOR).round() as u32).max(1) },
            brightness_max: max,
            can_power: self.helper.as_ref().is_some_and(|p| p.exists()),
            ssh: self.ssh_running(),
            can_ssh: self.helper.as_ref().is_some_and(|p| p.exists()) && self.ssh_service.as_ref().is_some_and(|p| p.exists()),
            look: *self.look.lock().unwrap(),
        }
    }

    /// 화면 테마를 바꾸고 저장한다(`None`인 항목은 그대로 둔다). 저장에 실패해도 이번 실행 동안은 바꾼 값을 쓴다.
    pub fn set_look(&self, dark: Option<bool>, glass: Option<bool>) -> Result<(), String> {
        {
            let mut look = self.look.lock().unwrap();
            look.dark = dark.unwrap_or(look.dark);
            look.glass = glass.unwrap_or(look.glass);
        }
        if self.settings.is_none() {
            return Ok(());
        }
        self.save(|s| {
            s.dark = dark.or(s.dark);
            s.glass = glass.or(s.glass);
        })
    }

    /// SSH를 켜거나 끈다. 도우미에 명령이 정상 전달된 경우에만 설정 파일에 저장해(다음 부팅 시 dropbear가 참조)
    /// 현재 상태와 부팅 설정이 어긋나지 않게 한다.
    /// 도우미 작업 완료까지 대기하지는 않으며, 실제 반영 여부는 잠시 뒤 `state().ssh`에 반영된다.
    pub fn set_ssh(&self, open: bool) -> Result<(), String> {
        if !self.state().can_ssh {
            return Err("SSH를 제어할 수 없습니다".into());
        }
        self.tell(if open { "ssh on" } else { "ssh off" })?;
        self.save(|s| s.ssh = Some(open))
    }

    /// 화면 밝기를 변경한다(허용 범위로 클램프). `keep`이 true이면 다음 부팅 시에도 복원되도록 저장한다.
    pub fn set_brightness(&self, value: u32, keep: bool) -> Result<(), String> {
        let st = self.state();
        let dir = self.backlight.as_ref().filter(|_| st.brightness_max > 0).ok_or("밝기를 제어할 수 없습니다")?;
        let value = value.clamp(st.brightness_min, st.brightness_max);
        fs::write(dir.join("brightness"), value.to_string()).map_err(|e| format!("밝기 값을 쓰지 못했습니다: {e}"))?;
        if keep && self.settings.is_some() {
            if let Err(e) = self.save(|s| s.brightness = Some(value)) {
                eprintln!("device: {e}");
            }
        }
        Ok(())
    }

    /// 부팅 시 저장된 밝기 값을 복원한다.
    pub fn restore(&self) {
        if let Some(value) = self.saved().brightness {
            match self.set_brightness(value, false) {
                Ok(()) => eprintln!("device: 저장된 밝기 복원 완료({value})"),
                Err(e) => eprintln!("device: 저장된 밝기 복원 실패: {e}"),
            }
        }
    }

    /// 전원을 끈다.
    pub fn power_off(&self) -> Result<(), String> {
        self.tell("poweroff")
    }

    /// 얕은 절전으로 들어간다(실제 진입까지 약 1초 소요). `SLEEP_GAP` 이내 중복 호출은 무시한다.
    pub fn sleep(&self) -> Result<(), String> {
        let mut slept = self.slept.lock().unwrap();
        if slept.is_some_and(|at| at.elapsed() < SLEEP_GAP) {
            return Err("방금 절전을 요청했습니다".into());
        }
        self.tell("sleep")?;
        *slept = Some(Instant::now());
        Ok(())
    }

    /// 도우미 FIFO에 명령을 전달한다. 도우미가 실행 중이 아니면 블로킹하지 않고 즉시 실패한다.
    /// 도우미는 FIFO를 열어 둔 채 명령을 순차 실행하므로 이전 작업 중 전달한 명령도 대기열에서 처리된다.
    fn tell(&self, word: &str) -> Result<(), String> {
        let path = self.helper.as_ref().ok_or("도우미가 설정되지 않았습니다")?;
        let mut f = fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(path).map_err(|e| format!("도우미에 연결할 수 없습니다({}): {e}", path.display()))?;
        f.write_all(format!("{word}\n").as_bytes()).map_err(|e| format!("도우미에 명령을 쓰지 못했습니다: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(name: &str, max: &str, now: &str) -> (Device, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tossd-test-device-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("backlight")).unwrap();
        fs::write(dir.join("backlight/max_brightness"), max).unwrap();
        fs::write(dir.join("backlight/brightness"), now).unwrap();
        (Device::at(&dir.join("backlight"), &dir.join("helper"), &dir.join("settings.json")), dir)
    }

    #[test]
    fn brightness_stays_in_range_and_never_goes_black() {
        let (dev, dir) = fake("range", "255\n", "200\n");
        assert_eq!(dev.state(), DeviceState { brightness: 200, brightness_min: 10, brightness_max: 255, ..DeviceState::default() });
        dev.set_brightness(120, false).unwrap();
        assert_eq!(dev.state().brightness, 120);
        dev.set_brightness(0, false).unwrap();
        assert_eq!(dev.state().brightness, 10);
        dev.set_brightness(9999, false).unwrap();
        assert_eq!(dev.state().brightness, 255);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kept_brightness_comes_back_after_a_restart() {
        let (dev, dir) = fake("keep", "255", "200");
        dev.set_brightness(90, false).unwrap();
        assert!(!dir.join("settings.json").exists());
        dev.set_brightness(60, true).unwrap();
        fs::write(dir.join("backlight/brightness"), "200").unwrap();
        dev.restore();
        assert_eq!(dev.state().brightness, 60);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_backlight_or_helper_nothing_is_offered() {
        let dev = Device::default();
        assert_eq!(dev.state(), DeviceState::default());
        assert!(dev.set_brightness(100, true).is_err());
        assert!(dev.power_off().is_err());
        let (dev, dir) = fake("nohelper", "255", "200");
        assert!(!dev.state().can_power);
        assert!(dev.power_off().is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// SSH: 활성화 여부는 dropbear 프로세스 실행 여부로 판단한다. 비활성화 시 설정에 기록하고(밝기 설정은 유지) 도우미에 전달한다.
    /// 저장 형식은 `dropbear` 서비스 시작 시 수행하는 검사(`nix/build-rootfs.sh`의 grep)와 일치해야 한다.
    #[test]
    fn ssh_is_closed_through_the_helper_and_the_choice_is_kept() {
        let (dev, dir) = fake("ssh", "255", "200");
        assert_eq!((dev.state().ssh, dev.state().can_ssh), (false, false));
        assert!(dev.set_ssh(false).is_err());
        let fifo = dir.join("helper");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        fs::write(dir.join("ssh.service"), "").unwrap();
        assert!(dev.state().can_ssh);
        fs::write(dir.join("ssh.pid"), format!("{}\n", std::process::id())).unwrap();
        assert!(dev.state().ssh);
        fs::write(dir.join("ssh.pid"), "4194999\n").unwrap();
        assert!(!dev.state().ssh, "종료된 프로세스 PID인데 SSH가 열려 있는 것으로 판정됨");

        dev.set_brightness(60, true).unwrap();
        assert!(dev.set_ssh(false).is_err(), "도우미가 수신 대기 중이 아닌데 성공함");
        assert!(!fs::read_to_string(dir.join("settings.json")).unwrap().contains("ssh"), "전달 실패한 SSH 설정이 저장됨");
        let tell = |open: bool| {
            let fifo = fifo.clone();
            let helper = std::thread::spawn(move || fs::read_to_string(fifo).unwrap());
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while dev.set_ssh(open).is_err() {
                assert!(std::time::Instant::now() < until, "도우미가 FIFO를 열지 않음");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            helper.join().unwrap()
        };
        assert_eq!(tell(false), "ssh off\n");
        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        let closed = |t: &str| t.lines().any(|l| l.trim().trim_end_matches(',').replace(' ', "") == "\"ssh\":false");
        assert!(closed(&text), "{text}");
        assert!(text.contains("\"brightness\": 60"), "기존 밝기 설정이 사라짐: {text}");
        assert_eq!(tell(true), "ssh on\n");
        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(!closed(&text) && text.contains("\"brightness\": 60"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_look_is_kept_across_a_restart_without_touching_other_settings() {
        let (dev, dir) = fake("look", "255", "200");
        assert_eq!(dev.state().look, Look { dark: false, glass: true });
        dev.set_brightness(60, true).unwrap();
        dev.set_look(Some(true), None).unwrap();
        assert_eq!(dev.state().look, Look { dark: true, glass: true });
        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"dark\": true") && !text.contains("glass") && text.contains("\"brightness\": 60"), "{text}");

        let dev = Device::at(&dir.join("backlight"), &dir.join("helper"), &dir.join("settings.json"));
        assert_eq!(dev.state().look, Look { dark: true, glass: true });
        dev.set_look(None, Some(false)).unwrap();
        let dev = Device::at(&dir.join("backlight"), &dir.join("helper"), &dir.join("settings.json"));
        assert_eq!(dev.state().look, Look { dark: true, glass: false });

        assert!(Device::default().set_look(Some(true), None).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn power_off_hands_one_word_to_the_helper() {
        let (dev, dir) = fake("power", "255", "200");
        let fifo = dir.join("helper");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(dev.state().can_power);
        assert!(dev.power_off().is_err());
        let fifo2 = fifo.clone();
        let helper = std::thread::spawn(move || fs::read_to_string(fifo2).unwrap());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while dev.power_off().is_err() {
            assert!(std::time::Instant::now() < until, "도우미가 FIFO를 열지 않음");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(helper.join().unwrap(), "poweroff\n");
        let fifo3 = fifo.clone();
        let helper = std::thread::spawn(move || fs::read_to_string(fifo3).unwrap());
        while dev.sleep().is_err() {
            assert!(std::time::Instant::now() < until, "도우미가 FIFO를 열지 않음");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(helper.join().unwrap(), "sleep\n");
        assert_eq!(dev.sleep(), Err("방금 절전을 요청했습니다".to_string()));
        let _ = fs::remove_dir_all(&dir);
    }
}
