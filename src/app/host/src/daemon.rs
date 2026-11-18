//! toss-connector 데몬 실행 모드: 페어링된 덱에 접속해 세션을 유지하고, 끊기면 재접속한다. 웹 콘솔도 여기서 띄운다.
//!
//! 덱의 알려진 주소(USB, 이더넷, Wi-Fi)를 순회하며 접속하고, 한 번에 하나의 덱에만 연결한다.
//! 페어링된 덱이 없으면 연결을 시도하지 않는다.
//!
//! 동일 설정 디렉터리에서는 단일 인스턴스만 실행해야 한다(두 프로세스가 같은 키로 접속하면
//! 덱이 기존 연결을 끊어 서로 밀어내며 재접속을 반복하게 된다).
//! 이를 막기 위해 설정 디렉터리의 lock 파일을 잠그며, `--config`로 경로를 분리하면 키도 달라지므로 독립 실행이 가능하다.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use toss_proto::STATUS_PAIRED;

use crate::link::{attach, denied_reason, dial_list, explain, merged_addrs, AttachError};
use crate::media::{self, MediaControl};
use crate::session::{can, session, show_caps};
use crate::store::Store;
use crate::{console, fail, trust};

/// 현재 연결된 덱 (키, 접속 주소). 웹 콘솔 표시용.
pub static ATTACHED: Mutex<Option<(String, String)>> = Mutex::new(None);

/// 중복 실행 시 종료 코드. systemd 서비스가 비정상 종료로 오인해 재시작하지 않도록 별도 코드를 사용한다(nix/nixos-module.nix).
const EXIT_ALREADY_RUNNING: i32 = 3;
const RETRY_EVERY: Duration = Duration::from_secs(1);

/// 설정 디렉터리 lock 획득 결과
enum Lock {
    /// 잠금 성공(핸들을 드롭하면 해제됨)
    Held(fs::File),
    /// 이미 다른 인스턴스가 실행 중
    Busy,
    /// 잠금 확인 불가(파일 열기 실패 또는 파일 락 미지원 파일시스템)
    Unknown(String),
}

fn take_lock(dir: &Path) -> Lock {
    let path = dir.join("lock");
    let file = match fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) {
        Ok(f) => f,
        Err(e) => return Lock::Unknown(format!("{}을(를) 열 수 없음: {e}", path.display())),
    };
    match file.try_lock() {
        Ok(()) => Lock::Held(file),
        Err(fs::TryLockError::WouldBlock) => Lock::Busy,
        Err(fs::TryLockError::Error(e)) => Lock::Unknown(format!("{}을(를) 잠글 수 없음: {e}", path.display())),
    }
}

/// 주소별 마지막 출력 로그. 1초마다 재시도할 때 동일한 메시지를 반복 출력하지 않기 위해 사용한다.
/// 해당 주소로 연결에 성공하면 초기화되어 이후 같은 문제가 재발했을 때 다시 출력한다.
#[derive(Default)]
struct Notes(HashMap<String, String>);

impl Notes {
    /// 이번에 새로 출력했으면 true
    fn say(&mut self, addr: &str, what: String) -> bool {
        if self.0.get(addr) == Some(&what) {
            return false;
        }
        eprintln!("{what}");
        self.0.insert(addr.to_string(), what);
        true
    }

    fn forget(&mut self, addr: &str) {
        self.0.remove(addr);
    }
}

pub fn run(store: &Store, with_console: bool) -> ! {
    match take_lock(store.dir()) {
        Lock::Held(file) => std::mem::forget(file),
        Lock::Busy => {
            eprintln!("toss-connector가 이미 실행 중입니다(동일 설정 디렉터리에서는 1개만 실행 가능). 웹 콘솔: toss-connector console");
            std::process::exit(EXIT_ALREADY_RUNNING);
        }
        Lock::Unknown(why) => eprintln!("{why}. 동일 설정 디렉터리의 중복 실행 여부를 확인할 수 없어 그대로 계속합니다"),
    }
    let me = store.identity().unwrap_or_else(|e| fail(&format!("키를 읽거나 생성할 수 없습니다 ({}): {e}", store.dir().display())));
    eprintln!("toss-connector {} — 페어링된 덱에 접속 대기 중", env!("CARGO_PKG_VERSION"));
    if with_console {
        console::spawn(store.dir().to_path_buf(), store.managed_path());
    }
    if store.decks().is_empty() {
        eprintln!("페어링된 덱이 없습니다. 덱 설정에서 페어링 모드를 켠 뒤 웹 콘솔(`toss-connector console`)이나 `toss-connector pair`로 페어링하세요.");
    }
    let mut notes = Notes::default();
    let mut quiet = false;
    let mut media: Option<Box<dyn MediaControl>> = None;
    let mut media_said = false;
    loop {
        if media.is_none() {
            match media::open() {
                Ok(m) => media = Some(m),
                Err(e) if !media_said => {
                    eprintln!("{e}");
                    media_said = true;
                }
                Err(_) => {}
            }
        }
        let decks = store.decks();
        let mut served = false;
        for addr in &dial_list(&decks) {
            let has_media = media.is_some();
            let caps_for = |key: &str| decks.iter().find(|d| d.key == key).map(|d| trust::caps(&store.effective(&d.perms), |cap| can(has_media, cap)));
            let mut a = match attach(addr, &me, false, caps_for) {
                Ok(Some(a)) => a,
                Ok(None) => {
                    notes.say(addr, format!("{addr}의 덱은 페어링된 기기가 아닙니다. 건너뜁니다"));
                    continue;
                }
                Err(AttachError::Unreachable(_)) => continue,
                Err(AttachError::Failed(why)) => {
                    notes.say(addr, format!("{addr} 연결 실패: {why}"));
                    continue;
                }
            };
            let Some(deck) = decks.iter().find(|d| d.key == a.conn.remote) else { continue };
            if a.status != STATUS_PAIRED {
                let reason = denied_reason(&mut a);
                notes.say(addr, format!("덱 {}이(가) 연결을 거부했습니다: {}. `toss-connector pair`로 다시 페어링하세요", deck.name, explain(&reason)));
                continue;
            }
            let known = merged_addrs(addr, &a.addrs, &deck.addrs);
            let wake = if a.wake.is_empty() { deck.wake.clone() } else { a.wake.clone() };
            if known != deck.addrs || a.name != deck.name || wake != deck.wake {
                let name = a.name.clone();
                let saved = store.update(&deck.key, |d| {
                    d.name = name;
                    d.addrs = known;
                    d.wake = wake;
                });
                if let Err(e) = saved {
                    eprintln!("덱 이름과 주소를 저장하지 못함: {e}");
                }
            }
            notes.forget(addr);
            eprintln!("덱 연결됨: {} ({addr}), 허용 권한: {}", a.name, show_caps(&a.caps));
            *ATTACHED.lock().unwrap() = Some((a.conn.remote.clone(), addr.clone()));
            let ended = session(a, &mut media, store);
            *ATTACHED.lock().unwrap() = None;
            eprintln!("덱 연결 끊김: {ended}");
            served = true;
            quiet = false;
            break;
        }
        if !served && !quiet && !decks.is_empty() {
            eprintln!("연결 가능한 덱이 없습니다 (재시도 중...)");
            quiet = true;
        }
        thread::sleep(RETRY_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 동일 설정 디렉터리에서 락이 하나만 유지되며, 단순 경합과 파일/경로 오류를 구분하는지 확인
    #[test]
    fn the_lock_is_held_by_one_and_other_trouble_is_told_apart() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let Lock::Held(first) = take_lock(&dir) else { panic!("초기 락 획득 실패") };
        assert!(matches!(take_lock(&dir), Lock::Busy));
        drop(first);
        assert!(matches!(take_lock(&dir), Lock::Held(_)));
        assert!(matches!(take_lock(&dir.join("없는 폴더")), Lock::Unknown(_)));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 1초마다 재시도해도 동일 주소의 동일 에러는 1회만 출력하고, 내용이 바뀌거나 재연결된 뒤에만 다시 출력한다.
    #[test]
    fn the_same_trouble_at_the_same_address_is_said_once() {
        let mut notes = Notes::default();
        assert!(notes.say("10.0.0.7", "프로토콜 버전이 다릅니다".into()));
        assert!(!notes.say("10.0.0.7", "프로토콜 버전이 다릅니다".into()));
        assert!(notes.say("10.0.0.8", "프로토콜 버전이 다릅니다".into()));
        assert!(notes.say("10.0.0.7", "페어링된 덱이 아닙니다".into()));
        notes.forget("10.0.0.7");
        assert!(notes.say("10.0.0.7", "페어링된 덱이 아닙니다".into()));
    }
}
