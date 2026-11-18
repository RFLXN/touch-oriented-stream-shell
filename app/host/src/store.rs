//! 호스트 키 쌍과 페어링된 덱 목록을 사용자 설정 디렉터리에 보관한다.
//!   리눅스  $XDG_CONFIG_HOME/toss-connector 또는 ~/.config/toss-connector
//!   윈도우  %APPDATA%\toss-connector
//!   맥      ~/Library/Application Support/toss-connector
//! 디렉터리 안에는 key(호스트 키 쌍, 비밀키 포함)와 decks.json(페어링된 덱의 공개키, 이름, 주소 목록, 권한)이 저장된다.
//! 디렉터리는 소유자 전용(유닉스에서 0700)으로 생성하며, 기존 디렉터리의 경우 key와 console-token이 외부에 노출되어 있으면 0600으로 교정한다.
//!
//! decks.json은 실행 중인 toss-connector, CLI(`toss-connector allow` 등), 웹 콘솔이 동시에 접근할 수 있다.
//! 따라서 수정 시 항상 최신 내용을 다시 읽어 대상 항목만 변경한 뒤 임시 파일 교체로 원자적으로 저장한다(jsonfile 모듈).
//! 실행 중인 toss-connector는 파일 변경 여부(stamp)를 감시해 자동으로 재로드한다.
//! 읽을 수 없는 파일(수동 편집 오류 등)은 빈 목록으로 취급하되 기존 파일을 덮어쓰지는 않는다.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toss_proto::secure::Identity;

use crate::jsonfile;
use crate::trust::Choices;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Deck {
    /// 덱 공개키 (16진수 ID)
    pub key: String,
    pub name: String,
    /// 접속 가능한 주소 목록(최근 접속 성공 주소가 맨 앞, 그 뒤로 덱이 알려 준 주소. link::merged_addrs로 정리)
    #[serde(default)]
    pub addrs: Vec<String>,
    /// 절전 중인 덱을 깨울 때 매직 패킷에 담을 Wi-Fi MAC 주소(덱이 전달). 미확인 시 빈 문자열
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wake: String,
    /// 사용자가 해당 덱에 직접 설정한 권한(trust 모듈). 생략된 항목은 기본값을 따른다.
    #[serde(default, skip_serializing_if = "Choices::is_empty")]
    pub perms: Choices,
}

impl Deck {
    /// CLI 출력 및 인자 지정용 키 접두사
    pub fn short_key(&self) -> &str {
        self.key.get(..12).unwrap_or(&self.key)
    }
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
    /// 시스템 관리 설정(managed 모듈) 경로. 없으면 None
    managed: Option<PathBuf>,
}

impl Store {
    pub fn open(dir: Option<PathBuf>) -> io::Result<Self> {
        let dir = match dir {
            Some(dir) => dir,
            None => default_dir().ok_or_else(|| io::Error::other("설정 디렉터리를 찾을 수 없습니다 (--config로 경로를 지정하세요)"))?,
        };
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir)?;
        for secret in ["key", "console-token"] {
            keep_private(&dir.join(secret));
        }
        Ok(Self { dir, managed: crate::managed::default_path() })
    }

    /// 시스템 관리 설정 경로를 직접 지정한다(None이면 미사용).
    pub fn with_managed(mut self, path: Option<PathBuf>) -> Self {
        self.managed = path;
        self
    }

    pub fn managed_path(&self) -> Option<PathBuf> {
        self.managed.clone()
    }

    /// 시스템 관리 설정 로드 (없으면 None)
    pub fn managed(&self) -> Option<crate::managed::Managed> {
        crate::managed::Managed::read(self.managed.as_deref())
    }

    pub fn managed_stamp(&self) -> crate::managed::Stamp {
        crate::managed::stamp(self.managed.as_deref())
    }

    /// 덱에 실제 적용되는 최종 권한(사용자 설정 위에 시스템 관리 설정을 덮어씀)
    pub fn effective(&self, perms: &Choices) -> Choices {
        match self.managed() {
            Some(m) => m.apply(perms),
            None => perms.clone(),
        }
    }

    /// 이 설정 디렉터리의 명령 목록 (시스템 관리 설정 포함)
    pub fn commands(&self) -> crate::commands::Commands {
        crate::commands::Commands::with(&self.dir, self.managed.clone())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn identity(&self) -> io::Result<Identity> {
        Identity::load_or_create(&self.dir.join("key"))
    }

    fn decks_path(&self) -> PathBuf {
        self.dir.join("decks.json")
    }

    /// 페어링된 덱 목록 로드. 읽기 실패 시 경고를 1회 출력하고 에러 반환
    pub fn load_decks(&self) -> io::Result<Vec<Deck>> {
        jsonfile::load_or_report(&self.decks_path(), "복구될 때까지 페어링된 덱이 없는 것으로 간주하고 모든 권한을 비활성화합니다").map(Option::unwrap_or_default)
    }

    /// 페어링된 덱 목록 로드 (읽기 실패 시 빈 목록 반환)
    pub fn decks(&self) -> Vec<Deck> {
        self.load_decks().unwrap_or_default()
    }

    /// decks.json 변경 감지용 스탬프 (파일이 없으면 None)
    pub fn stamp(&self) -> jsonfile::Stamp {
        jsonfile::stamp(&self.decks_path())
    }

    /// 특정 덱 항목 수정: 파일을 다시 읽어 대상 항목만 변경한다. 해당 키가 없으면 false
    pub fn update(&self, key: &str, change: impl FnOnce(&mut Deck)) -> io::Result<bool> {
        jsonfile::edit(&self.decks_path(), |list: &mut Vec<Deck>| {
            let found = list.iter_mut().find(|d| d.key == key).map(change).is_some();
            Ok::<_, io::Error>(found)
        })
    }

    /// 덱 정보 저장(동일 키가 있으면 교체)
    pub fn remember(&self, deck: Deck) -> io::Result<()> {
        jsonfile::edit(&self.decks_path(), |list: &mut Vec<Deck>| {
            list.retain(|d| d.key != deck.key);
            list.push(deck);
            Ok::<_, io::Error>(())
        })
    }

    pub fn forget(&self, key: &str) -> io::Result<()> {
        jsonfile::edit(&self.decks_path(), |list: &mut Vec<Deck>| {
            list.retain(|d| d.key != key);
            Ok::<_, io::Error>(())
        })
    }
}

/// 비밀키나 토큰 파일이 외부에 읽기 가능하면 소유자 전용(0600)으로 권한을 제한한다(파일이 없으면 무시. 유닉스에서만 적용).
fn keep_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o077 != 0) {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn default_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var("APPDATA").map(|p| p.join("toss-connector"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|p| p.join("Library/Application Support/toss-connector"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|p| p.join(".config"))).map(|p| p.join("toss-connector"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::open(Some(dir)).unwrap()
    }

    fn deck(key: &str) -> Deck {
        Deck { key: key.into(), name: "deck".into(), addrs: vec!["a".into()], wake: String::new(), perms: Choices::new() }
    }

    #[test]
    fn records_from_before_permissions_still_load() {
        let store = temp_store("old");
        fs::write(store.dir().join("decks.json"), r#"[{"key":"ab","name":"deck","addrs":["172.16.42.1"]}]"#).unwrap();
        let decks = store.decks();
        assert_eq!(decks.len(), 1);
        assert!(decks[0].perms.is_empty());
        let _ = fs::remove_dir_all(store.dir());
    }

    #[test]
    fn update_touches_one_field_and_keeps_the_rest() {
        let store = temp_store("update");
        store.remember(deck("ab")).unwrap();
        assert!(store.update("ab", |d| { d.perms.insert("media.control".into(), false); }).unwrap());
        assert!(store.update("ab", |d| d.addrs = vec!["b".into()]).unwrap());
        let d = &store.decks()[0];
        assert_eq!((d.addrs.as_slice(), d.perms.get("media.control")), (&["b".to_string()][..], Some(&false)));
        assert!(!store.update("zz", |_| {}).unwrap());
        let _ = fs::remove_dir_all(store.dir());
    }

    /// 수동 편집으로 손상된 파일: 페어링된 덱이 없는 것으로 취급하되, 변경 작업을 시도해도 기존 파일을 덮어쓰지 않는다.
    #[test]
    fn a_broken_record_is_never_written_over() {
        let store = temp_store("broken");
        store.remember(deck("ab")).unwrap();
        let broken = r#"[{"key":"ab","name":"deck","addrs":[]},"#;
        fs::write(store.dir().join("decks.json"), broken).unwrap();
        assert!(store.load_decks().is_err() && store.decks().is_empty());
        assert!(store.remember(deck("cd")).is_err());
        assert!(store.forget("ab").is_err());
        assert!(store.update("ab", |d| d.name = "x".into()).is_err());
        assert_eq!(fs::read_to_string(store.dir().join("decks.json")).unwrap(), broken);
        let _ = fs::remove_dir_all(store.dir());
    }

    /// 설정 디렉터리는 소유자 전용(0700)으로 생성하고, 기존 키와 토큰 파일의 과도한 권한은 0600으로 교정한다.
    #[cfg(unix)]
    #[test]
    fn the_folder_and_its_secrets_are_kept_private() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let store = temp_store("private");
        assert_eq!(mode(store.dir()), 0o700);
        store.remember(deck("ab")).unwrap();
        assert_eq!(mode(&store.dir().join("decks.json")), 0o600);
        for name in ["key", "console-token", "other"] {
            fs::write(store.dir().join(name), "x").unwrap();
            fs::set_permissions(store.dir().join(name), fs::Permissions::from_mode(0o644)).unwrap();
        }
        let again = Store::open(Some(store.dir().to_path_buf())).unwrap();
        assert_eq!((mode(&again.dir().join("key")), mode(&again.dir().join("console-token")), mode(&again.dir().join("other"))), (0o600, 0o600, 0o644));
        let _ = fs::remove_dir_all(store.dir());
    }
}
