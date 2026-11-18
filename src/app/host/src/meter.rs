//! 호스트 PC 리소스 사용량 전송 모듈. 실제 수집은 덱과 공용인 toss-meter 크레이트가 담당한다.
//!
//! resource.info 권한이 허용된 동안 전용 스레드(Meter)가 동작한다:
//!   - 측정 가능 항목(catalog): 시작 시 1회, 이후 디스크 변경 등을 감지해 변동 시에만 전송
//!   - 측정값(values): 덱이 구독(watch) 중인 항목만 1초 주기로 측정해 값이 바뀐 항목만 전송
//!     (덱의 리소스 위젯에서 이 PC를 보고 있지 않으면 측정 자체를 수행하지 않음)
//!
//! 핸들을 드롭하면 스레드가 종료된다.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use toss_proto::MetricInfo;

/// 측정 주기 및 카탈로그 재조회 주기
const SAMPLE_EVERY: Duration = Duration::from_secs(1);
const CATALOG_EVERY: Duration = Duration::from_secs(30);

pub enum Event {
    /// 측정 가능 리소스 목록 (초기 및 변경 시)
    Catalog(Vec<MetricInfo>),
    /// 변동된 측정값
    Values(BTreeMap<String, f64>),
}

pub struct Meter {
    tx: mpsc::Sender<Vec<String>>,
}

impl Meter {
    pub fn start(watch: Vec<String>, on_event: impl Fn(Event) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        thread::spawn(move || {
            let mut sampler = toss_meter::Sampler::new();
            let mut watch = watch;
            let mut catalog = sampler.catalog();
            on_event(Event::Catalog(catalog.clone()));
            let mut catalog_at = Instant::now();
            let mut sent: BTreeMap<String, f64> = BTreeMap::new();
            let mut next = Instant::now();
            loop {
                match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(w) => {
                        watch = w;
                        sent.clear();
                        sampler.sample(&watch);
                        next = Instant::now() + SAMPLE_EVERY;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                    Err(RecvTimeoutError::Timeout) => {}
                }
                next = Instant::now() + SAMPLE_EVERY;
                if catalog_at.elapsed() >= CATALOG_EVERY {
                    catalog_at = Instant::now();
                    let now = sampler.catalog();
                    if now != catalog {
                        catalog = now;
                        on_event(Event::Catalog(catalog.clone()));
                    }
                }
                if watch.is_empty() {
                    continue;
                }
                let changed: BTreeMap<String, f64> = sampler.sample(&watch).into_iter().filter(|(id, v)| sent.get(id) != Some(v)).collect();
                if !changed.is_empty() {
                    sent.extend(changed.clone());
                    on_event(Event::Values(changed));
                }
            }
        });
        Self { tx }
    }

    /// 덱의 구독(watch) 항목 변경 반영
    pub fn watch(&self, ids: Vec<String>) {
        let _ = self.tx.send(ids);
    }
}
