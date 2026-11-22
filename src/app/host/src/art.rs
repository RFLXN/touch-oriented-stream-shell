//! 앨범 아트 처리: 플레이어가 제공한 URL에서 이미지를 가져와 덱 전송에 알맞은 크기로 축소한다.
//!
//! 리눅스 MPRIS 플레이어는 앨범 아트를 로컬 파일(file://)이나 웹 URL(http/https)로 전달한다.
//!   예: Firefox는 file:///home/.../firefox-mpris/....png, Spotify는 https://i.scdn.co/image/...
//! 한 변이 ART_MAX_PX(512)를 넘으면 비율을 유지한 채 축소하고(작은 이미지는 확대하지 않음) JPEG로 재인코딩한다.
//! 결과물은 보통 수십 KB 정도로 청크 2~3개에 전송된다.
//!
//! 네트워크 다운로드가 포함될 수 있으므로 호출 측에서 별도 스레드로 실행한다.

use std::io::{Cursor, Read};
use std::time::Duration;

use toss_proto::ART_MAX_PX;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ImageReader;

/// 다운로드할 원본 이미지 최대 크기
const RAW_MAX: u64 = 16 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);
const JPEG_QUALITY: u8 = 85;

pub struct Art {
    pub id: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// 이미지 ID(URL 해시 기반). 다운로드 전에도 계산 가능하므로 media 메시지에 ID를 먼저 실어 보내고 이미지는 비동기로 후속 전송한다.
pub fn id_of(url: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// URL에서 이미지를 가져와 축소/인코딩한다.
pub fn fetch(url: &str) -> Result<Art, String> {
    let raw = load(url)?;
    Ok(Art { id: id_of(url), mime: "image/jpeg", bytes: shrink(&raw)? })
}

fn load(url: &str) -> Result<Vec<u8>, String> {
    let limited = |r: &mut dyn Read| -> Result<Vec<u8>, String> {
        let mut buf = Vec::new();
        r.take(RAW_MAX + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if buf.len() as u64 > RAW_MAX {
            return Err("이미지 크기가 너무 큽니다".to_string());
        }
        Ok(buf)
    };
    if let Some(path) = url.strip_prefix("file://") {
        let path = path.strip_prefix("localhost").unwrap_or(path);
        // Windows의 file:///C:/... 는 드라이브 문자 앞의 `/`를 떼어야 경로가 된다
        #[cfg(windows)]
        let path = match path.as_bytes() {
            [b'/', d, b':', ..] if d.is_ascii_alphabetic() => &path[1..],
            _ => path,
        };
        let mut f = std::fs::File::open(percent_decode(path)).map_err(|e| e.to_string())?;
        limited(&mut f)
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let agent = ureq::AgentBuilder::new().timeout(FETCH_TIMEOUT).redirects(3).build();
        let resp = agent.get(url).call().map_err(|e| e.to_string())?;
        limited(&mut resp.into_reader())
    } else {
        Err(format!("지원하지 않는 URL 스킴: {}", url.split(':').next().unwrap_or_default()))
    }
}

/// 한 변이 ART_MAX_PX를 넘지 않도록 축소해 JPEG로 변환
pub fn shrink(raw: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = ImageReader::new(Cursor::new(raw)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("이미지 디코딩 실패: {e}"))?;
    let img = if img.width() > ART_MAX_PX || img.height() > ART_MAX_PX {
        img.resize(ART_MAX_PX, ART_MAX_PX, FilterType::Triangle)
    } else {
        img
    };
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(&img.to_rgb8()).map_err(|e| e.to_string())?;
    Ok(out)
}

/// URL 내 %XX 퍼센트 인코딩 디코딩
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi * 16 + lo);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, RgbaImage};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 120, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size_of(jpeg: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory_with_format(jpeg, ImageFormat::Jpeg).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn big_pictures_are_fitted_into_the_limit_keeping_their_shape() {
        assert_eq!(size_of(&shrink(&png(2000, 2000)).unwrap()), (512, 512));
        assert_eq!(size_of(&shrink(&png(1024, 512)).unwrap()), (512, 256));
        assert_eq!(size_of(&shrink(&png(300, 1200)).unwrap()), (128, 512));
    }

    #[test]
    fn small_pictures_are_not_enlarged() {
        assert_eq!(size_of(&shrink(&png(200, 150)).unwrap()), (200, 150));
        assert_eq!(size_of(&shrink(&png(512, 512)).unwrap()), (512, 512));
    }

    #[test]
    fn a_local_file_is_read_through_its_address() {
        let dir = std::env::temp_dir().join(format!("toss-connector-test-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover art.png"), png(800, 800)).unwrap();
        let url = format!("file://{}/cover%20art.png", dir.display());
        let art = fetch(&url).unwrap();
        assert_eq!((art.id.as_str(), art.mime), (id_of(&url).as_str(), "image/jpeg"));
        assert_eq!(size_of(&art.bytes), (512, 512));
        assert!(art.bytes.len() < toss_proto::BLOB_MAX);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_a_picture_or_not_reachable_is_an_error() {
        assert!(shrink(b"not a picture").is_err());
        assert!(fetch("file:///nonexistent/cover.png").is_err());
        assert!(fetch("ftp://example.invalid/cover.png").is_err());
    }

    #[test]
    fn the_same_address_gets_the_same_id() {
        assert_eq!(id_of("https://a/b"), id_of("https://a/b"));
        assert_ne!(id_of("https://a/b"), id_of("https://a/c"));
        assert_eq!(id_of("x").len(), 16);
    }
}
