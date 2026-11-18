//! 현재 기기에서 측정 가능한 리소스와 실시간 값을 출력하는 예제: cargo run --example show
fn main() {
    let mut s = toss_meter::Sampler::new();
    let list = s.catalog();
    let ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
    s.sample(&ids);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let now = s.sample(&ids);
    for m in &list {
        let v = now.get(&m.id).map(|v| format!("{v}")).unwrap_or_else(|| "-".into());
        println!("{:<12} {:<6} {:<28} {:>16} {:<4} (상한 {})", m.id, m.name, m.title, v, m.unit, m.max);
    }
}
