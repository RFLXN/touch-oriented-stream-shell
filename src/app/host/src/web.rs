//! 웹페이지 열기: 덱의 웹페이지 버튼(`web.open`)이 보낸 주소를 이 PC의 기본 브라우저로 연다.
//!
//! http:// 또는 https:// 주소만 연다. 다른 스킴(file:, 사용자 정의 스킴 등)은 그 스킴에 연결된 프로그램을 실행할 수 있으므로 거절한다.
//! 주소는 셸을 거치지 않고 여는 프로그램(Linux는 `xdg-open`, macOS는 `open`)의 인자로만 넘긴다.

use std::process::{Command, Stdio};
use std::thread;

/// 덱이 보낼 수 있는 주소 최대 길이(덱의 `layout::URL_MAX`와 같다)
const URL_MAX: usize = 512;

/// 열어도 되는 주소인지 확인한다
pub fn check(url: &str) -> Result<(), String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).ok_or("http:// 또는 https:// 주소만 열 수 있습니다")?;
    if rest.is_empty() || rest.starts_with('/') || url.len() > URL_MAX || !url.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("주소 형식이 올바르지 않습니다".into());
    }
    Ok(())
}

/// 주소를 기본 브라우저로 연다(여는 프로그램이 끝나기를 기다리지 않는다)
pub fn open(url: &str) -> Result<(), String> {
    check(url)?;
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut child = Command::new(opener).arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("{opener}를 실행할 수 없습니다: {e}"))?;
    thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_web_addresses_are_opened() {
        for good in ["https://example.com", "http://example.com/a?b=c#d", "https://localhost:8080/"] {
            assert!(check(good).is_ok(), "{good}");
        }
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "steam://run/1", "https://", "https:///x", "https://a b", "https://a\nb", "HTTPS://example.com", &format!("https://{}", "a".repeat(URL_MAX))] {
            assert!(check(bad).is_err(), "{bad}");
        }
    }
}
