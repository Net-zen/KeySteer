"""Portable packaging checks; real LLVM/Apple SDK/signing still require CI."""
import importlib.util
import io
import json
import os
from pathlib import Path
import plistlib
import shutil
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


package = load('package-cross')
sdk = load('prepare-sdk')
resolver = load('resolve-sdk')


class CrossPackaging(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for relative in ('Cargo.toml', 'keysteer.default.toml',
                         'assets/icons/keysteer-icon.png', 'packaging/macos/Info.plist.in'):
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def binary(self, target):
        path = self.root / 'target' / target / 'release/keysteer'
        path.parent.mkdir(parents=True)
        path.write_bytes(struct.pack('<IIII', 0xFEEDFACF, package.CPUS[target], 0, 2))
        return path

    def test_both_architectures_layout_identity_icon_and_permissions(self):
        for target in package.CPUS:
            self.binary(target)
            app, version = package.make_bundle(self.root, target, self.root / target)
            plist = plistlib.loads((app / 'Contents/Info.plist').read_bytes())
            self.assertEqual(plist['CFBundleIdentifier'], 'com.keysteer.app')
            self.assertEqual(plist['LSMinimumSystemVersion'], '14.0')
            self.assertEqual(plist['CFBundleShortVersionString'], version)
            icon = (app / 'Contents/Resources/KeySteer.icns').read_bytes()
            self.assertEqual(icon[:4], b'icns')
            self.assertEqual(struct.unpack('>I', icon[4:8])[0], len(icon))
            archive = self.root / f'{target}.zip'
            package.make_archive(app.parent, archive)
            with zipfile.ZipFile(archive) as zipped:
                executable = zipped.getinfo('KeySteer/KeySteer.app/Contents/MacOS/KeySteer')
                self.assertEqual((executable.external_attr >> 16) & 0o777, 0o755)
                self.assertIn('KeySteer/keysteer.default.toml', zipped.namelist())
                self.assertTrue(all(name.startswith('KeySteer/') for name in zipped.namelist()))

    def test_wrong_architecture_is_rejected(self):
        binary = self.binary('x86_64-apple-darwin')
        with self.assertRaises(ValueError):
            package.validate_binary(binary, 'aarch64-apple-darwin')

    def test_signing_failure_produces_no_archive(self):
        target = 'aarch64-apple-darwin'
        self.binary(target)
        with patch.object(package, '__file__', str(self.root / 'packaging/macos/package-cross.py')), \
             patch('sys.argv', ['package-cross.py', target]), \
             patch.dict(os.environ, MACOS_SIGNING_P12_BASE64='', MACOS_NOTARY_KEY_JSON_BASE64=''), \
             patch.object(package.subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'rcodesign')):
            with self.assertRaises(subprocess.CalledProcessError):
                package.main()
        self.assertEqual(list((self.root / 'dist').rglob('*.zip')), [])

    def test_notarization_requires_certificate(self):
        with patch.object(package, '__file__', str(self.root / 'packaging/macos/package-cross.py')), \
             patch('sys.argv', ['package-cross.py', 'aarch64-apple-darwin']), \
             patch.dict(os.environ, MACOS_SIGNING_P12_BASE64='', MACOS_NOTARY_KEY_JSON_BASE64='e30='):
            with self.assertRaisesRegex(ValueError, 'requires'):
                package.main()

    def test_sdk_validation_and_old_sdk_rejection(self):
        root = self.root / 'MacOSX.sdk'
        (root / 'usr/lib').mkdir(parents=True)
        (root / 'usr/lib/libSystem.tbd').write_text('stub')
        (root / 'System/Library/Frameworks/AppKit.framework').mkdir(parents=True)
        settings = root / 'SDKSettings.json'
        settings.write_text(json.dumps({'Version': '14.5'}))
        self.assertEqual(sdk.sdk_root(self.root), root.resolve())
        settings.write_text(json.dumps({'Version': '13.3'}))
        with self.assertRaisesRegex(ValueError, 'at least 14'):
            sdk.sdk_root(self.root)

    def test_sdk_archive_cannot_escape_destination(self):
        archive = self.root / 'malicious.tar'
        with tarfile.open(archive, 'w') as output:
            member = tarfile.TarInfo('../escaped')
            member.size = 1
            output.addfile(member, io.BytesIO(b'x'))
        with patch.dict(os.environ, MACOS_SDK_DIR=str(self.root / 'sdk'),
                        GITHUB_ENV=str(self.root / 'env')), \
             patch('sys.argv', ['prepare-sdk.py', str(archive)]):
            with self.assertRaises(tarfile.FilterError):
                sdk.main()
        self.assertFalse((self.root / 'escaped').exists())

    def test_sdk_auto_selection_uses_newest_supported_version(self):
        def entry(version):
            return {'version': f'macOS {version}', 'architectures': ['x86_64', 'arm64'],
                    'github_download_url': f'https://github.com/joseluisq/macosx-sdks/releases/download/{version}/MacOSX{version}.sdk.tar.xz',
                    'github_download_sha256sum': 'a' * 64}
        _, digest, tag = resolver.select_sdk([entry('14.5'), entry('26.1'), entry('15.5')])
        self.assertEqual(tag, '26.1')
        self.assertEqual(digest, 'a' * 64)
        with self.assertRaises(ValueError):
            resolver.select_sdk([entry('13.3')])
        invalid = entry('26.1')
        invalid['github_download_url'] = 'https://other.example/sdk.tar.xz'
        with self.assertRaisesRegex(ValueError, 'Unexpected'):
            resolver.select_sdk([invalid])

    def test_custom_sdk_requires_url_and_digest_together(self):
        with patch.dict(os.environ, MACOS_SDK_URL='https://example.test/sdk.tar.xz',
                        MACOS_SDK_SHA256=''):
            with self.assertRaisesRegex(ValueError, 'both'):
                resolver.main()


if __name__ == '__main__':
    unittest.main()
