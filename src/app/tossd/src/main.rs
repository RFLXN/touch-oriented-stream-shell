//! `tossd` — 토스 프론트 스트림덱 데몬
//!
//! 화면을 그리는 QML 클라이언트와 WebSocket(JSON 텍스트 메시지)으로 통신하고, 호스트(PC)의 `toss-connector` 접속을 처리한다.
//! 로직과 시스템 접근은 모두 데몬에 두고, 화면은 전달받은 상태를 그리는 역할만 맡는다. 권한 없는 사용자로 실행된다.
//!
//! 구성
//!   ui_proto.rs  화면과 주고받는 메시지 프로토콜(상단에 명세 기재)
//!   server.rs    화면 연결 관리: 수신한 요청을 처리하고 변경된 상태 전송
//!   state.rs     모든 화면이 공유하는 상태 및 이를 갱신하는 워커 스레드
//!   layout/      배치 관리: 페이지, 랙, 위젯과 규칙, 카탈로그, 편집, 저장
//!   hosts/       호스트(PC) 연결 수락 및 페어링, 외부 연동 키 보관(금고)(프로토콜은 `src/app/proto`, `toss-proto` 크레이트)
//!   icons.rs     웹페이지 아이콘 버튼이 표시할 사이트 아이콘 가져오기와 보관
//!   wifi.rs      Wi-Fi 스캔 및 연결(`wpa_supplicant` 제어)
//!   device.rs    덱 자체 제어: 화면 밝기, 전원 종료, 절전, SSH(root 권한이 필요한 작업은 `toss-helper`에 위임)
//!   sys.rs       시스템 상태 조회(호스트명, IP 주소, 네트워크 인터페이스)
//!   persist.rs   영구 저장 관리(원자적 쓰기 및 손상된 파일 격리)

mod device;
mod hosts;
mod icons;
mod layout;
mod persist;
mod server;
mod spotify;
mod state;
mod sys;
mod ui_proto;
mod wifi;

use std::net::TcpListener;

const DEFAULT_LISTEN: &str = "127.0.0.1:7878";
/// 배치, 기기 설정, 덱 키 및 페어링 목록 저장 경로. 기기에서는 재플래싱 후에도 유지된다(`/etc/init.d/tf-data`).
const DEFAULT_DATA_DIR: &str = "/data/toss";

fn main() {
    let mut listen = DEFAULT_LISTEN.to_string();
    let mut data_dir = DEFAULT_DATA_DIR.to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--listen" => listen = args.next().expect("--listen 뒤에 주소가 필요합니다"),
            "--data-dir" => data_dir = args.next().expect("--data-dir 뒤에 디렉터리가 필요합니다"),
            "--version" => {
                println!("tossd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("알 수 없는 인자: {other}\n사용법: tossd [--listen 주소:포트] [--data-dir 경로]   (기본 {DEFAULT_LISTEN}, {DEFAULT_DATA_DIR})");
                std::process::exit(2);
            }
        }
    }

    let listener = TcpListener::bind(&listen).unwrap_or_else(|e| {
        eprintln!("{listen}에서 바인딩할 수 없습니다: {e}");
        std::process::exit(1);
    });
    eprintln!("tossd {} — {listen}에서 대기 중", env!("CARGO_PKG_VERSION"));
    let host_listen = format!("0.0.0.0:{}", toss_proto::DEFAULT_PORT).parse().expect("주소");
    if let Err(e) = persist::check_dir(std::path::Path::new(&data_dir)) {
        eprintln!("tossd: {data_dir}에 쓸 수 없습니다({e}). 재부팅 시 배치, 페어링, 덱 키, 설정이 유지되지 않습니다");
    }
    let layout_store = layout::Store::new(std::path::Path::new(&data_dir).join("layout.json"));
    let device = device::Device::find(std::path::Path::new(&data_dir));
    device.restore();
    server::run(listener, layout_store, device, wifi::CTRL_PATH.into(), Some((host_listen, data_dir.into())));
}
