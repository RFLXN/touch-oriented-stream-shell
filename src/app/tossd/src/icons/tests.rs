use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use super::*;

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
const ICO: &[u8] = &[0, 0, 1, 0, 1, 0, 16, 16];

/// 경로마다 정해 둔 응답을 돌려주고 받은 경로를 기록하는 가짜 웹 서버
fn site(pages: Vec<(&'static str, u16, Vec<u8>)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = head.split_whitespace().nth(1).unwrap_or_default().to_string();
            seen.lock().unwrap().push(path.clone());
            let (status, body) = pages.iter().find(|(p, _, _)| *p == path).map_or((404, b"nope".to_vec()), |(_, st, b)| (*st, b.clone()));
            let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(&body);
        }
    });
    (base, asked)
}

#[test]
fn relative_links_and_attributes_are_read_like_a_browser_would() {
    assert_eq!(join("https://a.example/x/y.html?q=1", "i.png").as_deref(), Some("https://a.example/x/i.png"));
    assert_eq!(join("https://a.example/x/", "/i.png").as_deref(), Some("https://a.example/i.png"));
    assert_eq!(join("https://a.example", "i.png").as_deref(), Some("https://a.example/i.png"));
    assert_eq!(join("https://a.example/x", "//cdn.example/i.png").as_deref(), Some("https://cdn.example/i.png"));
    assert_eq!(join("https://a.example/", "i.png?a=1&amp;b=2").as_deref(), Some("https://a.example/i.png?a=1&b=2"));
    assert_eq!(join("https://a.example/", "#top"), None);
    let a = attrs(r#" rel="apple-touch-icon" sizes=180x180 href='/t.png' data-x />"#);
    assert_eq!((a["rel"].as_str(), a["sizes"].as_str(), a["href"].as_str()), ("apple-touch-icon", "180x180", "/t.png"));
    assert_eq!(largest("16x16 32x32 192x192"), Some(192));
    assert_eq!(largest("any"), None);
}

#[test]
fn the_largest_icon_that_is_really_an_image_wins() {
    let html = r#"<html><head>
        <link rel="icon" href="/small.png" sizes="16x16">
        <LINK REL="apple-touch-icon" href="touch.png">
        <link rel="icon" type="image/svg+xml" href="/v.svg">
        <link rel="manifest" href="/app.webmanifest">
        </head><body><link rel="icon" href="/body.png" sizes="999x999"></body></html>"#;
    let manifest = r#"{"icons":[{"src":"big.png","sizes":"512x512"},{"src":"mono.png","sizes":"1024x1024","purpose":"monochrome"},{"src":"w.webp","sizes":"600x600","type":"image/webp"}]}"#;
    let (base, asked) = site(vec![
        ("/page/", 200, html.as_bytes().to_vec()),
        ("/app.webmanifest", 200, manifest.as_bytes().to_vec()),
        ("/big.png", 200, b"<html>not an image</html>".to_vec()),
        ("/page/touch.png", 200, PNG.to_vec()),
        ("/small.png", 200, PNG.to_vec()),
    ]);
    let (mime, bytes) = fetch(&format!("{base}/page/")).unwrap();
    assert_eq!((mime, bytes.as_slice()), ("image/png", PNG));
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked, ["/page/", "/app.webmanifest", "/big.png", "/page/touch.png"]);
}

#[test]
fn a_page_without_links_falls_back_to_the_well_known_files() {
    let (base, asked) = site(vec![("/", 200, b"<html></html>".to_vec()), ("/favicon.ico", 200, ICO.to_vec())]);
    assert_eq!(fetch(&format!("{base}/")).unwrap().0, "image/x-icon");
    assert_eq!(asked.lock().unwrap().clone(), ["/", "/apple-touch-icon.png", "/apple-touch-icon-precomposed.png", "/favicon.ico"]);
    let inline = format!(r#"<head><link rel="icon" href="data:image/png;base64,{}"></head>"#, base64::engine::general_purpose::STANDARD.encode(PNG));
    let (base, _) = site(vec![("/", 200, inline.into_bytes())]);
    assert_eq!(fetch(&format!("{base}/")).unwrap().1, PNG);
    let (base, _) = site(vec![]);
    assert!(fetch(&format!("{base}/")).is_err());
}

#[test]
fn icons_are_kept_on_disk_and_dropped_when_no_widget_wants_them() {
    let dir = std::env::temp_dir().join(format!("tossd-test-icons-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let (base, _) = site(vec![("/", 200, br#"<head><link rel="icon" href="/i.png"></head>"#.to_vec()), ("/i.png", 200, PNG.to_vec())]);
    let page = format!("{base}/");
    let (seen_tx, seen_rx) = mpsc::channel();
    let tx = spawn(Some(dir.clone()), move |m| {
        let _ = seen_tx.send(m);
    });
    assert!(seen_rx.recv_timeout(Duration::from_secs(5)).unwrap().is_empty());
    tx.send(vec![page.clone()]).unwrap();
    let shown = seen_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(shown[&page], data_uri("image/png", PNG));
    drop(tx);

    let (again_tx, again_rx) = mpsc::channel();
    let tx = spawn(Some(dir.clone()), move |m| {
        let _ = again_tx.send(m);
    });
    assert_eq!(again_rx.recv_timeout(Duration::from_secs(5)).unwrap()[&page], data_uri("image/png", PNG), "저장해 둔 아이콘을 다시 읽지 않음");
    tx.send(Vec::new()).unwrap();
    assert!(again_rx.recv_timeout(Duration::from_secs(5)).unwrap().is_empty());
    thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "빠진 주소의 아이콘 파일이 남음");
    let _ = fs::remove_dir_all(&dir);
}
