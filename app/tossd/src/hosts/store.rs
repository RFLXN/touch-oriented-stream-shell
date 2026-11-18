//! 호스트 영구 데이터 저장: 페어링된 호스트 목록(`hosts.json`)과 덱 신원 키(`key`). 둘 다 `data_dir` 아래에 위치한다(기기에서는 영구 파티션 `/data/toss`).

use std::path::Path;

use serde::{Deserialize, Serialize};
use toss_proto::secure::Identity;

use super::{Entry, Hosts};
use crate::persist;

/// 파일에 저장되는 페어링 레코드
#[derive(Serialize, Deserialize)]
struct Saved {
    key: String,
    name: String,
    os: String,
}

/// 페어링된 호스트 목록을 불러온다. 파일이 없으면 빈 목록을, 손상된 파일이면 백업 후(`persist.rs`) 빈 목록을 반환한다.
pub(super) fn load(path: &Path) -> Vec<Entry> {
    let saved: Vec<Saved> = persist::read_json(path, "host", Ok).unwrap_or_default();
    saved.into_iter().map(|s| Entry::new(s.key, s.name, s.os)).collect()
}

/// 덱의 신원 키를 불러온다(없으면 생성). 손상된 키 파일은 백업한 뒤 새로 생성한다 — 이전 페어링은 끊기지만 이후 페어링은 유지된다.
/// 디렉터리에 저장할 수 없는 환경이면 메모리 전용 임시 키를 생성한다(이 경우 재시작할 때마다 키가 바뀌어 페어링이 풀린다).
pub(super) fn load_identity(dir: &Path) -> Identity {
    let path = dir.join("key");
    let loaded = Identity::load_or_create(&path).or_else(|e| {
        if !path.exists() {
            return Err(e);
        }
        persist::set_aside(&path, "host", &e.to_string());
        Identity::load_or_create(&path)
    });
    match loaded {
        Ok(id) => {
            persist::sync(&path);
            id
        }
        Err(e) => {
            eprintln!("host: {}에 덱 키를 저장할 수 없습니다({e}). 이번 실행에만 유효한 임시 키를 사용하므로 페어링이 유지되지 않습니다", dir.display());
            Identity::generate().expect("키 생성 실패")
        }
    }
}

impl Hosts {
    /// 페어링된 호스트 목록을 파일에 저장한다. 파일 쓰기 중 다른 스레드가 블로킹되지 않도록 `inner` 락을 잡지 않은 상태에서 호출한다.
    /// 저장할 스냅샷은 함수 실행 시점 기준으로 캡처되며 `saving` 락으로 쓰기 순서를 직렬화한다.
    pub(super) fn save(&self) {
        let _saving = self.saving.lock().unwrap();
        let list: Vec<Saved> = {
            let inner = self.inner.lock().unwrap();
            inner.entries.iter().map(|e| Saved { key: e.info.id.clone(), name: e.info.name.clone(), os: e.info.os.clone() }).collect()
        };
        if let Err(e) = persist::write_json(&self.store, &list) {
            eprintln!("host: 페어링 목록 저장 실패({}): {e}", self.store.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 전원 차단 등으로 손상된 키 파일은 백업해 두고 새 키를 생성해 디스크에 남긴다(이후 재시작해도 동일 키 유지).
    #[test]
    fn a_broken_key_file_is_set_aside_and_the_new_key_is_kept() {
        let dir = std::env::temp_dir().join(format!("tossd-test-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("key"), "").unwrap();
        let first = load_identity(&dir).id();
        assert!(dir.join("key.bad").exists());
        assert_eq!(load_identity(&dir).id(), first);
        std::fs::write(dir.join("hosts.json"), "[{\"key\":").unwrap();
        assert!(load(&dir.join("hosts.json")).is_empty());
        assert!(dir.join("hosts.json.bad").exists() && !dir.join("hosts.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
