//! 영구 데이터 저장: 영구 파티션(`data_dir`)의 파일이 손상되지 않게 원자적으로 쓰고, 읽을 수 없는 파일은 따로 백업해 둔다.
//! 배치(`layout.json`), 페어링 목록(`hosts.json`), 덱 설정(`settings.json`), 덱 키(`key`), 금고(`vault.json`, `vault.key`)가 이 모듈을 사용한다.
//!
//! 덱은 전원을 바로 차단할 수 있는 기기이므로, 임시 파일(`.new`)에 먼저 쓰고 `fsync`한 뒤 이름을 바꾼다.
//! 쓰기 도중 전원이 꺼져도 이전 내용이나 새 내용 중 하나는 온전히 남도록 하기 위함이다.
//! 손상되거나 파싱할 수 없는 파일을 만나면 조용히 덮어쓰지 않고 `.bad` 접미사를 붙여 옮겨 둔 뒤 로그를 남긴다(직접 확인할 수 있도록).

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// 파일 이름 뒤에 `.<suffix>`를 붙인 경로(`layout.json` -> `layout.json.new`)
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// 파일과 상위 디렉터리를 디스크에 동기화한다(기존 파일을 확실히 남길 때 사용, 예: 덱 키).
pub fn sync(path: &Path) {
    if let Ok(f) = File::open(path) {
        let _ = f.sync_all();
    }
    if let Some(dir) = path.parent().and_then(|d| File::open(d).ok()) {
        let _ = dir.sync_all();
    }
}

/// 파일을 원자적으로 교체한다: 임시 파일(`.new`)에 쓰고 `sync_all`로 플러시한 뒤 `rename`한다.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = with_suffix(path, "new");
    let mut f = File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent().and_then(|d| File::open(d).ok()) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// `write_atomic`과 같되 파일을 소유자만 읽고 쓸 수 있게(0600) 만든다. 금고(`vault.json`, `vault.key`)처럼 비밀 값이 든 파일에 쓴다.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = with_suffix(path, "new");
    let _ = fs::remove_file(&tmp);
    let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent().and_then(|d| File::open(d).ok()) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// 값을 JSON으로 저장한다(`write_atomic` 사용).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    write_atomic(path, &serde_json::to_vec_pretty(value).map_err(io::Error::other)?)
}

/// 읽을 수 없는 파일을 `.bad`로 옮겨 두고 원인을 기록한다. `what`은 로그 접두사다(`layout`, `host`, `device`).
pub fn set_aside(path: &Path, what: &str, why: &str) {
    let aside = with_suffix(path, "bad");
    match fs::rename(path, &aside) {
        Ok(()) => eprintln!("{what}: {} 파일을 읽을 수 없습니다({why}). {}(으)로 백업합니다", path.display(), aside.display()),
        Err(e) => eprintln!("{what}: {} 파일을 읽을 수 없습니다({why}). 백업에도 실패했습니다: {e}", path.display()),
    }
}

/// JSON 파일을 읽는다. 파일이 없으면 `None`을 반환한다(아직 저장된 적이 없는 경우).
/// 읽지 못하거나 파싱할 수 없거나 `accept` 검증을 통과하지 못하면 파일을 백업(`set_aside`)하고 `None`을 반환한다.
pub fn read_json<T: DeserializeOwned>(path: &Path, what: &str, accept: impl FnOnce(T) -> Result<T, String>) -> Option<T> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            set_aside(path, what, &e.to_string());
            return None;
        }
    };
    match serde_json::from_slice::<T>(&bytes).map_err(|e| e.to_string()).and_then(accept) {
        Ok(value) => Some(value),
        Err(why) => {
            set_aside(path, what, &why);
            None
        }
    }
}

/// 디렉터리에 쓸 수 있는지 확인한다: 없으면 생성하고 임시 파일을 썼다가 지운다.
pub fn check_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".tossd-{}", std::process::id()));
    File::create(&probe)?.write_all(b"")?;
    fs::remove_file(&probe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tossd-test-persist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn what_is_written_comes_back_and_leaves_no_temporary_file() {
        let dir = dir("write");
        let path = dir.join("list.json");
        assert_eq!(read_json::<Vec<u32>>(&path, "test", Ok), None);
        write_json(&path, &vec![1u32, 2, 3]).unwrap();
        write_json(&path, &vec![4u32]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&path, "test", Ok), Some(vec![4]));
        assert!(!dir.join("list.json.new").exists() && !dir.join("list.json.bad").exists());
        check_dir(&dir).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_cannot_be_used_is_set_aside_not_overwritten() {
        let dir = dir("bad");
        let path = dir.join("list.json");
        fs::write(&path, "").unwrap();
        assert_eq!(read_json::<Vec<u32>>(&path, "test", Ok), None);
        assert!(dir.join("list.json.bad").exists() && !path.exists());
        write_json(&path, &vec![7u32]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&path, "test", |_| Err("검증 실패".into())), None);
        assert_eq!(fs::read_to_string(dir.join("list.json.bad")).unwrap().split_whitespace().collect::<String>(), "[7]");
        assert!(check_dir(&dir.join("list.json.bad").join("under-a-file")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
