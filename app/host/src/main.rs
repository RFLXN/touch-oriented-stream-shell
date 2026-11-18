//! toss-connector — 덱과 통신하는 호스트 데몬 및 CLI. 명령어와 옵션은 아래 USAGE 참고(`toss-connector --help`).
//!
//! 인자 없이 실행하면 페어링된 덱에 접속해 데몬으로 동작한다(프로토콜: app/proto, 설계 문서: docs/04-host-link.md):
//!   - 호스트 상태를 이벤트 기반으로 덱에 전송(재생 정보·앨범 아트, 등록된 명령과 상태 값, 리소스 사용량)
//!   - 덱의 요청 실행(미디어 제어, 등록된 명령 실행 및 값 설정; 덱은 등록된 항목 중에서만 선택 가능)
//!
//! 모든 동작은 해당 덱에 부여된 권한 범위 내에서만 수행한다(trust 모듈). 미허용 요청은 거절하고 로그에 기록한다.
//!
//! 로그인한 사용자 세션에서 실행해야 한다(MPRIS 플레이어는 사용자 세션 버스에 있으므로 시스템 서비스에서는 접근 불가).
//! OS별 차이는 media 모듈에서 추상화한다. 현재는 리눅스(MPRIS)만 구현되어 있으며, 타 OS에서는 미디어 기능 없이 연결된다.
//!
//! 모듈 구성:
//!   main      인자 파싱 및 하위 모듈 분기
//!   daemon    toss-connector 데몬 메인 루프: 덱 주소 순회 접속, session 실행, 끊김 시 재연결
//!   session   단일 덱 연결 세션 처리
//!   link      단일 주소 TCP 접속 및 hello 핸드셰이크
//!   pairing   페어링 흐름(CLI와 웹 콘솔 공용)
//!   standby   덱 절전 및 WoWLAN 깨우기
//!   cli       단발성 CLI 서브커맨드 처리
//!   console   웹 콘솔 및 HTTP API 서버
//!   store     설정 디렉터리 관리(키 쌍, 페어링 목록).  jsonfile  안전한 JSON 읽기/쓰기
//!   trust     덱별 권한 테이블 및 검증.  managed  시스템(NixOS) 관리 설정
//!   commands  사용자 명령 등록/실행/감시.  media, art  미디어 제어 및 앨범 아트.  meter  리소스 측정

mod art;
mod cli;
mod commands;
mod console;
mod daemon;
mod jsonfile;
mod link;
mod managed;
mod media;
mod meter;
mod pairing;
mod session;
mod standby;
mod store;
mod trust;

use std::path::PathBuf;

use store::Store;
use toss_proto::USB_DECK_ADDR;

const USAGE: &str = "사용법:
  toss-connector                      페어링된 덱에 접속해 데몬 실행
  toss-connector pair [--deck 주소]    덱과 페어링 (먼저 덱 설정에서 페어링 모드를 켜야 함. 기본 주소: USB 172.16.42.1)
  toss-connector list                 페어링된 덱 목록 출력
  toss-connector forget <키 접두사>     페어링 삭제
  toss-connector trust [덱]           덱별 권한 설정 조회
  toss-connector allow [덱] <권한>...  덱에 권한 허용             예: toss-connector allow media.control
  toss-connector deny  [덱] <권한>...  덱의 권한 해제             예: toss-connector deny all
                                 덱은 키 접두사로 지정 (페어링된 덱이 1개면 생략 가능).
                                 실행 중인 toss-connector에도 재시작 없이 곧 반영됩니다.
  toss-connector command list                     덱 명령 위젯용 명령 목록 출력
  toss-connector command add <이름> -- <명령...>   단발성 실행 명령 등록 (버튼)   예: toss-connector command add Terminal -- foot
  toss-connector command add-switch <이름> --on <명령> --off <명령> [--state <명령>]
                                 켜기/끄기 명령 등록 (스위치 버튼)
  toss-connector command add-stepped <이름> --step <단계[=명령]>... [--set <명령>] [--state <명령>]
                                 단계 선택 명령 등록 (단계 노브). 단계별 커맨드(--step 이름=명령)를 주거나,
                                 이름만(--step 이름) 적고 --set의 {}에 단계 이름이 들어가게 합니다.
  toss-connector command add-linear <이름> --set <명령> [--min 0] [--max 100] [--step 1] [--state <명령>]
                                 범위 값 명령 등록 (연속 노브). --set의 {} 자리에 값이 들어갑니다.
                                 --on, --off, --set, --state 뒤의 커맨드는 따옴표로 묶어 전달하세요.
                                 --state는 현재 상태를 출력하는 커맨드입니다: 스위치는 on/off(또는 종료 코드 0이 켜짐),
                                 단계는 단계 이름, 범위는 숫자. --every 초(기본 2)마다 확인해 변경 시 덱에 알립니다.
                                 생략하면 마지막으로 설정한 값만 기억합니다.
  toss-connector command remove <id 또는 이름>     명령 삭제
  toss-connector command run <id 또는 이름>        로컬 PC에서 테스트 실행
  toss-connector command set <id 또는 이름> <값>   로컬 PC에서 값 적용 테스트 (on/off, 단계 이름, 숫자)
  toss-connector command state <id 또는 이름>      현재 상태 조회 테스트
                                 덱에서 사용하려면 해당 덱에 command.run 권한이 필요합니다 (toss-connector allow command.run).
  toss-connector managed              시스템 관리 설정(NixOS programs.toss-connector.config) 조회
  toss-connector managed check <파일>  시스템 관리 설정 파일 검증 (NixOS 모듈 빌드 시 사용)
  toss-connector console              웹 콘솔을 브라우저로 열기 (실행 중인 데몬이 로컬에서 서빙)
  toss-connector console url|token    콘솔 URL(토큰 포함) 또는 토큰만 출력
  toss-connector console allow|deny <출처>   외부 호스팅 콘솔 페이지의 API 접근 허용/해제
                                 예: toss-connector console allow https://example.github.io (해당 페이지 코드를 신뢰하는 경우에만)
  toss-connector console list         허용된 출처 목록 출력
  toss-connector console port <번호>  콘솔 포트 변경 (기본 7880, 데몬 재시작 필요)
  toss-connector sleep [덱]           덱 절전 전환 (Wi-Fi 연결 시에만 가능. 절전 후 Wi-Fi로만 깨우기 가능)
  toss-connector wake [덱]            절전 중인 덱 깨우기 (Wi-Fi 매직 패킷 전송. 같은 네트워크에 있어야 함)
옵션:
  --config <디렉터리>            키와 설정 파일을 저장할 경로 (기본: 사용자 설정 디렉터리)
  --no-console                   웹 콘솔 비활성화";

fn main() {
    let mut command = String::new();
    let mut deck_addr: Option<String> = None;
    let mut config: Option<PathBuf> = None;
    let mut no_console = false;
    let mut rest: Vec<String> = Vec::new();
    let mut tail: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--" => {
                tail = args.by_ref().collect();
                break;
            }
            "--deck" => deck_addr = Some(args.next().unwrap_or_else(|| usage("--deck 뒤에 주소를 지정해야 합니다"))),
            "--config" => config = Some(args.next().unwrap_or_else(|| usage("--config 뒤에 디렉터리 경로를 지정해야 합니다")).into()),
            other if command == "command" => rest.push(other.to_string()),
            "--no-console" => no_console = true,
            "--version" => {
                println!("toss-connector {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other if other.starts_with('-') => usage(&format!("알 수 없는 옵션: {other}")),
            other if command.is_empty() => command = other.to_string(),
            other => rest.push(other.to_string()),
        }
    }
    if let Some(why) = misplaced(&command, &rest, deck_addr.is_some(), !tail.is_empty(), no_console) {
        usage(&why);
    }

    if command == "managed" && rest.first().map(String::as_str) == Some("check") {
        return cli::managed_check(rest.get(1));
    }
    let store = Store::open(config).unwrap_or_else(|e| fail(&format!("설정 디렉터리를 열 수 없습니다: {e}")));
    match command.as_str() {
        "" => daemon::run(&store, !no_console),
        "pair" => cli::pair(&store, deck_addr.as_deref().unwrap_or(USB_DECK_ADDR)),
        "list" => cli::list(&store),
        "forget" => cli::forget(&store, rest.first()),
        "sleep" => cli::sleep(&store, rest.first()),
        "wake" => cli::wake(&store, rest.first()),
        "trust" => cli::trust(&store, rest.first()),
        "allow" | "deny" => cli::set_trust(&store, &rest, command == "allow"),
        "command" => cli::command(&store.commands(), &rest, &tail),
        "managed" => cli::managed(&store, &rest),
        "console" => console::cli(&store, &rest),
        other => usage(&format!("알 수 없는 명령: {other}")),
    }
}

/// 해당 명령에서 허용하지 않는 옵션이나 여분의 인자가 있는지 검사한다.
/// 남는 인자를 조용히 무시하면 의도와 다르게 동작할 수 있기 때문(`pair 10.0.0.7`이 해당 IP가 아닌 USB 기본 주소로 연결되는 등).
fn misplaced(command: &str, rest: &[String], deck: bool, tail: bool, no_console: bool) -> Option<String> {
    let first = rest.first().map(String::as_str);
    if deck && command != "pair" {
        return Some("--deck은 pair 명령에서만 사용할 수 있습니다".to_string());
    }
    if no_console && !command.is_empty() {
        return Some("--no-console은 데몬 실행 시에만 사용할 수 있습니다".to_string());
    }
    if tail && !(command == "command" && first == Some("add")) {
        return Some("'--' 뒤 커맨드 지정은 command add에서만 사용할 수 있습니다".to_string());
    }
    let most = match (command, first) {
        ("pair" | "list", _) => 0,
        ("forget" | "sleep" | "wake" | "trust", _) => 1,
        ("managed", Some("check")) => 2,
        ("managed", _) => 1,
        ("console", Some("allow" | "deny" | "port")) => 2,
        ("console", _) => 1,
        ("command", None | Some("list")) => 1,
        ("command", Some("add" | "remove" | "run" | "state")) => 2,
        ("command", Some("set")) => 3,
        _ => return None,
    };
    let extra = rest.get(most..).filter(|e| !e.is_empty())?;
    let hint = if command == "pair" { " (덱 주소는 --deck 뒤에 지정하세요)" } else { "" };
    Some(format!("{command}: 허용되지 않는 추가 인자가 있습니다: {}{hint}", extra.join(" ")))
}

fn usage(why: &str) -> ! {
    eprintln!("{why}\n\n{USAGE}");
    std::process::exit(2);
}

fn fail(why: &str) -> ! {
    eprintln!("{why}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::misplaced;

    fn check(line: &str) -> Option<String> {
        let mut words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        let deck = words.iter().position(|w| w == "--deck").map(|i| words.drain(i..i + 2).count()).is_some();
        let no_console = words.iter().position(|w| w == "--no-console").map(|i| words.remove(i)).is_some();
        let tail = words.iter().position(|w| w == "--").map(|i| words.truncate(i)).is_some();
        let command = if words.is_empty() { String::new() } else { words.remove(0) };
        misplaced(&command, &words, deck, tail, no_console)
    }

    /// 여분의 인자나 잘못된 위치의 옵션은 거부하고, 정상적인 호출은 통과시킨다.
    #[test]
    fn leftover_arguments_are_refused() {
        for fine in [
            "",
            "--no-console",
            "pair",
            "pair --deck 10.0.0.7",
            "list",
            "forget 9c0b",
            "sleep",
            "wake 9c0b",
            "trust",
            "allow 9c0b media.control command.run",
            "deny all",
            "managed",
            "managed check file.json",
            "console",
            "console allow https://example.github.io",
            "console port 7881",
            "command",
            "command list",
            "command add Terminal -- foot --title x",
            "command add-switch Mic --on a --off b --state c",
            "command remove Terminal",
            "command set Volume 40",
        ] {
            assert_eq!(check(fine), None, "{fine}");
        }
        for bad in [
            "pair 10.0.0.7",
            "list all",
            "forget 9c0b 1a2b",
            "sleep 9c0b now",
            "trust 9c0b extra",
            "list --deck 10.0.0.7",
            "list --no-console",
            "trust -- foot",
            "command remove Terminal -- foot",
            "managed show extra",
            "managed check a.json b.json",
            "console url extra",
            "console port 7881 7882",
            "command list extra",
            "command remove A B",
            "command set Volume 40 50",
        ] {
            assert!(check(bad).is_some(), "{bad}");
        }
        assert!(check("pair 10.0.0.7").unwrap().contains("--deck"));
    }
}
