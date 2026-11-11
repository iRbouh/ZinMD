//! Startup timing.
//!
//! `ZINMD_BENCH=1` prints how long each stage of startup took, then exits once
//! the first frame is on screen. Without it, marks cost one atomic load each.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());
static START: Mutex<Option<Instant>> = Mutex::new(None);

/// With `ZINMD_TRACE` set, what the libraries under the app report is printed
/// as well: which graphics card was chosen, and anything that went wrong on
/// the way to a window. Otherwise it is all dropped, at no cost.
struct Reports;

impl log::Log for Reports {
    fn enabled(&self, about: &log::Metadata) -> bool {
        // Everything that went wrong, and what the window toolkit did; the
        // rest have a great deal to say about very little.
        // (The library that draws diagrams warns about every shape it
        // draws; none of it is news.)
        (about.level() <= log::Level::Warn && !about.target().starts_with("usvg"))
            || (about.level() == log::Level::Info && about.target().starts_with("gpui"))
    }

    fn log(&self, report: &log::Record) {
        if self.enabled(report.metadata()) {
            eprintln!(
                "zinmd: {} [{}] {}",
                report.level().as_str().to_lowercase(),
                report.target(),
                report.args()
            );
        }
    }

    fn flush(&self) {}
}

/// Call first thing in `main`.
pub fn begin() -> Instant {
    let now = Instant::now();
    if std::env::var_os("ZINMD_TRACE").is_some() && log::set_logger(&Reports).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    if std::env::var_os("ZINMD_BENCH").is_some() {
        ENABLED.store(true, Ordering::Relaxed);
        *START.lock().unwrap() = Some(now);
    }
    now
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Start loading the graphics driver on another thread.
///
/// Creating the window needs the Metal device, and the first request for it in
/// a process loads the GPU driver. Asking for it here lets that happen while
/// the main thread is still setting the application up.
pub fn warm_up_graphics() {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "Metal", kind = "framework")]
        unsafe extern "C" {
            fn MTLCreateSystemDefaultDevice() -> *mut std::ffi::c_void;
        }
        if std::env::var_os("ZINMD_NO_WARMUP").is_none() {
            // The device is deliberately kept: the renderer asks for the same one.
            std::thread::spawn(|| {
                unsafe { MTLCreateSystemDefaultDevice() };
            });
        }
    }
}

/// Say what is going on, when `ZINMD_TRACE` is set. For checking behaviour
/// that cannot be seen from outside, such as requests from the system.
pub fn trace(message: impl FnOnce() -> String) {
    static ON: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("ZINMD_TRACE").is_some());
    if *ON {
        eprintln!("zinmd: {}", message());
    }
}

/// Record that startup reached `stage`.
pub fn mark(stage: &'static str) {
    if !enabled() {
        return;
    }
    if let Some(start) = *START.lock().unwrap() {
        MARKS
            .lock()
            .unwrap()
            .push((stage, start.elapsed().as_secs_f64() * 1000.));
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
