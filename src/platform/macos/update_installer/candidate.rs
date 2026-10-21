//! Extract a bounded release without following ZIP paths or running its code.
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use semver::Version;
use sha2::{Digest, Sha256};

const MAX_ARCHIVE: u64 = 10 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024;
const BUNDLE_PREFIX: &str = "KeySteer/KeySteer.app/";

pub(super) fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("update installation was cancelled".into())
    } else {
        Ok(())
    }
}

pub(super) fn output(command: &mut Command) -> Result<String, String> {
    let result = command.output().map_err(|e| {
        format!(
            "cannot run {}: {e}",
            command.get_program().to_string_lossy()
        )
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if !result.status.success() {
        return Err(format!(
            "{} failed: {}",
            command.get_program().to_string_lossy(),
            text.trim()
        ));
    }
    Ok(text)
}

pub(super) fn extract(
    download: &Path,
    root: &Path,
    expected: Option<&str>,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    cancelled(cancel)?;
    let mut bytes = Vec::new();
    File::open(download)
        .and_then(|f| f.take(MAX_ARCHIVE + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_ARCHIVE {
        return Err("downloaded ZIP has an invalid size".into());
    }
    if let Some(expected) = expected
        && Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            != expected
    {
        return Err("downloaded ZIP no longer matches its release SHA-256".into());
    }
    // All subsequent reads use this private, immutable-for-the-updater copy.
    let archive = root.join("release.zip");
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&archive)
        .and_then(|mut f| f.write_all(&bytes))
        .map_err(|e| e.to_string())?;
    drop(bytes);
    let listing = output(
        Command::new("/usr/bin/unzip")
            .args(["-Z", "-1"])
            .arg(&archive),
    )?;
    let entries = bundle_entries(&listing)?;
    let app = root.join("KeySteer.app");
    fs::create_dir(&app).map_err(|e| e.to_string())?;
    let mut remaining = MAX_EXPANDED;
    for (name, relative) in entries {
        cancelled(cancel)?;
        let destination = app.join(relative);
        let parent = destination.parent().ok_or("invalid bundle path")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&destination)
            .map_err(|e| e.to_string())?;
        // unzip only writes to stdout. Symlink entries become regular files;
        // no archive-controlled link, mode or path is applied to the filesystem.
        let mut child = Command::new("/usr/bin/unzip")
            .arg("-p")
            .arg(&archive)
            .arg(name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let result: Result<u64, String> = (|| {
            let mut source = child
                .stdout
                .take()
                .ok_or("missing ZIP output pipe")?
                .take(remaining + 1);
            let mut count = 0;
            let mut buffer = [0u8; 32768];
            loop {
                cancelled(cancel)?;
                let read = source.read(&mut buffer).map_err(|e| e.to_string())?;
                if read == 0 {
                    break;
                }
                count += read as u64;
                if count > remaining {
                    return Err("expanded update exceeds the size limit".into());
                }
                file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
            }
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(count)
        })();
        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        remaining -= result?;
        if !status.success() {
            return Err("cannot extract a complete, CRC-valid update entry".into());
        }
    }
    fs::set_permissions(
        app.join("Contents/MacOS/KeySteer"),
        fs::Permissions::from_mode(0o755),
    )
    .map_err(|e| e.to_string())?;
    Ok(app)
}

fn bundle_entries(listing: &str) -> Result<Vec<(&str, PathBuf)>, String> {
    if listing.lines().count() > 512 {
        return Err("update ZIP contains too many entries".into());
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for name in listing.lines() {
        if name.contains(['\\', '\0', '\r', '*', '?', '[', ']'])
            || Path::new(name)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err("update ZIP contains an unsafe entry name".into());
        }
        let Some(relative) = name.strip_prefix(BUNDLE_PREFIX) else {
            continue;
        };
        if relative.is_empty() || relative.ends_with('/') {
            continue;
        }
        // ditto can store AppleDouble metadata beside files rather than under
        // __MACOSX. It is archive metadata, not part of the signed app bundle.
        if Path::new(relative)
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with("._"))
        {
            continue;
        }
        if !seen.insert(relative.to_lowercase()) {
            return Err("update ZIP contains duplicate bundle entries".into());
        }
        entries.push((name, PathBuf::from(relative)));
    }
    for required in [
        "Contents/Info.plist",
        "Contents/MacOS/KeySteer",
        "Contents/_CodeSignature/CodeResources",
    ] {
        if !entries.iter().any(|(_, p)| p == Path::new(required)) {
            return Err(format!("update ZIP is missing {required}"));
        }
    }
    Ok(entries)
}

fn plist(app: &Path, key: &str) -> Result<String, String> {
    output(
        Command::new("/usr/bin/plutil")
            .args(["-extract", key, "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist")),
    )
    .map(|s| s.trim().to_owned())
}

pub(super) fn validate(app: &Path, version: &Version) -> Result<(), String> {
    if plist(app, "CFBundleIdentifier")? != "com.keysteer.app"
        || plist(app, "CFBundleExecutable")? != "KeySteer"
        || plist(app, "CFBundlePackageType")? != "APPL"
    {
        return Err("the update is not a KeySteer application bundle".into());
    }
    for key in ["CFBundleShortVersionString", "CFBundleVersion"] {
        if plist(app, key)?
            .parse::<Version>()
            .map_err(|e| e.to_string())?
            != *version
        {
            return Err("the update bundle version does not match the release".into());
        }
    }
    let mut header = [0u8; 16];
    File::open(app.join("Contents/MacOS/KeySteer"))
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| e.to_string())?;
    let cpu = if cfg!(target_arch = "aarch64") {
        0x0100000cu32
    } else {
        0x01000007u32
    };
    if header[..4] != 0xfeedfacfu32.to_le_bytes()
        || header[4..8] != cpu.to_le_bytes()
        || header[12..16] != 2u32.to_le_bytes()
    {
        return Err("the update executable does not match this Mac's architecture".into());
    }
    output(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app),
    )?;
    Ok(())
}

pub(super) fn same_identity(
    current: &Path,
    candidate: &Path,
    has_digest: bool,
) -> Result<(), String> {
    output(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(current),
    )?;
    let signature = output(
        Command::new("/usr/bin/codesign")
            .args(["-d", "-r-", "--verbose=4"])
            .arg(current),
    )?;
    if signature.lines().any(|s| s == "Signature=adhoc") {
        if !has_digest {
            return Err(
                "this ad-hoc signed installation requires a SHA-256 directly from GitHub; retry when GitHub is reachable"
                    .into(),
            );
        }
        // Existing ad-hoc releases have no stable publisher identity. The ZIP
        // digest protects the downloaded bytes, not TCC identity continuity.
        return Ok(());
    }
    let requirement = signature
        .lines()
        .find_map(|s| s.strip_prefix("designated => "))
        .ok_or("cannot read the installed app's signing requirement")?;
    verify_requirement(candidate, requirement)
}

fn verify_requirement(candidate: &Path, requirement: &str) -> Result<(), String> {
    // codesign treats -R arguments as filenames unless source text starts '='.
    let source = format!("={requirement}");
    output(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict", "-R", &source])
            .arg(candidate),
    )
    .map_err(|e| format!("the update does not preserve the installed signing identity: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_bundle_extraction_checks_receipt_metadata_and_tampering() {
        let root = std::env::temp_dir().join(format!(
            "keysteer-candidate-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let _cleanup = super::super::Stage(root.clone(), true);
        let payload = root.join("KeySteer");
        let app = payload.join("KeySteer.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir(app.join("Contents/Resources")).unwrap();
        let source = root.join("fixture.c");
        fs::write(&source, "int main(void) { return 0; }\n").unwrap();
        output(
            Command::new("/usr/bin/clang")
                .arg(&source)
                .arg("-o")
                .arg(app.join("Contents/MacOS/KeySteer")),
        )
        .unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.keysteer.app</string>
<key>CFBundleExecutable</key><string>KeySteer</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>0.13.0</string>
<key>CFBundleShortVersionString</key><string>0.13.0</string>
</dict></plist>"#,
        )
        .unwrap();
        output(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&app),
        )
        .unwrap();
        let version = Version::new(0, 13, 0);
        validate(&app, &version).unwrap();
        let archive = root.join("update.zip");
        output(
            Command::new("/usr/bin/ditto")
                .args(["-c", "-k", "--keepParent"])
                .arg(&payload)
                .arg(&archive),
        )
        .unwrap();
        let extracted_root = root.join("extracted");
        fs::create_dir(&extracted_root).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(extract(&archive, &extracted_root, Some(&"0".repeat(64)), &cancel).is_err());
        let digest = Sha256::digest(fs::read(&archive).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let extracted = extract(&archive, &extracted_root, Some(&digest), &cancel).unwrap();
        validate(&extracted, &version).unwrap();
        assert!(validate(&extracted, &Version::new(0, 13, 1)).is_err());
        assert!(same_identity(&app, &extracted, false).is_err());
        same_identity(&app, &extracted, true).unwrap();
        verify_requirement(&extracted, "identifier \"com.keysteer.app\"").unwrap();
        assert!(verify_requirement(&extracted, "identifier \"com.keysteer.other\"").is_err());
        OpenOptions::new()
            .append(true)
            .open(extracted.join("Contents/MacOS/KeySteer"))
            .unwrap()
            .write_all(b"modified")
            .unwrap();
        assert!(validate(&extracted, &version).is_err());
    }

    #[test]
    fn zip_paths_cannot_escape_or_expand_patterns() {
        let valid = "KeySteer/KeySteer.app/Contents/Info.plist\nKeySteer/KeySteer.app/Contents/MacOS/KeySteer\nKeySteer/KeySteer.app/Contents/_CodeSignature/CodeResources\n";
        assert_eq!(bundle_entries(valid).unwrap().len(), 3);
        for name in [
            "../escape",
            "/absolute",
            "KeySteer/KeySteer.app/../escape",
            "KeySteer/KeySteer.app/a*b",
            "KeySteer/KeySteer.app/a\\b",
            "KeySteer/KeySteer.app/Contents/info.plist",
        ] {
            assert!(
                bundle_entries(&format!("{valid}{name}\n")).is_err(),
                "{name}"
            );
        }
    }
}
