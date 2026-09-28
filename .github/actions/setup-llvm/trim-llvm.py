"""Trim a dedicated, extracted Linux LLVM installation before caching it."""
import os
from pathlib import Path
import sys


TOOLS = ('clang', 'clang++', 'clang-cl', 'lld', 'ld.lld', 'ld64.lld', 'lld-link',
         'llvm-ar', 'llvm-ranlib', 'llvm-lib', 'llvm-rc', 'llvm-cvtres',
         'llvm-nm', 'llvm-objcopy', 'llvm-strip', 'llvm-dlltool')


def trim(root: Path) -> None:
    root = root.resolve(strict=True)
    # Only operate on the dedicated CI installation, never a system LLVM tree.
    expected = Path(os.environ['RUNNER_TEMP']).resolve() / 'keysteer-llvm'
    if root != expected or not (root / 'bin/clang').is_file():
        raise ValueError('Expected RUNNER_TEMP/keysteer-llvm with bin/clang')
    files = [p for p in root.rglob('*') if p.is_file() or p.is_symlink()]
    keep = set()

    def retain(path):
        # Preserve every link in a chain, including versioned tool aliases.
        while path not in keep:
            path.relative_to(root)
            keep.add(path)
            if not path.is_symlink():
                break
            path = Path(os.path.abspath(path.parent / os.readlink(path)))
            path.relative_to(root)

    for tool in TOOLS:
        path = root / 'bin' / tool
        if not path.is_file():
            raise ValueError(f'Missing LLVM tool: {tool}')
        retain(path)
    for path in files:
        relative = path.relative_to(root)
        # Retain all host shared libraries and all Clang resource headers and
        # runtimes. Development archives for embedding LLVM are not used by CI.
        if (relative.parts[0] in ('lib', 'lib64') and
                ('.so' in path.name or 'clang' in relative.parts[1:-1])):
            retain(path)
        if path.name.lower().startswith(('license', 'notice')):
            retain(path)
        if relative.as_posix() in ('lib', 'lib64') and path.is_symlink():
            retain(path)
    before = sum(p.lstat().st_size for p in files)
    for path in files:
        if path not in keep:
            path.unlink()
    for directory, _, _ in os.walk(root, topdown=False, followlinks=False):
        path = Path(directory)
        if path != root and not any(path.iterdir()):
            path.rmdir()
    after = sum(p.lstat().st_size for p in keep)
    print(f'LLVM files: {before / 2**20:.0f} MiB -> {after / 2**20:.0f} MiB '
          '(before cache compression)')


if __name__ == '__main__':
    trim(Path(sys.argv[1]))
