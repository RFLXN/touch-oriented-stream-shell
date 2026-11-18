//! 배치 영구 저장소. 기기에서는 영구 파티션의 `/data/toss/layout.json`을 사용한다(사용자 기기 설정이므로 저장소에 포함되지 않음).

use std::path::PathBuf;

use super::{default_layout, Edit, Layout};
use crate::persist;

/// 기본값(`Store::default()`)은 디스크에 저장하지 않는다(테스트용).
#[derive(Default)]
pub struct Store {
    path: Option<PathBuf>,
}

impl Store {
    pub fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// 저장된 배치를 불러온다(빈 페이지는 정리한다). 파일이 없으면 기본 배치를 사용한다.
    /// 파일을 읽지 못하거나 유효성 검사에 실패하면 `.bad`로 백업하고(`persist.rs`) 기본 배치로 시작한다 —
    /// 깨진 배치 때문에 UI가 망가지는 것을 막기 위함이다.
    pub fn load(&self) -> Layout {
        let Some(path) = &self.path else { return default_layout() };
        let tidy = |mut l: Layout| {
            l.edit(&Edit::Tidy)?;
            Ok(l)
        };
        persist::read_json(path, "layout", tidy).unwrap_or_else(default_layout)
    }

    pub fn save(&self, layout: &Layout) {
        let Some(path) = &self.path else { return };
        if let Err(e) = persist::write_json(path, layout) {
            eprintln!("layout: 배치를 저장하지 못했습니다({}): {e}", path.display());
        }
    }
}
