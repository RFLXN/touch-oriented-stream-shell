//! 단발성 CLI 서브커맨드 처리: 페어링, 덱 목록/권한 관리, 명령 등록, 절전/깨우기.
//! (인자 없이 실행하는 toss-connector 데몬 모드는 daemon 모듈, `toss-connector console ...`은 console 모듈에 있다.)
//!
//! 실행 결과를 출력하고 실패 시 원인을 출력한 뒤 종료한다(crate::fail: 종료 코드 1, crate::usage: 2).
//! 실제 로직은 다른 모듈에 위임하며 웹 콘솔과 동일한 백엔드(pairing, standby, commands, store)를 공유한다.

use std::io::{self, BufRead, Write};
use std::thread;
use std::time::{Duration, Instant};

use toss_proto::secure::Identity;

use crate::commands::{Command, Commands};
use crate::managed::Managed;
use crate::pairing::{Begin, Pairing, Step};
use crate::store::{Deck, Store};
use crate::{fail, standby, trust, usage};

const NO_DECKS: &str = "페어링된 덱이 없습니다. `toss-connector pair`로 먼저 페어링하세요.";

fn identity(store: &Store) -> Identity {
    store.identity().unwrap_or_else(|e| fail(&format!("키를 읽거나 생성할 수 없습니다 ({}): {e}", store.dir().display())))
}

/// 키 접두사로 덱을 선택한다. 접두사를 생략하면 페어링된 덱이 1개일 때만 자동 선택한다.
fn pick_deck(store: &Store, prefix: Option<&String>) -> Deck {
    let mut hits: Vec<Deck> = store.decks().into_iter().filter(|d| prefix.is_none_or(|p| d.key.starts_with(p.as_str()))).collect();
    match (hits.len(), prefix) {
        (1, _) => hits.remove(0),
        (0, Some(_)) => fail("일치하는 덱이 없습니다 (toss-connector list로 키 확인)"),
        (0, None) => fail(NO_DECKS),
        (_, Some(_)) => fail("여러 덱이 일치합니다. 키 접두사를 더 길게 지정하세요"),
        (_, None) => fail("페어링된 덱이 여러 개입니다. 대상 덱의 키 접두사를 지정하세요 (toss-connector list 참고)"),
    }
}

pub fn pair(store: &Store, addr: &str) {
    println!("덱({addr})에 페어링을 요청하는 중...");
    let mut pairing = match Pairing::begin(store, &identity(store), addr) {
        Ok(Begin::Ready(p)) => p,
        Ok(Begin::Already(name)) => {
            println!("이미 페어링된 덱입니다: {name}");
            return;
        }
        Err(why) => fail(&why),
    };
    println!("\n덱: {}\n\n        {}\n", pairing.name(), pairing.code());
    println!("덱 화면에도 동일한 숫자가 표시되는지 확인하세요. 다르면 보안 위협일 수 있으니 중단해야 합니다.");
    print!("숫자가 일치합니까? [y/N] ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    let _ = io::stdin().lock().read_line(&mut answer);
    pairing.answerer().send(matches!(answer.trim(), "y" | "Y" | "yes"));
    loop {
        match pairing.next() {
            Step::Confirmed => println!("덱에서 ACCEPT를 누를 때까지 기다리는 중..."),
            Step::Paired => break,
            Step::Cancelled => fail("취소되었습니다."),
            Step::Failed(why) => fail(&why),
        }
    }
    println!("페어링 완료: {}. 이제 `toss-connector`를 실행하면 자동으로 연결됩니다.", pairing.name());
    println!("권한 확인은 `toss-connector trust`, 변경은 `toss-connector allow / deny`를 사용하세요.");
}

pub fn list(store: &Store) {
    let decks = store.decks();
    if decks.is_empty() {
        println!("{NO_DECKS}");
    }
    for d in decks {
        println!("{}  {}  ({})", d.short_key(), d.name, d.addrs.join(", "));
    }
}

pub fn forget(store: &Store, prefix: Option<&String>) {
    let prefix = prefix.unwrap_or_else(|| usage("삭제할 덱의 키 접두사를 지정하세요 (toss-connector list 참고)"));
    let d = pick_deck(store, Some(prefix));
    store.forget(&d.key).unwrap_or_else(|e| fail(&format!("설정 파일을 수정할 수 없습니다: {e}")));
    println!("삭제됨: {}. 덱 설정에서도 이 PC를 삭제해야 다시 페어링할 수 있습니다.", d.name);
}

pub fn sleep(store: &Store, prefix: Option<&String>) {
    let deck = pick_deck(store, prefix);
    match standby::sleep_deck(&identity(store), &deck) {
        Ok((name, can_wake)) => {
            println!("{name}이(가) 절전 모드로 전환됩니다. 깨우려면: toss-connector wake");
            if !can_wake {
                println!("(이 덱을 깨울 Wi-Fi MAC 주소를 아직 모릅니다. 원격으로 깨우지 못할 수 있습니다)");
            }
        }
        Err(e) => fail(&e),
    }
}

pub fn wake(store: &Store, prefix: Option<&String>) {
    let deck = pick_deck(store, prefix);
    match standby::wake_deck(&deck, || println!("{}에 매직 패킷을 전송했습니다. 깨어나는지 확인 중...", deck.name)) {
        Ok(addr) => println!("깨어났습니다 ({addr})."),
        Err(e) => fail(&e),
    }
}

pub fn trust(store: &Store, prefix: Option<&String>) {
    let decks = match prefix {
        Some(prefix) => vec![pick_deck(store, Some(prefix))],
        None => store.decks(),
    };
    if decks.is_empty() {
        println!("{NO_DECKS}");
    }
    let managed = store.managed();
    for d in &decks {
        print_trust(managed.as_ref(), d);
    }
}

fn print_trust(managed: Option<&Managed>, d: &Deck) {
    println!("{}  ({})", d.name, d.short_key());
    let now = managed.map_or_else(|| d.perms.clone(), |m| m.apply(&d.perms));
    for p in trust::PERMISSIONS {
        let on = trust::allowed(&now, p.id);
        let how = if managed.is_some_and(|m| m.fixed(p.id).is_some()) {
            "  [Nix로 고정]"
        } else if d.perms.contains_key(p.id) {
            ""
        } else {
            "  (기본값)"
        };
        println!("  {}  {:<14} {} — {}{how}", if on { "켬" } else { "끔" }, p.id, p.title, p.about);
    }
}

pub fn set_trust(store: &Store, rest: &[String], allow: bool) {
    let is_perm = |w: &String| w == "all" || trust::find(w).is_some();
    let (deck, words) = match rest.split_first() {
        Some((first, tail)) if !is_perm(first) && !first.contains('.') => (pick_deck(store, Some(first)), tail),
        _ => (pick_deck(store, None), rest),
    };
    let known = || trust::PERMISSIONS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ");
    if words.is_empty() {
        usage(&format!("변경할 권한을 지정하세요: {}", known()));
    }
    let mut ids: Vec<&str> = Vec::new();
    for w in words {
        match (w.as_str(), trust::find(w)) {
            ("all", _) if allow => fail(&format!("전체 일괄 허용(allow all)은 지원하지 않습니다. 필요한 권한만 개별 지정하세요: {}", known())),
            ("all", _) => ids.extend(trust::PERMISSIONS.iter().map(|p| p.id)),
            (_, Some(p)) => ids.push(p.id),
            _ => fail(&format!("알 수 없는 권한: {w}\n사용 가능: {}", known())),
        }
    }
    let managed = store.managed();
    if let Some(id) = ids.iter().find(|id| managed.as_ref().is_some_and(|m| m.fixed(id).is_some())) {
        fail(&format!("{id}은(는) 시스템 설정(Nix)으로 고정되어 있습니다. Nix 설정에서 변경하세요 (toss-connector managed 참고)"));
    }
    let found = store
        .update(&deck.key, |d| {
            for id in &ids {
                d.perms.insert(id.to_string(), allow);
            }
        })
        .unwrap_or_else(|e| fail(&format!("설정 파일을 수정할 수 없습니다: {e}")));
    if !found {
        fail("그사이 해당 덱의 페어링이 삭제되었습니다");
    }
    if let Some(d) = store.decks().iter().find(|d| d.key == deck.key) {
        print_trust(managed.as_ref(), d);
    }
    println!("실행 중인 toss-connector가 있으면 곧 반영됩니다.");
}

/// `toss-connector managed check <파일>`: 시스템 관리 설정 파일을 검증하고 정규화된 JSON을 출력한다.
/// NixOS 모듈 빌드 중 실행되며, 오류가 있으면 빌드를 실패시킨다. 사용자 설정 디렉터리는 건드리지 않는다.
pub fn managed_check(path: Option<&String>) {
    let path = path.unwrap_or_else(|| usage("검사할 파일 경로를 지정하세요: toss-connector managed check <파일>"));
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("{path}을(를) 읽을 수 없습니다: {e}")));
    let m = Managed::parse(&text).unwrap_or_else(|e| fail(&format!("시스템 설정 형식이 올바르지 않습니다: {e}")));
    println!("{}", serde_json::to_string_pretty(&m).unwrap_or_else(|e| fail(&format!("JSON 직렬화 실패: {e}"))));
}

/// `toss-connector managed`: 시스템 관리 설정(managed 모듈) 출력
pub fn managed(store: &Store, rest: &[String]) {
    match rest.first().map(String::as_str) {
        None | Some("show") => match (store.managed_path(), store.managed()) {
            (_, None) => println!("시스템 관리 설정이 없습니다. 모든 권한과 명령을 사용자가 직접 관리합니다."),
            (path, Some(m)) => {
                println!("시스템 관리 설정: {}", path.map(|p| p.display().to_string()).unwrap_or_default());
                println!("  strict: {}", if m.strict { "예 — 명시된 항목만 사용하며 CLI/콘솔에서 수정 불가" } else { "아니오 — 명시된 항목은 고정되고 나머지 추가 가능" });
                println!("  권한: {}", if m.permissions.is_empty() { "없음".to_string() } else { m.permissions.join(", ") });
                println!("  명령: {}개", m.commands.len());
                for c in &m.commands {
                    println!("    {}  {:<8} {}", c.id, c.kind, c.name);
                }
            }
        },
        Some(other) => usage(&format!("알 수 없는 명령: managed {other}")),
    }
}

/// `command add-...` 옵션 파싱: `--이름 값` 쌍을 순회하며, 동일 옵션이 여러 번 나오면 순서대로 수집한다(`--step`).
fn options(words: &[String], known: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut it = words.iter();
    while let Some(w) = it.next() {
        let Some(key) = w.strip_prefix("--").filter(|k| known.contains(k)) else { usage(&format!("알 수 없는 옵션: {w} (사용 가능: {})", known.iter().map(|k| format!("--{k}")).collect::<Vec<_>>().join(" "))) };
        let value = it.next().unwrap_or_else(|| usage(&format!("--{key} 뒤에 값이 필요합니다")));
        out.push((key.to_string(), value.clone()));
    }
    out
}

/// `toss-connector command ...`: 덱 명령 위젯용 명령 관리 및 로컬 테스트 실행
pub fn command(cmds: &Commands, rest: &[String], tail: &[String]) {
    let pick = |key: Option<&String>| {
        let key = key.unwrap_or_else(|| usage("명령 ID 또는 이름을 지정하세요 (toss-connector command list 참고)"));
        cmds.find(key).unwrap_or_else(|| fail("일치하는 명령이 없거나 여러 개가 해당합니다"))
    };
    let name = || rest.get(1).filter(|n| !n.starts_with("--")).unwrap_or_else(|| usage("명령 이름을 지정하세요: toss-connector command add... <이름> ..."));
    let one = |opts: &[(String, String)], key: &str| opts.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default();
    let number = |opts: &[(String, String)], key: &str, default: f64| match one(opts, key).as_str() {
        "" => default,
        text => text.parse::<f64>().unwrap_or_else(|_| usage(&format!("--{key} 뒤에는 숫자를 지정해야 합니다"))),
    };
    let registered = |c: Command| {
        let c = cmds.add(c).unwrap_or_else(|e| fail(&e.to_string()));
        println!("등록됨: {}  {}  ({})", c.id, c.name, c.kind);
        if c.has_value() && c.state.is_empty() {
            println!("상태 확인 커맨드(--state)가 없어 마지막으로 설정한 값만 표시됩니다 (PC에서 직접 바꾼 상태는 감지 못 함).");
        }
        println!("덱에서 사용하려면 권한을 허용하세요: toss-connector allow command.run");
    };
    let after_name = &rest[2.min(rest.len())..];
    match rest.first().map(String::as_str) {
        None | Some("list") => {
            let list = cmds.list();
            if list.is_empty() {
                println!("등록된 명령이 없습니다. `toss-connector command add <이름> -- <명령...>`으로 추가하세요 (스위치·노브는 add-switch, add-stepped, add-linear).");
            }
            if cmds.locked() {
                println!("(명령 목록이 시스템 설정(Nix)으로 잠겨 있어 여기서 추가하거나 수정할 수 없습니다)");
            }
            let fixed = cmds.managed_ids();
            for c in list {
                println!("{}  {:<8} {}{}", c.id, c.kind, c.name, if fixed.contains(&c.id) { "   [Nix]" } else { "" });
                let mut lines: Vec<(String, &str)> = Vec::new();
                match c.kind.as_str() {
                    toss_proto::KIND_SWITCH => lines.extend([("on".to_string(), c.on.as_str()), ("off".to_string(), c.off.as_str())]),
                    toss_proto::KIND_STEPPED => {
                        lines.extend(c.steps.iter().map(|s| (format!("step {}", s.name), if s.run.is_empty() { "(set)" } else { s.run.as_str() })));
                        if !c.set.is_empty() {
                            lines.push(("set".to_string(), c.set.as_str()));
                        }
                    }
                    toss_proto::KIND_LINEAR => lines.push((format!("set {}~{} /{}", c.min, c.max, c.step), c.set.as_str())),
                    _ => lines.push(("run".to_string(), c.run.as_str())),
                }
                if !c.state.is_empty() {
                    lines.push((if c.every > 0.0 { format!("state /{}s", c.every) } else { "state".to_string() }, c.state.as_str()));
                }
                for (what, line) in lines {
                    println!("          {what:<18} {line}");
                }
            }
        }
        Some("add") => {
            if tail.is_empty() {
                usage("실행할 커맨드를 '--' 뒤에 지정하세요: toss-connector command add <이름> -- <명령...>");
            }
            registered(Command::run(name(), &tail.join(" ")));
        }
        Some("add-switch") => {
            let o = options(after_name, &["on", "off", "state", "every"]);
            registered(Command { every: number(&o, "every", 0.0), ..Command::switch(name(), &one(&o, "on"), &one(&o, "off"), &one(&o, "state")) });
        }
        Some("add-stepped") => {
            let o = options(after_name, &["step", "set", "state", "every"]);
            let steps: Vec<(&str, &str)> = o.iter().filter(|(k, _)| k == "step").map(|(_, v)| v.split_once('=').unwrap_or((v.as_str(), ""))).collect();
            registered(Command { every: number(&o, "every", 0.0), ..Command::stepped(name(), &steps, &one(&o, "set"), &one(&o, "state")) });
        }
        Some("add-linear") => {
            let o = options(after_name, &["set", "min", "max", "step", "state", "every"]);
            let range = (number(&o, "min", 0.0), number(&o, "max", 100.0), number(&o, "step", 1.0));
            registered(Command { every: number(&o, "every", 0.0), ..Command::linear(name(), range, &one(&o, "set"), &one(&o, "state")) });
        }
        Some("remove") => {
            let c = pick(rest.get(1));
            cmds.remove(&c.id).unwrap_or_else(|e| fail(&e.to_string()));
            println!("삭제됨: {}. 이 명령을 가리키던 덱 위젯은 비활성화됩니다.", c.name);
        }
        Some("run") => {
            let c = pick(rest.get(1));
            if c.has_value() {
                fail("값을 설정하는 명령입니다. `toss-connector command set <이름> <값>`을 사용하세요");
            }
            Commands::run_by(&c).unwrap_or_else(|e| fail(&e.to_string()));
            println!("실행됨: {}", c.run);
        }
        Some("set") => {
            let c = pick(rest.get(1));
            let text = rest.get(2).unwrap_or_else(|| usage("설정할 값을 지정하세요: toss-connector command set <이름> <값>"));
            let value = c.parse_value(text).unwrap_or_else(|e| fail(&e));
            c.set_value(value).unwrap_or_else(|e| fail(&format!("값 설정 실패: {e}")));
            println!("적용됨: {} = {}", c.name, c.show_value(Some(value)));
            if !c.state.is_empty() {
                let until = Instant::now() + Duration::from_secs(4);
                let mut seen = c.read_value();
                while seen != Some(value) && Instant::now() < until {
                    thread::sleep(Duration::from_millis(400));
                    seen = c.read_value();
                }
                println!("현재 상태: {}{}", c.show_value(seen), if seen == Some(value) { "" } else { "  (설정한 값과 다름)" });
            }
        }
        Some("state") => {
            let c = pick(rest.get(1));
            if c.state.is_empty() {
                fail("이 명령에는 상태 확인 커맨드(--state)가 설정되어 있지 않습니다");
            }
            println!("{}", c.show_value(c.read_value()));
        }
        Some(other) => usage(&format!("알 수 없는 명령: command {other}")),
    }
}
