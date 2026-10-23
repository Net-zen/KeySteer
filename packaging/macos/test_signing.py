import base64
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import signing


class SigningTests(unittest.TestCase):
    def setUp(self):
        self.certificate = b'fixture public certificate'
        pin = patch.object(signing, 'CERTIFICATE_SHA256', hashlib.sha256(self.certificate).hexdigest())
        pin.start()
        self.addCleanup(pin.stop)

    def certificate_report(self, certificate):
        return ('Subject is Issuer?: true\n-----BEGIN CERTIFICATE-----\n'
                + base64.b64encode(certificate).decode() + '\n-----END CERTIFICATE-----\n')

    def test_required_ci_identity_never_falls_back_to_local_or_ad_hoc(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'true', 'KEYSTEER_REQUIRE_MACOS_SIGNING': '1'}, clear=True):
            with self.assertRaisesRegex(ValueError, 'refusing an ad-hoc'):
                with signing.credentials():
                    self.fail('missing release identity was accepted')

    def test_ci_credentials_are_private_and_removed_after_failure(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'true',
                                    'MACOS_SIGNING_P12_BASE64': base64.b64encode(b'fixture').decode(),
                                    'MACOS_SIGNING_PASSWORD': 'fixture-password'}, clear=True):
            with self.assertRaisesRegex(RuntimeError, 'injected'):
                with signing.credentials() as arguments:
                    p12, password = Path(arguments[1]), Path(arguments[3])
                    self.assertEqual(p12.read_bytes(), b'fixture')
                    if os.name != 'nt':
                        self.assertEqual(p12.stat().st_mode & 0o777, 0o600)
                        self.assertEqual(password.stat().st_mode & 0o777, 0o600)
                    raise RuntimeError('injected')
            self.assertFalse(p12.exists())
            self.assertFalse(password.exists())

    def test_wrong_certificate_and_bad_password_are_rejected(self):
        valid = self.certificate_report(self.certificate)
        other = self.certificate_report(b'another certificate')
        cases = [(0, other, False), (1, valid, False), (0, '', False),
                 (0, other + valid, False), (0, valid + other, True)]
        for status, report, accepted in cases:
            with self.subTest(status=status, report=report), \
                 patch.object(signing, 'tool', return_value=['rcodesign']), \
                 patch.object(signing.subprocess, 'run', return_value=subprocess.CompletedProcess(
                     [], status, report, '')):
                if accepted:
                    self.assertEqual(signing.check_identity([]), (self.certificate, True))
                else:
                    with self.assertRaises(ValueError):
                        signing.check_identity([])

    @unittest.skipUnless(sys.platform == 'darwin', 'requires the independent Apple requirement compiler')
    def test_portable_requirement_matches_apple_compiler(self):
        digest = hashlib.sha1(self.certificate).hexdigest()
        with tempfile.TemporaryDirectory() as temporary:
            compiled = Path(temporary) / 'apple.bin'
            for identifier in ('com.keysteer.app', 'a', 'ab', 'abc', 'abcd'):
                with self.subTest(identifier=identifier):
                    expression = f'=identifier "{identifier}" and certificate leaf = H"{digest}"'
                    subprocess.run(['/usr/bin/csreq', '-r', expression, '-b', str(compiled)], check=True)
                    self.assertEqual(signing.code_requirement(self.certificate, identifier), compiled.read_bytes())

    def test_generated_material_is_available_until_verification_then_removed(self):
        for platform in ('linux', 'darwin', 'win32'):
            for fail in (False, True):
                with self.subTest(platform=platform, fail=fail), \
                     patch.dict(os.environ, {}, clear=True), \
                     patch.object(signing.sys, 'platform', platform), \
                     patch.object(signing, 'tool', return_value=['rcodesign']), \
                     patch.object(signing, 'credentials', return_value=nullcontext(['--p12-file', 'fixture'])), \
                     patch.object(signing, 'check_identity', return_value=(self.certificate, True)):
                    material = []

                    def run(command, **kwargs):
                        if command[0] == 'rcodesign':
                            requirement = Path(command[command.index('--code-requirements-file') + 1])
                            material.append(requirement.parent)
                            self.assertTrue(requirement.is_file())
                            self.assertEqual((requirement.parent / 'certificate.der').read_bytes(), self.certificate)
                            if fail:
                                raise subprocess.CalledProcessError(1, 'rcodesign')
                        else:
                            self.assertEqual(command[0], '/usr/bin/codesign')
                            self.assertTrue(Path(command[command.index('-R') + 1]).is_file())
                        return subprocess.CompletedProcess(command, 0)

                    with patch.object(signing.subprocess, 'run', side_effect=run) as called:
                        if fail:
                            with self.assertRaises(subprocess.CalledProcessError):
                                signing.sign_bundle(Path('fixture.app'))
                        else:
                            self.assertTrue(signing.sign_bundle(Path('fixture.app')))
                            self.assertEqual(called.call_count, 2 if platform == 'darwin' else 1)
                    self.assertEqual(len(material), 1)
                    self.assertFalse(material[0].exists())

    def test_automatic_trust_is_opt_in_and_restricted_to_mac_ci(self):
        for platform, github, enabled, allowed in (
                ('darwin', '', '', True), ('darwin', '', '1', False),
                ('linux', 'true', '1', False), ('darwin', 'true', '1', True)):
            with self.subTest(platform=platform, github=github, enabled=enabled), \
                 patch.object(signing.sys, 'platform', platform), \
                 patch.dict(os.environ, {'GITHUB_ACTIONS': github,
                                         'KEYSTEER_TRUST_SIGNING_CERTIFICATE': enabled}, clear=True), \
                 patch.object(signing.subprocess, 'run') as run:
                if allowed:
                    signing.trust_ci_certificate(Path('certificate.der'))
                    self.assertEqual(run.call_count, 1 if enabled else 0)
                else:
                    with self.assertRaisesRegex(ValueError, 'restricted'):
                        signing.trust_ci_certificate(Path('certificate.der'))
                    run.assert_not_called()

    def test_restore_rejects_unexpected_archive_entries_and_never_overwrites_keys(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / 'backup.zip'
            with zipfile.ZipFile(archive, 'w') as zipped:
                zipped.writestr('../escape', b'bad')
            with self.assertRaisesRegex(ValueError, 'Unexpected'):
                signing.import_backup(archive, root / 'new', 'password')
            self.assertFalse((root / 'new').exists())
            installed = root / 'installed'
            installed.mkdir()
            (installed / 'identity.p12').write_bytes(b'existing')
            with self.assertRaisesRegex(ValueError, 'refusing to replace'):
                signing.import_backup(archive, installed, 'password')
            self.assertEqual((installed / 'identity.p12').read_bytes(), b'existing')

    def test_restore_checks_integrity_and_password_before_creating_destination(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / 'backup.zip'
            with zipfile.ZipFile(archive, 'w') as zipped:
                zipped.writestr('identity.p12', b'fixture')
                zipped.writestr('certificate.der', self.certificate)
                zipped.writestr('manifest.json', json.dumps({'p12_sha256': hashlib.sha256(b'fixture').hexdigest()}))
            with patch.object(signing, 'check_identity', side_effect=ValueError('wrong password')):
                with self.assertRaisesRegex(ValueError, 'wrong password'):
                    signing.import_backup(archive, root / 'new', 'wrong')
            self.assertFalse((root / 'new').exists())


if __name__ == '__main__':
    unittest.main()
