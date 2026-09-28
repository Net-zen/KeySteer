#!/usr/bin/env python3
"""Extract a verified SDK archive, then expose its sysroot to later CI steps."""
import json
import os
from pathlib import Path
import sys
import tarfile


def sdk_root(directory: Path) -> Path:
    candidates = [p.parent for p in directory.rglob('SDKSettings.json')
                  if p.parent.name.startswith('MacOSX') and p.parent.suffix == '.sdk']
    if len(candidates) != 1:
        raise ValueError('SDK archive must contain exactly one MacOSX*.sdk with SDKSettings.json')
    root = candidates[0]
    settings = json.loads((root / 'SDKSettings.json').read_text())
    if int(settings['Version'].split('.')[0]) < 14:
        raise ValueError('KeySteer requires a macOS SDK version of at least 14')
    for item in ('usr/lib/libSystem.tbd', 'System/Library/Frameworks/AppKit.framework'):
        if not (root / item).exists():
            raise ValueError(f'Incomplete macOS SDK: missing {item}')
    return root.resolve()


def main() -> None:
    directory = Path(os.environ['MACOS_SDK_DIR'])
    if len(sys.argv) == 2:
        directory.mkdir(parents=True, exist_ok=True)
        with tarfile.open(sys.argv[1], 'r:*') as archive:
            # Preserve SDK symlinks, but reject traversal and links outside it.
            archive.extractall(directory, filter='data')
    root = sdk_root(directory)
    with open(os.environ['GITHUB_ENV'], 'a') as output:
        output.write(f'SDKROOT={root}\n')
    print(f'Using Apple SDK: {root.name}')


if __name__ == '__main__':
    main()
