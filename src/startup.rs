//! Startup timing.
//!
//! `ZWINMD_BENCH=1` prints how long each stage of startup took, then exits once
//! the first frame is on screen. Without it, marks cost one atomic load each.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());
static START: Mutex<Option<Instant>> = Mutex::new(None);

/// Call first thing in `main`.
pub fn begin() -> Instant {
    let now = Instant::now();
    if std::env::var_os("ZWINMD_BENCH").is_some() {
        ENABLED.store(true, Ordering::Relaxed);
        *START.lock().unwrap() = Some(now);
    }
    now
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Record that startup reached `stage`.
pub fn mark(stage: &'static str) {
    if !enabled() {
        return;
    }
    if let Some(start) = *START.lock().unwrap() {
        MARKS.lock().unwrap().push((stage, start.elapsed().as_secs_f64() * 1000.));
    }
}

/// Print every stage with the time it took since the one before.
pub fn report() {
    let marks = MARKS.lock().unwrap();
    let mut previous = 0.;
    for (stage, at) in marks.iter() {
        println!("{:>7.1} ms  +{:>6.1}  {stage}", at, at - previous);
        previous = *at;
    }
}
