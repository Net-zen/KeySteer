//! User-approved bundle replacement; preparation never blocks AppKit/Engine.
mod candidate;
mod native;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::api::backend::{BackendEvent, UpdateCheckResult};
use crate::support::worker::WorkerJoin;
use serde::{Deserialize, Serialize};

const HELPER_ARG: &str = "--keysteer-apply-update";
const READY_ENV: &str = "KEYSTEER_UPDATE_READY_SOCKET";
const ARM_WAIT: Duration = Duration::from_secs(5);
const START_WAIT: Duration = Duration::from_secs(15);

#[derive(Debug)]
struct StartError {
    message: String,
    rollback_safe: bool,
}

impl StartError {
    fn stopped(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            rollback_safe: true,
        }
    }

    fn uncertain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            rollback_safe: false,
        }
    }
}

#[derive(Clone)]
pub(super) struct Download {
    pub(super) path: PathBuf,
    pub(super) latest: String,
    pub(super) sha256: Option<String>,
}

pub(super) struct Installer {
    worker: WorkerJoin,
    cancel: Arc<AtomicBool>,
}

impl Installer {
    pub(super) fn start(download: Download, sender: super::EventSender) -> Result<Self, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&cancel);
        let worker = WorkerJoin::spawn(
            "macOS update installer",
            std::thread::Builder::new().name("keysteer-install-update".into()),
            move || match prepare(&download, &stopped) {
                Ok(()) => {
                    let _ = sender.send(BackendEvent::Quit);
                }
                Err(error) if !stopped.load(Ordering::Acquire) => {
                    let _ = sender.send(BackendEvent::UpdateChecked(UpdateCheckResult::Failed(
                        format!("The installed application was not changed. {error}"),
                    )));
                }
                Err(_) => {}
            },
        )?;
        Ok(Self { worker, cancel })
    }

    pub(super) fn finished(&mut self) -> bool {
        match self.worker.reap_finished() {
            Ok(done) => done,
            Err(error) => {
                crate::support::logging::report_error("macos-update", error);
                true
            }
        }
    }
}

impl Drop for Installer {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Err(error) = self.worker.join_timeout(Duration::from_millis(250)) {
            crate::support::logging::report_error("macos-update", error);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Plan {
    parent: u32,
    target: PathBuf,
    version: String,
    digest: Option<String>,
    args: Vec<OsString>,
}

fn relaunch_arguments(mut args: Vec<OsString>, directory: &Path) -> Vec<OsString> {
    // LaunchServices does not retain the caller's cwd. Freeze explicit relative
    // config paths before leaving it, while preserving bare names that resolve
    // in KeySteer's application-data directory (the public CLI convention).
    let mut index = 0;
    while index + 1 < args.len() {
        if matches!(args[index].to_str(), Some("-c" | "--config")) {
            let path = Path::new(&args[index + 1]);
            if path.is_relative() && path.components().count() > 1 {
                args[index + 1] = directory.join(path).into_os_string();
            }
            index += 1;
        }
        index += 1;
    }
    args
}

struct Stage(PathBuf, bool);
impl Drop for Stage {
    fn drop(&mut self) {
        if self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn prepare(download: &Download, cancel: &AtomicBool) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = super::app_bundle_for_executable(&executable)
        .ok_or("automatic installation requires running the packaged KeySteer.app")?;
    let parent = target
        .parent()
        .ok_or("application has no install directory")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let root = parent.join(format!(".keysteer-update-{}-{nonce:x}", std::process::id()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|e| {
            format!(
                "cannot stage an update beside {} (the install directory must be writable): {e}",
                target.display()
            )
        })?;
    let mut stage = Stage(root.clone(), true);
    let version = download
        .latest
        .parse::<semver::Version>()
        .map_err(|e| e.to_string())?;
    let current = env!("CARGO_PKG_VERSION")
        .parse::<semver::Version>()
        .map_err(|e| e.to_string())?;
    if version <= current {
        return Err("the downloaded macOS version is not newer than the running version".into());
    }
    let app = candidate::extract(&download.path, &root, download.sha256.as_deref(), cancel)?;
    candidate::validate(&app, &version)?;
    candidate::same_identity(&target, &app, download.sha256.is_some())?;
    candidate::cancelled(cancel)?;
    let plan = Plan {
        parent: std::process::id(),
        target,
        version: version.to_string(),
        digest: download.sha256.clone(),
        args: relaunch_arguments(
            std::env::args_os().skip(1).collect(),
            &std::env::current_dir().map_err(|e| e.to_string())?,
        ),
    };
    let plan_path = root.join("plan.json");
    fs::write(
        &plan_path,
        serde_json::to_vec(&plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let listener = UnixListener::bind(root.join("arm.sock")).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut helper = Command::new(executable)
        .arg(HELPER_ARG)
        .arg(&plan_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Err(error) = arm_helper(&listener, &mut helper, cancel) {
        let _ = helper.kill();
        let _ = helper.wait();
        return Err(format!("cannot arm the update helper: {error}"));
    }
    // The helper now owns staged files and waits for this exact process to exit.
    stage.1 = false;
    Ok(())
}

fn arm_helper(
    listener: &UnixListener,
    helper: &mut Child,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut stream = accept_until(
        listener,
        || {
            helper
                .try_wait()
                .map(|status| status.is_some())
                .map_err(|e| e.to_string())
        },
        ARM_WAIT,
        cancel,
    )?;
    // Darwin inherits O_NONBLOCK from the listener. The handshake must wait
    // for each reply; socket timeouts alone do not clear nonblocking mode.
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(ARM_WAIT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(ARM_WAIT))
        .map_err(|e| e.to_string())?;
    read_token(&mut stream, b'R')?;
    candidate::cancelled(cancel)?;
    stream.write_all(b"C").map_err(|e| e.to_string())?;
    read_token(&mut stream, b'A')
}

fn read_token(stream: &mut UnixStream, expected: u8) -> Result<(), String> {
    let mut token = [0];
    stream.read_exact(&mut token).map_err(|e| e.to_string())?;
    if token[0] != expected {
        Err("invalid update handshake".into())
    } else {
        Ok(())
    }
}

fn read_startup_token(stream: &mut UnixStream) -> Result<(), String> {
    // Darwin can reject SO_RCVTIMEO after the peer has already closed, even
    // with its acknowledgement buffered. signal_ready intentionally closes
    // immediately, so use nonblocking reads with this helper's own deadline.
    stream.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + ARM_WAIT;
    let mut token = [0];
    loop {
        match stream.read(&mut token) {
            Ok(1) if token[0] == b'R' => return Ok(()),
            Ok(_) => return Err("invalid update startup acknowledgement".into()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() >= deadline {
            return Err("update startup acknowledgement timed out".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn accept_until(
    listener: &UnixListener,
    mut has_exited: impl FnMut() -> Result<bool, String>,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<UnixStream, String> {
    let deadline = Instant::now() + timeout;
    loop {
        candidate::cancelled(cancel)?;
        if has_exited()? {
            return Err("update process exited before reporting readiness".into());
        }
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() >= deadline {
            return Err("update process did not report readiness in time".into());
        }
        // Only this short-lived installation helper polls; the app has no
        // periodic update timer or idle process monitor.
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn run_internal_helper() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(HELPER_ARG)) {
        return None;
    }
    let result = (|| {
        let path = args.next().ok_or("update helper needs a plan")?;
        if args.next().is_some() {
            return Err("unexpected update helper arguments".into());
        }
        apply(Path::new(&path))
    })();
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            crate::support::logging::report_error("macos-update", &error);
            // Fixed script, with the error passed as an argv value, not code.
            let _ = Command::new("/usr/bin/osascript").args(["-e", "on run argv\ndisplay alert \"KeySteer Update\" message (item 1 of argv) as critical\nend run", &error]).status();
            ExitCode::FAILURE
        }
    })
}

fn apply(plan_path: &Path) -> Result<(), String> {
    let root = plan_path.parent().ok_or("update plan has no directory")?;
    let plan: Plan = serde_json::from_slice(&fs::read(plan_path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    if super::app_bundle_for_executable(&executable).as_ref() != Some(&plan.target)
        || root.parent() != plan.target.parent()
        || !root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".keysteer-update-"))
    {
        return Err("update plan does not belong to the running application".into());
    }
    let mut cleanup = Stage(root.to_path_buf(), true);
    let staged = root.join("KeySteer.app");
    let version = plan
        .version
        .parse::<semver::Version>()
        .map_err(|e| e.to_string())?;
    candidate::validate(&staged, &version)?;
    candidate::same_identity(&plan.target, &staged, plan.digest.is_some())?;
    let exited = native::ProcessExit::watch(plan.parent)?;
    let mut arm = UnixStream::connect(root.join("arm.sock")).map_err(|e| e.to_string())?;
    arm.set_read_timeout(Some(ARM_WAIT))
        .map_err(|e| e.to_string())?;
    arm.write_all(b"R").map_err(|e| e.to_string())?;
    read_token(&mut arm, b'C')?;
    arm.write_all(b"A").map_err(|e| e.to_string())?;
    drop(arm);
    exited.wait(Duration::from_secs(30))?;

    let result = (|| {
        candidate::validate(&staged, &version).map_err(|e| (e, true))?;
        candidate::same_identity(&plan.target, &staged, plan.digest.is_some())
            .map_err(|e| (e, true))?;
        exchange_and_start(&plan.target, &staged, || start_and_wait(&plan, root))
    })();
    match result {
        Ok(()) => Ok(()),
        Err((error, restored)) => {
            if restored {
                let restart = native::launch(&plan.target, &plan.args, None, START_WAIT);
                restart.map_err(|e| {
                    format!(
                        "{error}; previous app restored, but restart failed: {}",
                        e.message
                    )
                })?;
                Err(format!(
                    "{error}\nThe previous application was restored and restarted."
                ))
            } else {
                cleanup.1 = false;
                Err(format!(
                    "{error}\nThe previous application is preserved at {}.",
                    staged.display()
                ))
            }
        }
    }
}

fn exchange_and_start(
    target: &Path,
    staged: &Path,
    start: impl FnOnce() -> Result<(), StartError>,
) -> Result<(), (String, bool)> {
    native::exchange(target, staged)
        .map_err(|e| (format!("cannot replace the application: {e}"), true))?;
    if let Err(error) = start() {
        // A timed-out LaunchServices request may still start the new version.
        // Likewise, a failed termination must not swap files under a live app.
        // Keep both bundles recoverable until its process state is known.
        if !error.rollback_safe {
            return Err((error.message, false));
        }
        native::exchange(target, staged).map_err(|e| {
            (
                format!(
                    "{}; cannot restore the previous application: {e}",
                    error.message
                ),
                false,
            )
        })?;
        return Err((error.message, true));
    }
    Ok(())
}

fn start_and_wait(plan: &Plan, root: &Path) -> Result<(), StartError> {
    let socket = root.join("ready.sock");
    let listener = UnixListener::bind(&socket).map_err(|e| StartError::stopped(e.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| StartError::stopped(e.to_string()))?;
    let app = native::launch(&plan.target, &plan.args, Some(&socket), START_WAIT)?;
    let result = (|| {
        let mut stream = accept_until(
            &listener,
            || app.has_exited(),
            START_WAIT,
            &AtomicBool::new(false),
        )?;
        read_startup_token(&mut stream)
    })();
    result.map_err(|error| match app.stop() {
        Ok(()) => StartError::stopped(error),
        Err(stop) => {
            StartError::uncertain(format!("{error}; cannot confirm application exit: {stop}"))
        }
    })
}

pub(super) fn signal_ready() -> Result<(), String> {
    let Some(path) = std::env::var_os(READY_ENV) else {
        return Ok(());
    };
    UnixStream::connect(path)
        .and_then(|mut s| s.write_all(b"R"))
        .map_err(|e| format!("cannot acknowledge update startup: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_services_arguments_keep_explicit_configs_independent_of_working_directory() {
        for flag in ["-c", "--config"] {
            for (path, expected) in [
                ("keysteer.work.toml", "keysteer.work.toml"),
                ("./keysteer.work.toml", "/original/./keysteer.work.toml"),
                (
                    "profiles/工作 配置.toml",
                    "/original/profiles/工作 配置.toml",
                ),
                ("/absolute/keysteer.toml", "/absolute/keysteer.toml"),
            ] {
                let args =
                    relaunch_arguments(vec![flag.into(), path.into()], Path::new("/original"));
                assert_eq!(args, vec![OsString::from(flag), OsString::from(expected)]);
            }
        }
    }

    #[test]
    fn update_helper_handshake_waits_for_delayed_acknowledgement() {
        let root = std::env::temp_dir().join(format!(
            "keysteer-arm-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let _cleanup = Stage(root.clone(), true);
        let socket = root.join("arm.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut child = Command::new("/bin/sleep").arg("10").spawn().unwrap();
        let peer = std::thread::spawn(move || -> Result<(), String> {
            let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(ARM_WAIT))
                .map_err(|e| e.to_string())?;
            stream.write_all(b"R").map_err(|e| e.to_string())?;
            read_token(&mut stream, b'C')?;
            // The acknowledgement must arrive after the parent starts reading,
            // as it can when the helper is scheduled on another CPU.
            std::thread::sleep(Duration::from_millis(100));
            stream.write_all(b"A").map_err(|e| e.to_string())
        });
        let result = arm_helper(&listener, &mut child, &AtomicBool::new(false));
        let _ = child.kill();
        child.wait().unwrap();
        let peer_result = peer.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
        peer_result.unwrap();
    }

    #[test]
    fn startup_acknowledgement_survives_immediate_peer_close() {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"R").unwrap();
        drop(writer);
        read_startup_token(&mut reader).unwrap();
    }

    #[test]
    fn native_exit_notification_waits_for_the_watched_child() {
        let mut child = Command::new("/bin/sleep").arg("0.1").spawn().unwrap();
        let notification = native::ProcessExit::watch(child.id()).unwrap();
        notification.wait(Duration::from_secs(2)).unwrap();
        assert!(child.wait().unwrap().success());
        assert!(notification.poll(Duration::ZERO).unwrap());
        notification.wait(Duration::ZERO).unwrap();
        assert!(
            native::ProcessExit::watch(child.id())
                .unwrap()
                .poll(Duration::ZERO)
                .unwrap()
        );
    }

    #[test]
    fn bundle_exchange_rolls_back_failed_start_and_keeps_old_bundle_on_success() {
        let root = std::env::temp_dir().join(format!(
            "keysteer-update-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let _cleanup = Stage(root.clone(), true);
        let target = root.join("installed.app");
        let staged = root.join("candidate.app");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&staged).unwrap();
        fs::write(target.join("version"), "old").unwrap();
        fs::write(staged.join("version"), "new").unwrap();
        let result = exchange_and_start(&target, &staged, || {
            assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
            Err(StartError::stopped("injected startup failure"))
        });
        assert!(matches!(result, Err((_, true))));
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
        exchange_and_start(&target, &staged, || Ok(())).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
        assert_eq!(fs::read_to_string(staged.join("version")).unwrap(), "old");
        // A pending launch or unconfirmed termination must preserve both bundles
        // without restoring files underneath a possibly running new version.
        native::exchange(&target, &staged).unwrap();
        let result = exchange_and_start(&target, &staged, || {
            Err(StartError::uncertain("launch completion timed out"))
        });
        assert!(matches!(result, Err((_, false))));
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
        assert_eq!(fs::read_to_string(staged.join("version")).unwrap(), "old");
    }

    #[test]
    #[ignore = "requires a logged-in macOS desktop; launches only a disposable test application"]
    fn launch_services_preserves_arguments_and_handshake_and_stops_failed_startup() {
        // A short path also leaves room for the Unix socket's Darwin path limit.
        let root = PathBuf::from(format!("/private/tmp/ks-update-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let _cleanup = Stage(root.clone(), true);
        let app = root.join("更新 fixture.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        candidate::output(
            Command::new("/usr/bin/clang")
                .args(["-fobjc-arc", "-fblocks", "-framework", "AppKit"])
                .arg(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macos-update-app.m"),
                )
                .arg("-o")
                .arg(app.join("Contents/MacOS/KeySteer")),
        )
        .unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.keysteer.update-test.p{}</string>
<key>CFBundleExecutable</key><string>KeySteer</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSUIElement</key><true/>
</dict></plist>"#,
                std::process::id()
            ),
        )
        .unwrap();
        candidate::output(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&app),
        )
        .unwrap();
        let staged = root.join("staged.app");
        candidate::output(Command::new("/usr/bin/ditto").arg(&app).arg(&staged)).unwrap();
        candidate::output(
            Command::new("/usr/bin/plutil")
                .args(["-replace", "CFBundleVersion", "-string", "2"])
                .arg(staged.join("Contents/Info.plist")),
        )
        .unwrap();
        candidate::output(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&staged),
        )
        .unwrap();
        let old_report = root.join("old.plist");
        let old =
            native::launch(&app, &[old_report.as_os_str().to_owned()], None, START_WAIT).unwrap();
        let deadline = Instant::now() + START_WAIT;
        while !old_report.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(old_report.exists());
        // Keep the old image alive across the exact atomic replacement used by
        // the updater. LaunchServices must launch version 2, not reuse version 1.
        native::exchange(&app, &staged).unwrap();
        let report = root.join("report.plist");
        let args = vec![
            report.as_os_str().to_owned(),
            OsString::from("空 格 $literal;argument"),
        ];
        let socket = root.join("ready.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let launched = native::launch(&app, &args, Some(&socket), START_WAIT).unwrap();
        let handshake = accept_until(
            &listener,
            || launched.has_exited(),
            START_WAIT,
            &AtomicBool::new(false),
        )
        .and_then(|mut stream| read_startup_token(&mut stream));
        let stop = launched.stop();
        let old_still_running = !old.has_exited().unwrap();
        old.stop().unwrap();
        handshake.unwrap();
        stop.unwrap();
        assert!(old_still_running);
        let read_report = || -> serde_json::Value {
            serde_json::from_str(
                &candidate::output(
                    Command::new("/usr/bin/plutil")
                        .args(["-convert", "json", "-o", "-"])
                        .arg(&report),
                )
                .unwrap(),
            )
            .unwrap()
        };
        let details = read_report();
        assert_eq!(details["arguments"][2], "空 格 $literal;argument");
        assert_eq!(details["bundle"], app.to_str().unwrap());
        assert_eq!(details["version"], "2");
        assert_eq!(details["ready"], socket.to_str().unwrap());
        assert_ne!(
            details["parent"].as_u64().unwrap(),
            std::process::id() as u64
        );
        drop(listener);
        fs::remove_file(&socket).unwrap();
        fs::remove_file(&report).unwrap();

        // The rollback route uses the same bundle launcher without a ready env.
        native::exchange(&app, &staged).unwrap();
        let launched = native::launch(&app, &args, None, START_WAIT).unwrap();
        let deadline = Instant::now() + START_WAIT;
        while !report.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        launched.stop().unwrap();
        assert_eq!(read_report()["ready"], "");
        assert_eq!(read_report()["version"], "1");

        let mut plan = Plan {
            parent: 0,
            target: app,
            version: "0.0.0".into(),
            digest: None,
            args,
        };
        for argument in ["--bad-ready", "--exit"] {
            plan.args.push(argument.into());
            let failure = start_and_wait(&plan, &root).unwrap_err();
            assert!(failure.rollback_safe, "{failure:?}");
            let pid = read_report()["pid"].as_u64().unwrap() as u32;
            assert!(
                native::ProcessExit::watch(pid)
                    .unwrap()
                    .poll(Duration::ZERO)
                    .unwrap()
            );
            fs::remove_file(&socket).unwrap();
            plan.args.pop();
        }
    }
}
