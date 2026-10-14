"""Publish native/cross archives while preserving existing release tags."""

import os
from pathlib import Path
import re
import subprocess
import tomllib


def ensure_tag(tag, source_sha, cwd=None):
    def git(*args):
        return subprocess.run(
            ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
        ).stdout.strip()

    git("check-ref-format", f"refs/tags/{tag}")
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", source_sha):
        raise ValueError("Expected a complete source commit SHA")
    ref = f"refs/tags/{tag}"
    refs = {}
    for line in git("ls-remote", "origin", ref, f"{ref}^{{}}").splitlines():
        sha, name = line.split()
        refs[name] = sha
    if ref in refs:
        commit = refs.get(f"{ref}^{{}}", refs[ref])
        if commit != source_sha:
            raise ValueError(f"Existing tag {tag} points to {commit}, not {source_sha}")
        print(f"Keeping existing tag {tag} at {commit}")
        return
    # No force: a concurrent creation of this tag cannot be overwritten.
    git("push", "origin", f"{source_sha}:{ref}")
    print(f"Created tag {tag} at {source_sha}")


def release_identity(version, tag, release_type, package_set, run_number):
    if release_type not in ("release", "pre-release"):
        raise ValueError(f"Unknown release type: {release_type}")
    if package_set not in ("cross", "native"):
        raise ValueError(f"Unknown package set: {package_set}")
    prerelease = release_type == "pre-release"
    if tag:
        prerelease |= bool(re.match(r"^v?\d+\.\d+\.\d+-", tag))
        title = f"KeySteer {tag}"
    elif prerelease:
        tag = f"v{version}-pre"
        title = f"KeySteer {tag}"
        if package_set == "native":
            tag += f".{run_number}"
            title = f"KeySteer v{version} pre-release #{run_number}"
    else:
        tag = f"v{version}"
        title = f"KeySteer {tag}"
    latest = ["--latest=false"] if prerelease else (["--latest"] if package_set == "cross" else [])
    return tag, title, prerelease, latest


def verify_assets(directory, version, package_set):
    targets = [
        "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
        "aarch64-apple-darwin", "x86_64-apple-darwin",
    ]
    if package_set == "cross":
        targets += ["x86_64-pc-windows-msvc-compatible", "x86_64-pc-windows-msvc-avx512"]
    elif package_set != "native":
        raise ValueError(f"Unknown package set: {package_set}")
    directory = Path(directory)
    expected = {f"KeySteer-v{version}-{target}.zip" for target in targets}
    packages = sorted(directory.glob("*.zip"))
    if {path.name for path in packages} != expected or list(directory.glob("*.sha256")):
        raise ValueError("Release assets must contain exactly the expected platform ZIPs")
    if any(not path.is_file() or path.stat().st_size == 0 for path in packages):
        raise ValueError("Release archives must be nonempty files")
    return packages


def publish_release(tag, source_sha, title, prerelease, latest, packages, replace):
    # Validate the remote tag before touching an existing Release or its assets.
    ensure_tag(tag, source_sha)
    existing = subprocess.run(["gh", "release", "view", tag], capture_output=True, text=True)
    if existing.returncode == 0:
        if not replace:
            raise ValueError(f"Release already exists: {tag}")
        subprocess.run(["gh", "release", "delete", tag, "--yes"], check=True)
    subprocess.run([
        "gh", "release", "create", tag, "--verify-tag", "--target", source_sha,
        "--title", title, f"--prerelease={str(prerelease).lower()}", *latest,
        "--generate-notes", "--notes-file", "release-body.md",
        *(str(path) for path in packages),
    ], check=True)


def main():
    with open("Cargo.toml", "rb") as file:
        version = tomllib.load(file)["package"]["version"]
    package_set = os.environ["PACKAGE_SET"]
    tag, title, prerelease, latest = release_identity(
        version, os.environ.get("RELEASE_TAG", ""), os.environ["RELEASE_TYPE"],
        package_set, os.environ["GITHUB_RUN_NUMBER"],
    )
    packages = verify_assets("release-assets", version, package_set)
    subprocess.run(["bash", "tools/compose-release-notes.sh", version, "release-body.md"], check=True)
    publish_release(
        tag, os.environ["SOURCE_SHA"], title, prerelease, latest, packages,
        os.environ.get("REPLACE_RELEASE") == "true",
    )
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as file:
        file.write(f"tag={tag}\n")


if __name__ == "__main__":
    main()
