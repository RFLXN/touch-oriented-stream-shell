//! 설정 디렉터리 내 JSON 파일(decks.json, commands.json, console.json)의 안전한 읽기·쓰기 유틸리티.
//!
//! 실행 중인 toss-connector, CLI 명령(`toss-connector allow` 등), 웹 콘솔이 같은 파일에 동시 접근하므로 다음 원칙을 따른다:
//!   - 읽기(load): 파일 미존재(None)와 파싱/권한 오류(Err)를 구분한다. 손상된 파일을 빈 파일로 오인해 덮어쓰면 데이터가 유실되기 때문.
//!   - 수정(edit): 인접한 락 파일(<이름>.lock)을 잠근 뒤 최신 파일을 읽어 수정하고, 임시 파일에 쓴 다음 rename으로 원자적 교체한다.
//!     파싱 불가능한 파일은 덮어쓰지 않고 에러를 반환해 사용자가 직접 복구하도록 한다.
//!   - 변경 감지(stamp): mtime, 파일 크기, inode를 함께 비교한다. 같은 초/타임스탬프 내에 두 번 수정되어도 rename 시 inode가 바뀌므로 놓치지 않는다.
//!
//! 새로 생성하는 파일은 소유자 전용(유닉스에서 0600) 권한을 부여한다(커맨드에 민감한 값이 포함될 수 있음).

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// 파일 변경 감지용 스탬프 (mtime, 파일 크기, inode). 파일이 없으면 None
pub type Stamp = Option<(Option<SystemTime>, u64, u64)>;

pub fn stamp(path: &Path) -> Stamp {
    let meta = fs::metadata(path).ok()?;
    #[cfg(unix)]
    let number = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let number = 0;
    Some((meta.modified().ok(), meta.len(), number))
}

/// JSON 파일을 읽는다. 파일이 없거나 공백뿐이면 Ok(None), 권한 부족이나 파싱 실패 시 Err 반환
pub fn load<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
}

/// 파일을 읽고, 오류 시 1회 경고를 출력한 뒤 에러를 반환한다. meanwhile에는 에러 시의 폴백 동작 안내를 전달한다(읽기 전용 경로에서 사용).
pub fn load_or_report<T: DeserializeOwned>(path: &Path, meanwhile: &str) -> io::Result<Option<T>> {
    let got = load(path);
    complain(path, got.as_ref().err().map(|e| format!("{}을(를) 읽을 수 없음: {e}. {meanwhile}", path.display())).as_deref());
    got
}

/// 파일 오류 메시지를 중복 없이 1회만 출력한다. why가 None(정상 복구)이면 이전 기록을 지워 재발 시 다시 출력되게 한다.
/// 이번에 새로 출력했으면 true
pub fn complain(path: &Path, why: Option<&str>) -> bool {
    static SAID: Mutex<BTreeMap<PathBuf, String>> = Mutex::new(BTreeMap::new());
    let mut said = SAID.lock().unwrap();
    match why {
        None => {
            said.remove(path);
            false
        }
        Some(why) if said.get(path).is_some_and(|s| s == why) => false,
        Some(why) => {
            eprintln!("{why}");
            said.insert(path.to_path_buf(), why.to_string());
            true
        }
    }
}

/// 파일 락을 잡고 최신 내용을 읽어 change를 적용한 뒤, 변경이 있으면 원자적으로 저장한다.
/// 파일이 없으면 T::default()에서 시작하고, 파싱에 실패한 파일은 수정하지 않고 에러를 반환한다.
/// change가 에러를 반환하면 저장하지 않는다.
pub fn edit<T, R, E>(path: &Path, change: impl FnOnce(&mut T) -> Result<R, E>) -> Result<R, E>
where
    T: DeserializeOwned + Serialize + Default + PartialEq + Clone,
    E: From<io::Error>,
{
    let _held = lock(path);
    let mut value: T = load(path)
        .map_err(|e| io::Error::new(e.kind(), format!("{}의 형식이 손상되어 수정하지 않았습니다 ({e}). 파일을 복구하거나 삭제한 뒤 다시 시도하세요", path.display())))?
        .unwrap_or_default();
    let before = value.clone();
    let out = change(&mut value)?;
    if value != before {
        save(path, &value)?;
    }
    Ok(out)
}

/// 인접 락 파일을 잠근다(반환값 드롭 시 해제). 잠금 실패 시 None을 반환하지만 수정 작업은 계속 진행한다.
fn lock(path: &Path) -> Option<fs::File> {
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(beside(path, ".lock")).ok()?;
    file.lock().ok()?;
    Some(file)
}

/// 같은 디렉터리에서 파일명 뒤에 suffix를 붙인 경로 반환
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let tmp = beside(path, &format!(".{}-{}.new", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let written = opts.open(&tmp).and_then(|mut f| f.write_all(&bytes)).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &Path, f: impl FnOnce(&mut Vec<u32>)) -> io::Result<()> {
        edit(path, |list: &mut Vec<u32>| {
            f(list);
            Ok(())
        })
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-json-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_is_empty_but_a_broken_one_is_an_error_and_is_never_replaced() {
        let dir = temp("broken");
        let path = dir.join("list.json");
        assert_eq!(load::<Vec<u32>>(&path).unwrap(), None);
        assert_eq!(change(&path, |l| l.push(1)).ok(), Some(()));
        assert_eq!(load::<Vec<u32>>(&path).unwrap(), Some(vec![1]));

        fs::write(&path, "[1, 2").unwrap();
        assert!(load::<Vec<u32>>(&path).is_err());
        let refused = change(&path, |l| l.push(3)).unwrap_err();
        assert!(refused.to_string().contains("수정하지 않았습니다"), "{refused}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "[1, 2");

        fs::write(&path, "  \n").unwrap();
        assert_eq!(load::<Vec<u32>>(&path).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 동일 타임스탬프 내 연속 수정도 inode 변경으로 감지하며, 변경이 없으면 파일을 다시 쓰지 않는다.
    #[test]
    fn every_write_changes_the_stamp_and_an_unchanged_value_is_not_written() {
        let dir = temp("stamp");
        let path = dir.join("list.json");
        assert_eq!(stamp(&path), None);
        let mut seen = vec![stamp(&path)];
        for n in 0..20u32 {
            change(&path, |l| *l = vec![n % 2]).unwrap();
            let now = stamp(&path);
            assert_ne!(Some(&now), seen.last(), "{n}번째 수정이 감지되지 않음");
            seen.push(now);
        }
        let before = stamp(&path);
        change(&path, |l| *l = vec![1]).unwrap();
        assert_eq!(stamp(&path), before);
        let left: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.ends_with(".new")).collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 다중 스레드에서 동시에 수정해도 변경 사항이 유실되지 않는지 확인
    #[test]
    fn edits_from_many_threads_are_all_kept() {
        let dir = temp("race");
        let path = dir.join("list.json");
        let threads: Vec<_> = (0..8u32)
            .map(|t| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        change(&path, |l| l.push(t * 100 + i)).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(load::<Vec<u32>>(&path).unwrap().unwrap().len(), 200);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_complaint_is_made_once_until_the_file_was_fine_again() {
        let path = temp("complain").join("x.json");
        assert!(complain(&path, Some("깨졌다")));
        assert!(!complain(&path, Some("깨졌다")));
        assert!(complain(&path, Some("다르게 깨졌다")));
        assert!(!complain(&path, None));
        assert!(complain(&path, Some("깨졌다")));
    }
}
