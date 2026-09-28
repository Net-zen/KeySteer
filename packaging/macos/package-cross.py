#!/usr/bin/env python3
"""Package a Linux-built Mach-O with the same public layout as package.sh."""
import base64
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import sys
import tempfile
import tomllib
import zipfile


CPUS = {'aarch64-apple-darwin': 0x0100000C, 'x86_64-apple-darwin': 0x01000007}


def validate_binary(binary: Path, target: str) -> None:
    with binary.open('rb') as source:
        header = source.read(16)
    if len(header) != 16:
        raise ValueError('Truncated macOS executable')
    magic, cpu, _, filetype = struct.unpack('<IIII', header)
    if magic != 0xFEEDFACF or cpu != CPUS[target] or filetype != 2:
        raise ValueError(f'Expected a 64-bit Mach-O executable for {target}')


def make_bundle(root: Path, target: str, destination: Path) -> tuple[Path, str]:
    binary = root / 'target' / target / 'release/keysteer'
    validate_binary(binary, target)
    version = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    app = destination / 'KeySteer/KeySteer.app'
    contents = app / 'Contents'
    (contents / 'MacOS').mkdir(parents=True)
    (contents / 'Resources').mkdir()
    executable = contents / 'MacOS/KeySteer'
    shutil.copyfile(binary, executable)
    executable.chmod(0o755)
    template = (root / 'packaging/macos/Info.plist.in').read_text()
    plist = plistlib.loads(template.replace('@VERSION@', version).replace('@MIN_MACOS@', '14.0').encode())
    (contents / 'Info.plist').write_bytes(plistlib.dumps(plist, sort_keys=False))
    (contents / 'PkgInfo').write_bytes(b'APPL????')
    # Modern ICNS accepts a PNG-backed 256px element. Keep the original pixels
    # instead of depending on macOS sips/iconutil or resizing the source icon.
    png = (root / 'assets/icons/keysteer-icon.png').read_bytes()
    if png[:8] != b'\x89PNG\r\n\x1a\n' or struct.unpack('>II', png[16:24]) != (256, 256):
        raise ValueError('Expected the shipped 256x256 PNG icon')
    chunk = b'ic08' + struct.pack('>I', len(png) + 8) + png
    (contents / 'Resources/KeySteer.icns').write_bytes(b'icns' + struct.pack('>I', len(chunk) + 8) + chunk)
    shutil.copyfile(root / 'keysteer.default.toml', destination / 'KeySteer/keysteer.default.toml')
    return app, version


def make_archive(payload: Path, archive: Path) -> None:
    # Unix attributes preserve the executable bit when extracted on macOS.
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as output:
        for path in sorted(payload.rglob('*')):
            if path.is_file():
                name = path.relative_to(payload.parent).as_posix()
                info = zipfile.ZipInfo.from_file(path, name)
                info.create_system = 3
                mode = 0o100755 if name.endswith('/Contents/MacOS/KeySteer') else 0o100644
                info.external_attr = mode << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                output.writestr(info, path.read_bytes())


def main() -> None:
    target = sys.argv[1] if len(sys.argv) == 2 else ''
    if target not in CPUS:
        raise ValueError(f'Unsupported macOS target: {target}')
    root = Path(__file__).resolve().parents[2]
    dist = root / 'dist' / target
    dist.mkdir(parents=True, exist_ok=True)
    certificate = os.environ.get('MACOS_SIGNING_P12_BASE64', '')
    notary_key = os.environ.get('MACOS_NOTARY_KEY_JSON_BASE64', '')
    if notary_key and not certificate:
        raise ValueError('Notarization requires MACOS_SIGNING_P12_BASE64')
    # A fresh staging directory prevents stale bundle files from entering a ZIP.
    with tempfile.TemporaryDirectory(prefix='macos-package-', dir=dist) as staging:
        app, version = make_bundle(root, target, Path(staging))
        archive = Path(staging) / f'KeySteer-v{version}-{target}.zip'
        with tempfile.TemporaryDirectory(prefix='keysteer-macos-sign-') as credentials:
            secret_dir = Path(credentials)
            sign = ['rcodesign', 'sign']
            if certificate:
                p12 = secret_dir / 'identity.p12'
                password = secret_dir / 'password'
                p12.write_bytes(base64.b64decode(certificate, validate=True))
                password.write_text(os.environ.get('MACOS_SIGNING_PASSWORD', '') + '\n')
                p12.chmod(0o600)
                password.chmod(0o600)
                sign += ['--p12-file', str(p12), '--p12-password-file', str(password),
                         '--code-signature-flags', 'runtime']
                if notary_key:
                    sign += ['--for-notarization']
            subprocess.run([*sign, str(app)], check=True)
            # rcodesign verifies Mach-O signatures, not Apple's full bundle or
            # Gatekeeper policy. Native validation is still a release test.
            subprocess.run(['rcodesign', 'verify', str(app / 'Contents/MacOS/KeySteer')], check=True)
            if not (app / 'Contents/_CodeSignature/CodeResources').is_file():
                raise ValueError('Signing did not produce bundle resource seals')
            make_archive(app.parent, archive)
            if notary_key:
                api_key = secret_dir / 'notary.json'
                api_key.write_bytes(base64.b64decode(notary_key, validate=True))
                api_key.chmod(0o600)
                subprocess.run(['rcodesign', 'notary-submit', '--api-key-file', str(api_key),
                                '--wait', str(archive)], check=True)
                subprocess.run(['rcodesign', 'staple', str(app)], check=True)
                make_archive(app.parent, archive)
        result = dist / archive.name
        archive.replace(result)
        print(result)


if __name__ == '__main__':
    main()
