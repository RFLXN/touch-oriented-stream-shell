//! 덱별 신뢰 수준 및 권한 관리.
//!
//! 페어링은 기기 신원만 보장하며, 해당 덱에 어떤 기능을 열어 줄지는 권한(trust) 설정으로 별도 지정한다.
//!   - 권한 목록과 기본값은 아래 PERMISSIONS 테이블에서 관리한다.
//!   - 덱별 선택값은 decks.json의 "perms" 필드에 저장된다(선택하지 않은 항목은 생략하고 기본값을 적용).
//!   - 덱이 요청한 액션은 action_cap에 매핑된 권한이 활성화된 경우에만 실행한다.
//!     테이블에 없는 권한이 필요하거나 매핑되지 않은 알 수 없는 액션은 거부한다.
//!   - 권한 변경은 호스트(`toss-connector allow / deny` 및 웹 콘솔)에서만 가능하며 덱은 이를 변경할 수 없다.
//!   - 실행 중인 toss-connector는 decks.json 변경을 감지해 재시작 없이 곧 반영한다.
//!
//! 새 권한 추가 방법: toss-proto에 CAP_* 상수와 action_cap 매핑을 추가하고 아래 PERMISSIONS 테이블에 등록한다(docs/04-host-link.md 4.12절).
//! 호스트 시스템에 영향을 주는 권한(키 입력, 명령 실행 등)은 반드시 기본값을 false로 둔다.

use std::collections::BTreeMap;

use toss_proto::{action_cap, CAP_COMMAND_RUN, CAP_MEDIA_CONTROL, CAP_MEDIA_INFO, CAP_RESOURCE_INFO, CAP_WEB_OPEN, ERROR_NOT_ALLOWED};

pub struct Permission {
    /// 프로토콜 CAP_* 상수와 동일한 식별자. decks.json과 CLI에서 사용
    pub id: &'static str,
    pub title: &'static str,
    /// 이 권한을 허용했을 때 덱에 부여되는 기능 설명
    pub about: &'static str,
    /// 명시적으로 설정하지 않았을 때의 기본값
    pub default: bool,
}

pub const PERMISSIONS: &[Permission] = &[
    Permission { id: CAP_MEDIA_INFO, title: "재생 정보 공유", about: "재생 중인 곡의 제목·아티스트·앨범·진행 상태를 덱에 표시합니다", default: true },
    Permission { id: CAP_MEDIA_CONTROL, title: "미디어 제어", about: "덱에서 재생/일시정지, 곡 이동, 탐색, 반복·셔플을 제어합니다", default: true },
    Permission { id: CAP_RESOURCE_INFO, title: "리소스 사용량 공유", about: "PC의 CPU·메모리·GPU·디스크·네트워크·온도를 덱에 보냅니다 (위젯에 등록된 항목만 측정)", default: true },
    Permission { id: CAP_COMMAND_RUN, title: "명령 실행", about: "PC에 등록된 명령을 덱에서 실행하고 스위치·노브 값을 조회/변경합니다 (등록된 항목만 가능, 커맨드 원문 비공개)", default: false },
    Permission { id: CAP_WEB_OPEN, title: "웹페이지 열기", about: "덱의 웹페이지 버튼으로 이 PC의 기본 브라우저에서 주소를 엽니다 (http/https 주소만)", default: false },
];

/// 특정 덱에 대한 사용자 권한 설정(권한 ID → 허용 여부). 목록에 없으면 기본값 사용
pub type Choices = BTreeMap<String, bool>;

pub fn find(id: &str) -> Option<&'static Permission> {
    PERMISSIONS.iter().find(|p| p.id == id)
}

/// 해당 권한이 허용되어 있는지 확인. 테이블에 없는 알 수 없는 권한명은 파일에 true로 적혀 있어도 false 반환
pub fn allowed(choices: &Choices, id: &str) -> bool {
    find(id).is_some_and(|p| choices.get(id).copied().unwrap_or(p.default))
}

/// 덱이 요청한 액션 실행 가능 여부를 확인하고, 불가하면 에러 사유를 반환
pub fn check(choices: &Choices, action: &str) -> Result<(), String> {
    match action_cap(action) {
        None => Err(format!("알 수 없는 액션: {action}")),
        Some(cap) if allowed(choices, cap) => Ok(()),
        Some(_) => Err(ERROR_NOT_ALLOWED.to_string()),
    }
}

/// 덱에 전달할 활성 권한 목록: 허용되어 있고 현재 호스트에서 실제 지원 가능한 항목(`can`)만 포함
pub fn caps(choices: &Choices, can: impl Fn(&str) -> bool) -> Vec<String> {
    PERMISSIONS.iter().filter(|p| allowed(choices, p.id) && can(p.id)).map(|p| p.id.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use toss_proto::{ACTION_MEDIA_NEXT, ACTION_MEDIA_PLAY_PAUSE};

    #[test]
    fn unchosen_permissions_follow_their_defaults() {
        let none = Choices::new();
        for p in PERMISSIONS {
            assert_eq!(allowed(&none, p.id), p.default, "{}", p.id);
        }
        assert_eq!(check(&none, ACTION_MEDIA_NEXT), Ok(()));
        assert_eq!(check(&none, toss_proto::ACTION_COMMAND_RUN), Err(ERROR_NOT_ALLOWED.to_string()));
        let on = Choices::from([(CAP_COMMAND_RUN.to_string(), true)]);
        assert_eq!(check(&on, toss_proto::ACTION_COMMAND_RUN), Ok(()));
    }

    #[test]
    fn a_denied_permission_blocks_its_actions_only() {
        let c = Choices::from([(CAP_MEDIA_CONTROL.to_string(), false)]);
        assert_eq!(check(&c, ACTION_MEDIA_PLAY_PAUSE), Err(ERROR_NOT_ALLOWED.to_string()));
        assert!(allowed(&c, CAP_MEDIA_INFO));
        assert_eq!(caps(&c, |_| true), vec![CAP_MEDIA_INFO.to_string(), CAP_RESOURCE_INFO.to_string()]);
        assert!(!caps(&Choices::new(), |_| true).contains(&CAP_COMMAND_RUN.to_string()));
        assert!(caps(&Choices::new(), |_| false).is_empty());
    }

    #[test]
    fn anything_outside_the_table_is_refused() {
        let c = Choices::from([("exec".to_string(), true)]);
        assert!(!allowed(&c, "exec"));
        assert!(check(&c, "exec").is_err());
        assert!(check(&c, "").is_err());
    }

    #[test]
    fn every_permission_is_documented_and_unique() {
        for (i, p) in PERMISSIONS.iter().enumerate() {
            assert!(!p.title.is_empty() && !p.about.is_empty());
            assert!(PERMISSIONS[i + 1..].iter().all(|q| q.id != p.id));
        }
    }
}
