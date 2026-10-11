"""Create a missing release tag; preserve existing tag objects and commits."""

import re
import subprocess
import sys


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


if __name__ == "__main__":
    ensure_tag(*sys.argv[1:])
