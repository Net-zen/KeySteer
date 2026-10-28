#![cfg(target_os = "macos")]

use std::process::Command;

#[test]
fn actual_capture_result_overrides_stale_preflight_and_preserves_failure_kinds() {
    let root = std::env::temp_dir().join(format!(
        "keysteer-capture-permissions-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir(&root).unwrap();
    let binary = root.join("capture-permissions");
    let compile = Command::new("/usr/bin/clang")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "-fobjc-arc",
            "-fblocks",
            "-mmacosx-version-min=14.0",
            "tests/fixtures/macos-capture-permissions.m",
            "-framework",
            "Foundation",
            "-framework",
            "CoreGraphics",
            "-framework",
            "ScreenCaptureKit",
            "-framework",
            "Vision",
            "-o",
        ])
        .arg(&binary)
        .output()
        .unwrap();
    let run = compile
        .status
        .success()
        .then(|| Command::new(&binary).output().unwrap());
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = run.unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
