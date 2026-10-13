//! Diagnostic timing log (`<config>/logs/perf.log`) for finding stalls: steps
//! of slow paths are logged with their duration, and a watchdog thread logs
//! whenever the UI thread stops responding, naming the last step it started.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::App;

/// Steps shorter than this are not logged by [`Span`].
const SPAN_THRESHOLD: Duration = Duration::from_millis(15);
/// The UI thread counts as stalled when its heartbeat is older than this.
const STALL_THRESHOLD: Duration = Duration::from_millis(250);
const HEARTBEAT: Duration = Duration::from_millis(50);
/// A log larger than this is started afresh at launch.
const MAX_LOG_SIZE: u64 = 2 * 1024 * 1024;

struct Log {
    file: Option<File>,
    last_step: String,
}

fn log() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(|| {
        // Tests do not write into the real config folder.
        let file = log_path().filter(|_| !cfg!(test)).and_then(|path| {
            std::fs::create_dir_all(path.parent()?).ok()?;
            let too_large = std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_SIZE);
            OpenOptions::new()
                .create(true)
                .append(!too_large)
                .write(true)
                .truncate(too_large)
                .open(path)
                .ok()
        });
        Mutex::new(Log {
            file,
            last_step: String::new(),
        })
    })
}

pub(crate) fn log_path() -> Option<PathBuf> {
    crate::config::VelotypeConfigDirs::from_system()
        .ok()
        .map(|dirs| dirs.logs_dir().join("perf.log"))
}

fn write_line(log: &mut Log, text: &str) {
    if let Some(file) = log.file.as_mut() {
        let time = chrono::Local::now().format("%H:%M:%S%.3f");
        let _ = writeln!(file, "{time}  {text}");
    }
}

/// Logs `text` and remembers it as the step the UI thread is in.
pub(crate) fn step(text: impl Into<String>) {
    let text = text.into();
    if let Ok(mut log) = log().lock() {
        write_line(&mut log, &text);
        log.last_step = text;
    }
}

/// Logs `text` without changing the current step.
pub(crate) fn note(text: impl AsRef<str>) {
    if let Ok(mut log) = log().lock() {
        write_line(&mut log, text.as_ref());
    }
}

/// Times a step: remembers it as current and logs its duration when dropped
/// (if it took at least [`SPAN_THRESHOLD`]).
#[must_use]
pub(crate) struct Span {
    label: String,
    start: Instant,
}

impl Span {
    pub(crate) fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        if let Ok(mut log) = log().lock() {
            log.last_step = label.clone();
        }
        Self {
            label,
            start: Instant::now(),
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed();
        if let Ok(mut log) = log().lock() {
            if elapsed >= SPAN_THRESHOLD {
                let text = format!("{} – {} ms", self.label, elapsed.as_millis());
                write_line(&mut log, &text);
            }
            // Work after the step (e.g. layout after a render) shows as "nach".
            log.last_step = format!("nach: {}", self.label);
        }
    }
}

fn millis_since(origin: Instant) -> u64 {
    origin.elapsed().as_millis() as u64
}

/// Starts the heartbeat on the UI thread and the watchdog thread that logs
/// every stall of it.
pub(crate) fn start(cx: &mut App) {
    static HEARTBEAT_AT: AtomicU64 = AtomicU64::new(0);
    let origin = Instant::now();
    note(format!(
        "--- Velotype {} gestartet (Protokoll: {}) ---",
        env!("CARGO_PKG_VERSION"),
        log_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    ));

    let timer = cx.background_executor().clone();
    cx.spawn(async move |_cx| {
        loop {
            HEARTBEAT_AT.store(millis_since(origin), Ordering::Relaxed);
            timer.timer(HEARTBEAT).await;
        }
    })
    .detach();

    let _ = std::thread::Builder::new()
        .name("velotype-watchdog".into())
        .spawn(move || {
            let mut stalled_since: Option<u64> = None;
            loop {
                std::thread::sleep(HEARTBEAT);
                let now = millis_since(origin);
                let beat = HEARTBEAT_AT.load(Ordering::Relaxed);
                let age = now.saturating_sub(beat);
                match stalled_since {
                    None if age >= STALL_THRESHOLD.as_millis() as u64 => {
                        stalled_since = Some(beat);
                        let step = log()
                            .lock()
                            .map(|log| log.last_step.clone())
                            .unwrap_or_default();
                        note(format!(
                            "!! UI-Thread hängt (seit {age} ms), Schritt: {step}"
                        ));
                    }
                    Some(since) if beat > since => {
                        stalled_since = None;
                        let step = log()
                            .lock()
                            .map(|log| log.last_step.clone())
                            .unwrap_or_default();
                        note(format!(
                            "!! UI-Thread wieder frei nach ~{} ms (zuletzt: {step})",
                            beat.saturating_sub(since)
                        ));
                    }
                    _ => {}
                }
            }
        });
}
