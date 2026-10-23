//! Dependency-free diagnostic logging.
//!
//! Errors and panics are always persisted because the normal app may have no
//! console. Debug, info and warning records are emitted only while logging is
//! enabled by the active configuration. Files are rotated at a fixed bound.
//! A bounded FIFO writer owns file/console I/O and rotation. Saturation applies
//! backpressure rather than dropping errors. Explicit flush/shutdown and panic
//! reporting wait for queued records; none promise power-loss durability.
//! Callers must finish input recovery before reporting aggregated failures.

use std::backtrace::Backtrace;
use std::fmt;
use std::fs::{self, File, OpenOptions};
#[cfg(target_os = "macos")]
use std::io::IsTerminal;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::thread::{JoinHandle, ThreadId};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const RETAINED_LOGS: usize = 3;
const LOG_QUEUE_CAPACITY: usize = 256;

static LOGGER: OnceLock<Logger> = OnceLock::new();
static PANIC_HOOK: Once = Once::new();
static NON_ERROR_ENABLED: AtomicBool = AtomicBool::new(false);
static SESSION_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warning,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        })
    }
}

struct RepeatedError {
    target: String,
    message: String,
    /// Contextual records already start with the stable identity. Borrow its
    /// UTF-8 prefix instead of retaining a second owned copy of the error.
    identity_len: Option<usize>,
    console: bool,
    since: Instant,
    suppressed: u64,
}

struct LoggerState {
    repeated: Option<RepeatedError>,
    sender: Option<SyncSender<Output>>,
    sink_available: Arc<AtomicBool>,
}

struct Logger {
    path: PathBuf,
    state: Mutex<LoggerState>,
    writer: Mutex<Option<JoinHandle<()>>>,
    writer_id: ThreadId,
}

enum Output {
    Record {
        line: String,
        console: Option<String>,
        error: bool,
    },
    Flush(SyncSender<()>),
    #[cfg(test)]
    Pause {
        entered: SyncSender<()>,
        release: Receiver<()>,
    },
}

struct FileSink {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    available: Arc<AtomicBool>,
}

// Error rendering happens outside the logger mutex: a caller's Display
// implementation must not deadlock the panic hook. Common errors stay on stack.
struct MessageBuffer {
    bytes: [u8; 4096],
    len: usize,
}
impl fmt::Write for MessageBuffer {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self
            .len
            .checked_add(text.len())
            .filter(|end| *end <= self.bytes.len())
            .ok_or(fmt::Error)?;
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
fn suppress(
    state: &mut LoggerState,
    level: Level,
    target: &str,
    message: &str,
    identity: Option<&str>,
    console: bool,
    now: Instant,
) -> bool {
    if level == Level::Error
        && state.sink_available.load(Ordering::Acquire)
        && let Some(repeated) = &mut state.repeated
        && repeated.target == target
        && repeated.console == console
        && now.saturating_duration_since(repeated.since) < Duration::from_secs(1)
        && match identity {
            Some(key) => repeated
                .identity_len
                .is_some_and(|len| repeated.message.get(..len) == Some(key)),
            None => repeated.identity_len.is_none() && repeated.message == message,
        }
    {
        repeated.suppressed = repeated.suppressed.saturating_add(1);
        true
    } else {
        false
    }
}

impl Logger {
    fn open(path: PathBuf) -> io::Result<Self> {
        let sink = FileSink::open(path.clone())?;
        let sink_available = Arc::clone(&sink.available);
        let (sender, receiver) = mpsc::sync_channel(LOG_QUEUE_CAPACITY);
        let writer = std::thread::Builder::new()
            .name("diagnostic-log".into())
            .spawn(move || run_writer(sink, receiver))?;
        let writer_id = writer.thread().id();
        Ok(Self {
            path,
            state: Mutex::new(LoggerState {
                repeated: None,
                sender: Some(sender),
                sink_available,
            }),
            writer: Mutex::new(Some(writer)),
            writer_id,
        })
    }

    fn write(&self, level: Level, target: &str, message: &str) {
        self.write_at(level, target, message, Instant::now());
    }
    fn write_at(&self, level: Level, target: &str, message: &str, now: Instant) {
        self.emit(level, target, format_args!("{message}"), None, false, now);
    }
    fn emit(
        &self,
        level: Level,
        target: &str,
        message: fmt::Arguments<'_>,
        identity: Option<&str>,
        console: bool,
        now: Instant,
    ) {
        // The panic hook must never enqueue onto, or wait for, its own writer.
        if std::thread::current().id() == self.writer_id {
            write_emergency_stderr(message);
            return;
        }
        if let Some(key) = identity {
            {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if suppress(&mut state, level, target, "", identity, console, now) {
                    return;
                }
            }
            let message = format!("{key}; {message}");
            self.emit_rendered(level, target, &message, identity, console, now);
        } else if let Some(message) = message.as_str() {
            self.emit_rendered(level, target, message, None, console, now);
        } else {
            let mut buffer = MessageBuffer {
                bytes: [0; 4096],
                len: 0,
            };
            if fmt::write(&mut buffer, message).is_ok() {
                // Only complete &str chunks were appended.
                let text = std::str::from_utf8(&buffer.bytes[..buffer.len])
                    .unwrap_or("invalid diagnostic text");
                self.emit_rendered(level, target, text, None, console, now);
            } else {
                self.emit_rendered(level, target, &message.to_string(), None, console, now);
            }
        }
    }
    fn emit_rendered(
        &self,
        level: Level,
        target: &str,
        message: &str,
        identity: Option<&str>,
        console: bool,
        now: Instant,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if suppress(&mut state, level, target, message, identity, console, now) {
            return;
        }
        self.write_repeats(&mut state);
        Self::queue_record(&state, level, target, message, console);
        if level == Level::Error
            && state.sink_available.load(Ordering::Acquire)
            && target.len() + message.len() <= 4096
        {
            state.repeated = Some(RepeatedError {
                target: target.into(),
                message: message.into(),
                identity_len: identity.map(str::len),
                console,
                since: now,
                suppressed: 0,
            });
        }
    }
    fn write_repeats(&self, state: &mut LoggerState) {
        if let Some(repeated) = state.repeated.take()
            && repeated.suppressed > 0
        {
            let message = format!(
                "{} (repeated {} additional times)",
                repeated.message, repeated.suppressed
            );
            Self::queue_record(
                state,
                Level::Error,
                &repeated.target,
                &message,
                repeated.console,
            );
        }
    }

    fn queue_record(state: &LoggerState, level: Level, target: &str, message: &str, console: bool) {
        // Format here to retain the originating thread and timestamp. The
        // worker never takes state, including when a full queue applies backpressure.
        let output = Output::Record {
            line: format_line(level, target, message),
            console: console.then(|| message.to_owned()),
            error: level == Level::Error,
        };
        if state
            .sender
            .as_ref()
            .is_none_or(|sender| sender.send(output).is_err())
        {
            state.sink_available.store(false, Ordering::Release);
            write_emergency_stderr(format_args!("diagnostic writer unavailable: {message}"));
        }
    }

    fn flush(&self) {
        if std::thread::current().id() == self.writer_id {
            return;
        }
        let (sent, received) = mpsc::sync_channel(0);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.write_repeats(&mut state);
        let queued = state
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(Output::Flush(sent)).is_ok());
        drop(state);
        if queued {
            let _ = received.recv();
        }
    }

    fn shutdown(&self) {
        if std::thread::current().id() == self.writer_id {
            return;
        }
        {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            self.write_repeats(&mut state);
            // Disconnect after the final record. The worker drains FIFO and
            // flushes before returning; concurrent late reports use stderr.
            state.sender.take();
        }
        let mut writer = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(writer) = writer.take()
            && writer.join().is_err()
        {
            write_emergency_stderr(format_args!("diagnostic writer panicked"));
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_writer(mut sink: FileSink, receiver: Receiver<Output>) {
    while let Ok(output) = receiver.recv() {
        match output {
            Output::Record {
                line,
                console,
                error,
            } => {
                if let Some(message) = console {
                    write_emergency_stderr(format_args!("{message}"));
                }
                sink.write_record(&line, error);
            }
            Output::Flush(sent) => {
                sink.flush();
                let _ = sent.send(());
            }
            #[cfg(test)]
            Output::Pause { entered, release } => {
                let _ = entered.send(());
                let _ = release.recv();
            }
        }
    }
    sink.flush();
}

impl FileSink {
    fn open(path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        rotate_before_open(&path)?;
        let file = open_append(&path)?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            path,
            file: Some(file),
            bytes,
            available: Arc::new(AtomicBool::new(true)),
        })
    }

    fn write_record(&mut self, line: &str, error_record: bool) {
        if self.file.is_none() {
            match open_append(&self.path) {
                Ok(file) => {
                    self.bytes = file.metadata().map_or(0, |metadata| metadata.len());
                    self.file = Some(file);
                }
                Err(error) => {
                    write_emergency_stderr(format_args!(
                        "cannot reopen diagnostic log {}: {error}",
                        self.path.display()
                    ));
                    return;
                }
            }
        }
        if self.bytes.saturating_add(line.len() as u64 + 1) > MAX_LOG_BYTES
            && let Err(error) = self.rotate()
        {
            write_emergency_stderr(format_args!(
                "cannot rotate diagnostic log {}: {error}",
                self.path.display()
            ));
        }
        let mut flush_failed = false;
        let write_result = match self.file.as_mut() {
            Some(file) => {
                let result = writeln!(file, "{line}");
                if result.is_ok()
                    && error_record
                    && let Err(error) = file.flush()
                {
                    write_emergency_stderr(format_args!(
                        "cannot flush diagnostic log {}: {error}",
                        self.path.display()
                    ));
                    flush_failed = true;
                }
                result
            }
            None => return,
        };
        if let Err(error) = write_result {
            write_emergency_stderr(format_args!(
                "cannot write diagnostic log {}: {error}",
                self.path.display()
            ));
            self.file = None;
            self.available.store(false, Ordering::Release);
            return;
        }
        if flush_failed {
            // A sink that cannot honour the Error flush contract is no
            // longer usable. Reopen it on the next record instead of keeping
            // a poisoned handle for the rest of the process.
            self.file = None;
            self.available.store(false, Ordering::Release);
            return;
        }
        self.bytes = self.bytes.saturating_add(line.len() as u64 + 1);
        self.available.store(true, Ordering::Release);
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut()
            && let Err(error) = file.flush()
        {
            self.file = None;
            self.available.store(false, Ordering::Release);
            return Err(error);
        }
        drop(self.file.take());
        let rotation = rotate_files(&self.path);
        match open_append(&self.path) {
            Ok(file) => {
                self.bytes = file.metadata().map_or(0, |metadata| metadata.len());
                self.file = Some(file);
            }
            Err(reopen_error) => {
                self.available.store(false, Ordering::Release);
                return match rotation {
                    Ok(()) => Err(reopen_error),
                    Err(rotation_error) => Err(io::Error::new(
                        reopen_error.kind(),
                        format!(
                            "rotation failed ({rotation_error}); reopening failed ({reopen_error})"
                        ),
                    )),
                };
            }
        }
        rotation
    }

    fn flush(&mut self) {
        let flush_error = self.file.as_mut().and_then(|file| file.flush().err());
        if let Some(error) = flush_error {
            write_emergency_stderr(format_args!(
                "cannot flush diagnostic log {}: {error}",
                self.path.display()
            ));
            self.file = None;
            self.available.store(false, Ordering::Release);
        }
    }
}

/// Initialise the logger in the active application data directory, falling
/// back to a process-writable temporary directory when necessary.
///
/// Startup treats failure as non-fatal: a read-only program directory should
/// disable logging, not prevent the tray application from running.
pub fn init(candidate_paths: impl IntoIterator<Item = PathBuf>) -> Result<&'static Path, String> {
    if let Some(logger) = LOGGER.get() {
        return Ok(&logger.path);
    }
    let mut failures = Vec::new();
    for path in candidate_paths {
        match Logger::open(path.clone()) {
            Ok(logger) => {
                if LOGGER.set(logger).is_ok() {
                    let Some(installed) = LOGGER.get() else {
                        return Err("logger installation completed without a logger".into());
                    };
                    let path = &installed.path;
                    if !failures.is_empty() {
                        report_error(
                            "logging",
                            format!(
                                "preferred diagnostic log was unavailable; using {}: {}",
                                path.display(),
                                failures.join("; ")
                            ),
                        );
                    }
                    return Ok(path);
                }
                return LOGGER
                    .get()
                    .map(|logger| logger.path.as_path())
                    .ok_or_else(|| "logger installation race produced no logger".into());
            }
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    Err(format!(
        "cannot create a diagnostic log ({})",
        failures.join("; ")
    ))
}

pub fn path() -> Option<&'static Path> {
    LOGGER.get().map(|logger| logger.path.as_path())
}

/// Enable or disable every non-error diagnostic level.
///
/// Errors deliberately bypass this switch. `Relaxed` ordering is sufficient:
/// the value controls observability only and does not publish program state.
pub fn set_non_error_enabled(enabled: bool) {
    NON_ERROR_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Start a diagnostic session after configuration has enabled non-error logs.
pub(crate) fn start_session(backend_name: &str) {
    if !level_enabled(Level::Info)
        || LOGGER.get().is_none()
        || SESSION_STARTED.swap(true, Ordering::AcqRel)
    {
        return;
    }
    let log_path = path().map_or_else(|| "<unavailable>".into(), |path| path.display().to_string());
    info(
        "session",
        format!(
            "session started version={} pid={} os={} arch={} profile={} backend={} executable={} log={}",
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
            std::env::consts::OS,
            std::env::consts::ARCH,
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            backend_name,
            std::env::current_exe().map_or_else(
                |error| format!("<unavailable: {error}>"),
                |value| value.display().to_string()
            ),
            log_path,
        ),
    );
}

pub(crate) fn end_session() {
    if SESSION_STARTED.swap(false, Ordering::AcqRel) {
        info("session", "session ended normally");
    }
}

/// Lazy variant used by callers that would otherwise build a formatted
/// message before discovering that non-error logging is disabled.
pub(crate) fn debug_args(target: &str, message: fmt::Arguments<'_>) {
    if level_enabled(Level::Debug) {
        log_args(Level::Debug, target, message);
    }
}

pub fn info(target: &str, message: impl AsRef<str>) {
    log(Level::Info, target, message.as_ref());
}

pub(crate) fn info_args(target: &str, message: fmt::Arguments<'_>) {
    if level_enabled(Level::Info) {
        log_args(Level::Info, target, message);
    }
}

#[cold]
#[inline(never)]
fn report(level: Level, target: &str, message: &str) {
    debug_assert!(level >= Level::Warning);
    report_args(level, target, format_args!("{message}"));
}
fn report_args(level: Level, target: &str, message: fmt::Arguments<'_>) {
    if let Some(logger) = LOGGER.get() {
        logger.emit(level, target, message, None, true, Instant::now());
    } else {
        write_emergency_stderr(message);
    }
}
/// Stable error identity is separate from request-specific context. The first
/// occurrence retains that context; subsequent occurrences retain an exact count.
pub(crate) fn report_error_context(target: &str, error: &str, context: fmt::Arguments<'_>) {
    if let Some(logger) = LOGGER.get() {
        logger.emit(
            Level::Error,
            target,
            context,
            Some(error),
            true,
            Instant::now(),
        );
    } else {
        write_emergency_stderr(format_args!("{error}; {context}"));
    }
}
#[cold]
#[inline(never)]
pub fn report_error(target: &str, message: impl AsRef<str>) {
    report(Level::Error, target, message.as_ref());
}
#[cold]
#[inline(never)]
pub(crate) fn report_error_args(target: &str, message: fmt::Arguments<'_>) {
    report_args(Level::Error, target, message);
}

/// Central emergency console path used before the persistent logger exists or
/// for CLI-only location hints. Platform and application modules must not
/// write stderr directly.
#[cold]
#[inline(never)]
pub(crate) fn emergency_console(message: impl fmt::Display) {
    write_emergency_stderr(format_args!("{message}"));
}

#[cfg(target_os = "macos")]
pub(crate) fn emergency_stderr_is_terminal() -> bool {
    io::stderr().is_terminal()
}

/// Best-effort emergency output must never panic while reporting another
/// failure. There is deliberately no recursive fallback after stderr itself
/// rejects a write.
#[cold]
#[inline(never)]
fn write_emergency_stderr(message: fmt::Arguments<'_>) {
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(b"KeySteer: ");
    let _ = stderr.write_fmt(message);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}

pub(crate) fn report_warning_args(target: &str, message: fmt::Arguments<'_>) {
    if level_enabled(Level::Warning) {
        report_args(Level::Warning, target, message);
    }
}

/// Formatting macros keep disabled warning/info paths allocation-free. Errors
/// bypass the non-error switch; the writer emits stderr/file and flushes errors.
#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        if $crate::support::logging::level_enabled($crate::support::logging::Level::Info) {
            $crate::support::logging::info_args($target, format_args!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! report_warning {
    ($target:expr, $($arg:tt)*) => {
        if $crate::support::logging::level_enabled($crate::support::logging::Level::Warning) {
            $crate::support::logging::report_warning_args($target, format_args!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! report_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::support::logging::report_error_args($target, format_args!($($arg)*))
    };
}

pub fn flush() {
    if let Some(logger) = LOGGER.get() {
        logger.flush();
    }
}

/// Called after runtime shutdown, including CLI failures. Unlike ordinary
/// logging this intentionally waits for every queued record and the writer.
pub(crate) fn shutdown() {
    if let Some(logger) = LOGGER.get() {
        logger.shutdown();
    }
}

/// Install once for the process. The previous hook remains responsible for
/// normal stderr output after the full diagnostic has been persisted.
pub fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            let payload = panic
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            let location = panic.location().map_or_else(
                || "unknown location".into(),
                |location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                },
            );
            report_error(
                "panic",
                format!(
                    "panic at {location}: {payload}\nbacktrace:\n{}",
                    Backtrace::force_capture()
                ),
            );
            flush();
            previous(panic);
        }));
    });
}

pub fn level_enabled(level: Level) -> bool {
    level_enabled_with(level, NON_ERROR_ENABLED.load(Ordering::Relaxed))
}

fn level_enabled_with(level: Level, non_error_enabled: bool) -> bool {
    level == Level::Error || non_error_enabled
}

fn log(level: Level, target: &str, message: &str) {
    if !level_enabled(level) {
        return;
    }
    if let Some(logger) = LOGGER.get() {
        logger.write(level, target, message);
    }
}

fn log_args(level: Level, target: &str, message: fmt::Arguments<'_>) {
    if let Some(logger) = LOGGER.get() {
        logger.emit(level, target, message, None, false, Instant::now());
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn rotate_before_open(path: &Path) -> io::Result<()> {
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
    {
        rotate_files(path)?;
    }
    Ok(())
}

fn rotate_files(path: &Path) -> io::Result<()> {
    let oldest = rotated_path(path, RETAINED_LOGS);
    if oldest.exists() {
        fs::remove_file(oldest)?;
    }
    for index in (1..RETAINED_LOGS).rev() {
        let source = rotated_path(path, index);
        if source.exists() {
            fs::rename(source, rotated_path(path, index + 1))?;
        }
    }
    if path.exists() {
        fs::rename(path, rotated_path(path, 1))?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(format!(".{index}"));
    PathBuf::from(value)
}

#[cold]
#[inline(never)]
fn format_line(level: Level, target: &str, message: &str) -> String {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("unnamed");
    let message = if message.contains('\n') {
        std::borrow::Cow::Owned(message.replace('\n', "\n    "))
    } else {
        std::borrow::Cow::Borrowed(message)
    };
    format!(
        "{} [{level}] [{target}] [thread={name} {:?}] {message}",
        UtcTimestamp(SystemTime::now()),
        thread.id()
    )
}

struct UtcTimestamp(SystemTime);

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let duration = self.0.duration_since(UNIX_EPOCH).unwrap_or_default();
        let seconds = duration.as_secs();
        let days = (seconds / 86_400) as i64;
        let second_of_day = seconds % 86_400;
        let (year, month, day) = civil_from_days(days);
        write!(
            formatter,
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
            second_of_day / 3_600,
            second_of_day % 3_600 / 60,
            second_of_day % 60,
            duration.subsec_millis()
        )
    }
}

/// Gregorian civil date from days since 1970-01-01 (Howard Hinnant's
/// public-domain civil calendar algorithm).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    fn temporary_log() -> PathBuf {
        std::env::temp_dir().join(format!(
            "keysteer-logging-{}-{}.log",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    // Wait for disk I/O without consuming the suppression window under test.
    fn drain_output(logger: &Logger) {
        let (sent, received) = mpsc::sync_channel(0);
        logger
            .state
            .lock()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Output::Flush(sent))
            .unwrap();
        received.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn slow_sink_does_not_block_the_first_error_and_shutdown_drains_fifo() {
        let path = temporary_log();
        let logger = Arc::new(Logger::open(path.clone()).unwrap());
        let (entered, waiting) = mpsc::sync_channel(0);
        let (release, paused) = mpsc::channel();
        logger
            .state
            .lock()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Output::Pause {
                entered,
                release: paused,
            })
            .unwrap();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let (sent, received) = mpsc::channel();
        let producer_logger = Arc::clone(&logger);
        let producer = std::thread::Builder::new()
            .name("error-origin".into())
            .spawn(move || {
                producer_logger.write(Level::Error, "test", "first queued error");
                producer_logger.write(Level::Info, "test", "second queued record");
                sent.send(()).unwrap();
            })
            .unwrap();
        let responsive = received.recv_timeout(Duration::from_secs(2)).is_ok();
        release.send(()).unwrap();
        producer.join().unwrap();
        logger.shutdown();
        assert!(
            responsive,
            "producer waited on a blocked sink with queue capacity available"
        );
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("first queued error"));
        assert!(lines[1].contains("second queued record"));
        assert!(
            lines
                .iter()
                .all(|line| line.contains("thread=error-origin"))
        );
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn saturated_queue_applies_backpressure_and_shutdown_preserves_every_record() {
        let path = temporary_log();
        let logger = Arc::new(Logger::open(path.clone()).unwrap());
        let (entered, waiting) = mpsc::sync_channel(0);
        let (release, paused) = mpsc::channel();
        logger
            .state
            .lock()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Output::Pause {
                entered,
                release: paused,
            })
            .unwrap();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        for index in 0..LOG_QUEUE_CAPACITY {
            logger.write(Level::Info, "test", &format!("record-{index}"));
        }
        let (started, starting) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        let producer_logger = Arc::clone(&logger);
        let producer = std::thread::spawn(move || {
            started.send(()).unwrap();
            producer_logger.write(Level::Error, "test", "final-error");
            done.send(()).unwrap();
        });
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        let blocked = completed.recv_timeout(Duration::from_millis(20)).is_err();
        release.send(()).unwrap();
        producer.join().unwrap();
        logger.shutdown();
        assert!(blocked, "a full bounded queue must retain its final error");
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), LOG_QUEUE_CAPACITY + 1);
        for (index, line) in lines[..LOG_QUEUE_CAPACITY].iter().enumerate() {
            assert!(line.ends_with(&format!("record-{index}")));
        }
        assert!(lines.last().unwrap().ends_with("final-error"));
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn timestamp_is_stable_utc_iso_8601() {
        let time = UNIX_EPOCH + std::time::Duration::from_millis(1_704_067_200_123);
        assert_eq!(UtcTimestamp(time).to_string(), "2024-01-01T00:00:00.123Z");
    }

    #[test]
    fn rotated_paths_keep_the_log_name_visible() {
        let path = Path::new("diagnostics/keysteer.log");
        assert_eq!(
            rotated_path(path, 2),
            PathBuf::from("diagnostics/keysteer.log.2")
        );
    }

    #[test]
    fn only_errors_bypass_the_configuration_switch() {
        assert!(level_enabled_with(Level::Error, false));
        assert!(!level_enabled_with(Level::Warning, false));
        assert!(!level_enabled_with(Level::Info, false));
        assert!(!level_enabled_with(Level::Debug, false));
        assert!(level_enabled_with(Level::Warning, true));
        assert!(level_enabled_with(Level::Info, true));
        assert!(level_enabled_with(Level::Debug, true));
    }

    #[test]
    fn logger_persists_context_and_message() {
        let path = temporary_log();
        let logger = Logger::open(path.clone()).unwrap();
        logger.write(Level::Error, "test-target", "first line\nsecond line");
        logger.flush();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[ERROR] [test-target]"), "{text}");
        assert!(text.contains("first line\n    second line"), "{text}");
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_errors_write_once_then_flush_an_exact_count() {
        let path = temporary_log();
        let logger = Logger::open(path.clone()).unwrap();
        let now = Instant::now();
        logger.write_at(Level::Error, "window", "unavailable", now);
        drain_output(&logger);
        let first = fs::read_to_string(&path).unwrap();
        for _ in 0..100 {
            logger.write_at(Level::Error, "window", "unavailable", now);
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), first);
        logger.flush();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("repeated 100 additional times"));
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_contextual_errors_skip_formatting_and_keep_first_context_and_count() {
        struct Context<'a>(&'a std::cell::Cell<u32>);
        impl fmt::Display for Context<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.set(self.0.get() + 1);
                formatter.write_str("request=first")
            }
        }
        let path = temporary_log();
        let logger = Logger::open(path.clone()).unwrap();
        let now = Instant::now();
        let formats = std::cell::Cell::new(0);
        logger.emit(
            Level::Error,
            "window",
            format_args!("{}", Context(&formats)),
            Some("timed out"),
            false,
            now,
        );
        drain_output(&logger);
        let allocation = stats_alloc::Region::new(crate::TEST_ALLOCATOR);
        for request in 0..100 {
            logger.emit(
                Level::Error,
                "window",
                format_args!("request={request} {}", Context(&formats)),
                Some("timed out"),
                false,
                now,
            );
        }
        let stats = allocation.change();
        assert_eq!((stats.allocations, stats.reallocations), (0, 0));
        assert_eq!(formats.get(), 1);
        logger.flush();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("request=first"));
        assert!(text.contains("repeated 100 additional times"));
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn contextual_identity_prefix_is_utf8_exact_and_does_not_merge_shorter_keys() {
        let path = temporary_log();
        let logger = Logger::open(path.clone()).unwrap();
        let now = Instant::now();
        for request in 0..2 {
            logger.emit(
                Level::Error,
                "audio",
                format_args!("request={request}"),
                Some("失败"),
                false,
                now,
            );
        }
        logger.emit(
            Level::Error,
            "audio",
            format_args!("request=2"),
            Some("失"),
            false,
            now,
        );
        logger.flush();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(text.contains("失败; request=0 (repeated 1 additional times)"));
        assert!(text.contains("失; request=2"));
        assert!(!text.contains("request=1"));
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_errors_do_not_merge_different_targets_messages_or_time_windows() {
        let path = temporary_log();
        let logger = Logger::open(path.clone()).unwrap();
        let now = Instant::now();
        for (target, message, time) in [
            ("one", "error", now),
            ("two", "error", now),
            ("two", "another", now),
            ("two", "another", now + Duration::from_secs(1)),
        ] {
            logger.write_at(Level::Error, target, message, time);
        }
        logger.flush();
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 4);
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn error_is_visible_and_flushed_when_non_error_logging_is_disabled() {
        let path = temporary_log();
        let logger = Logger::open(path.clone()).unwrap();
        set_non_error_enabled(false);
        assert!(level_enabled(Level::Error));
        logger.write(Level::Error, "test-target", "unconditional error");
        logger.flush();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("unconditional error"), "{text}");
        drop(logger);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_failed_file_sink_is_invalidated_and_reopened_on_the_next_error() {
        let path = temporary_log();
        let mut sink = FileSink::open(path.clone()).unwrap();
        let read_only = fs::File::open(&path).unwrap();
        sink.file = Some(read_only);

        sink.write_record("expected write failure", true);
        assert!(sink.file.is_none());
        assert!(!sink.available.load(Ordering::Acquire));

        sink.write_record("reopened sink", true);
        assert!(sink.available.load(Ordering::Acquire));
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("expected write failure"), "{text}");
        assert!(text.contains("reopened sink"), "{text}");
        drop(sink);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn disabled_macros_do_not_evaluate_targets_or_arguments() {
        let previous = NON_ERROR_ENABLED.swap(false, Ordering::Relaxed);
        let evaluated = std::cell::Cell::new(0);
        crate::log_info!(
            {
                evaluated.set(1);
                "unused"
            },
            "{}",
            {
                evaluated.set(2);
                "unused"
            }
        );
        crate::report_warning!(
            {
                evaluated.set(3);
                "unused"
            },
            "{}",
            {
                evaluated.set(4);
                "unused"
            }
        );
        NON_ERROR_ENABLED.store(previous, Ordering::Relaxed);
        assert_eq!(evaluated.get(), 0);
    }
}
