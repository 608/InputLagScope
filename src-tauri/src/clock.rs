use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn timestamp_ns() -> u64 {
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}
