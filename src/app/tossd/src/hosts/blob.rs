//! 청크(`Blob`) 단위로 수신되는 대용량 바이너리 데이터를 조립한다(현재는 앨범 아트 용도). 완성되면 해당 호스트의 `art` 필드가 된다.

use toss_proto::{BLOB_CHUNK, BLOB_MAX};

use super::Art;

/// 수신 중인 청크 버퍼
pub(super) struct Blob {
    id: String,
    mime: String,
    count: u32,
    /// 다음에 수신해야 할 청크 인덱스
    next: u32,
    data: String,
}

/// 단일 청크의 base64 문자열 길이(마지막 청크는 더 짧을 수 있음)
const PIECE_LEN: usize = BLOB_CHUNK / 3 * 4;

/// 청크 하나를 누적한다. 마지막 청크까지 모이면 완성된 `Art`를 반환한다.
/// 순서가 어긋나거나 이미지 형식이 아니거나 허용 크기를 넘으면 버퍼를 비운다(호스트가 처음부터 재전송해야 함).
pub(super) fn take_piece(slot: &mut Option<Blob>, id: String, mime: String, index: u32, count: u32, data: String) -> Option<Art> {
    let last = index.checked_add(1) == Some(count);
    let base64 = |s: &str| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=');
    let sane = matches!(mime.as_str(), "image/jpeg" | "image/png")
        && !id.is_empty()
        && id.len() <= 64
        && count >= 1
        && index < count
        && (count as usize - 1).saturating_mul(BLOB_CHUNK) < BLOB_MAX
        && base64(&data)
        && if last { !data.is_empty() && data.len() <= PIECE_LEN && data.len().is_multiple_of(4) } else { data.len() == PIECE_LEN && !data.contains('=') };
    if !sane {
        *slot = None;
        return None;
    }
    if index == 0 {
        *slot = Some(Blob { id, mime, count, next: 0, data: String::new() });
    } else if !slot.as_ref().is_some_and(|b| b.id == id && b.count == count && b.next == index) {
        *slot = None;
        return None;
    }
    let blob = slot.as_mut()?;
    blob.data.push_str(&data);
    blob.next += 1;
    if !last {
        return None;
    }
    let blob = slot.take()?;
    Some(Art { id: blob.id, mime: blob.mime, data: blob.data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(n: usize) -> String {
        "QUJD".repeat(n / 4)
    }

    #[test]
    fn pieces_in_order_become_one_picture() {
        let mut slot = None;
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 0, 3, piece(PIECE_LEN)), None);
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 1, 3, piece(PIECE_LEN)), None);
        let art = take_piece(&mut slot, "a".into(), "image/jpeg".into(), 2, 3, "QUI=".into()).unwrap();
        assert_eq!((art.id.as_str(), art.mime.as_str(), art.data.len()), ("a", "image/jpeg", 2 * PIECE_LEN + 4));
        assert!(slot.is_none());
        assert!(take_piece(&mut slot, "b".into(), "image/png".into(), 0, 1, "QUJD".into()).is_some());
    }

    #[test]
    fn broken_sequences_and_non_pictures_are_dropped() {
        let mut slot = None;
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 1, 2, "QUJD".into()), None);
        take_piece(&mut slot, "a".into(), "image/jpeg".into(), 0, 3, piece(PIECE_LEN));
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 2, 3, "QUJD".into()), None);
        assert!(slot.is_none());
        take_piece(&mut slot, "a".into(), "image/jpeg".into(), 0, 2, piece(PIECE_LEN));
        assert_eq!(take_piece(&mut slot, "b".into(), "image/jpeg".into(), 1, 2, "QUJD".into()), None);
        assert_eq!(take_piece(&mut slot, "a".into(), "text/html".into(), 0, 1, "QUJD".into()), None);
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 0, 1, "<b>!".into()), None);
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 0, 2, "QUJD".into()), None);
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), 0, 1000, piece(PIECE_LEN)), None);
        assert_eq!(take_piece(&mut slot, "a".into(), "image/jpeg".into(), u32::MAX, u32::MAX, "QUJD".into()), None);
        assert!(slot.is_none());
    }
}
