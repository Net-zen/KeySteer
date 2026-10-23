#!/usr/bin/env python3
"""One portable signing identity for local builds and both CI build modes."""
import argparse
import base64
from contextlib import contextmanager, nullcontext
import getpass
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile


# Public pin only: reject accidental signer rotation without storing certificate
# or compiled requirement files in the repository. Keep the existing identity.
CERTIFICATE_SHA256 = '1cda7c1ed762d3dd5d80dbd03c25edd371c562e13f0b0d7ff960c63c291ab449'
BUNDLE_IDENTIFIER = 'com.keysteer.app'


def signing_directory():
    override = os.environ.get('KEYSTEER_SIGNING_DIR')
    if override:
        return Path(override).expanduser()
    if sys.platform == 'darwin':
        return Path.home() / 'Library/Application Support/KeySteer/Signing'
    if sys.platform == 'win32':
        return Path(os.environ['APPDATA']) / 'KeySteer/Signing'
    return Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'KeySteer/Signing'


def tool():
    executable = shutil.which('rcodesign')
    cached = Path.home() / '.cache/keysteer-rcodesign' / ('rcodesign.exe' if os.name == 'nt' else 'rcodesign')
    if not executable and cached.is_file():
        executable = str(cached)
    if not executable:
        raise ValueError('rcodesign is required; use the setup-rcodesign action or install the official tool')
    # Do not inherit unrelated user rcodesign profiles or signing settings.
    return [executable, '-C', os.devnull]


def private_write(path, data):
    with path.open('xb') as output:
        path.chmod(0o600)
        output.write(data)


@contextmanager
def credentials():
    encoded = os.environ.get('MACOS_SIGNING_P12_BASE64', '').strip()
    password = os.environ.get('MACOS_SIGNING_PASSWORD', '')
    explicit = os.environ.get('MACOS_SIGNING_P12_FILE', '')
    password_file = os.environ.get('MACOS_SIGNING_PASSWORD_FILE', '')
    if encoded and explicit:
        raise ValueError('Use one P12 source: BASE64 or FILE')
    with tempfile.TemporaryDirectory(prefix='keysteer-macos-sign-') as temporary:
        root = Path(temporary)
        if encoded:
            p12 = root / 'identity.p12'
            private_write(p12, base64.b64decode(encoded, validate=True))
        elif explicit:
            p12 = Path(explicit)
        elif os.environ.get('GITHUB_ACTIONS') != 'true' and (signing_directory() / 'identity.p12').is_file():
            p12 = signing_directory() / 'identity.p12'
            password_file = password_file or str(signing_directory() / 'MACOS_SIGNING_PASSWORD.txt')
        else:
            if password or password_file or os.environ.get('KEYSTEER_REQUIRE_MACOS_SIGNING') == '1':
                raise ValueError('MACOS_SIGNING_P12_BASE64 and MACOS_SIGNING_PASSWORD are required; refusing an ad-hoc release')
            yield None
            return
        if password_file:
            pwd = Path(password_file)
        elif password:
            pwd = root / 'password'
            private_write(pwd, (password + '\n').encode())
        else:
            raise ValueError('The configured P12 requires a password file or MACOS_SIGNING_PASSWORD')
        if not p12.is_file() or not pwd.is_file():
            raise ValueError('Signing P12 or password file does not exist')
        yield ['--p12-file', str(p12), '--p12-password-file', str(pwd)]


def check_identity(arguments):
    result = subprocess.run([*tool(), 'analyze-certificate', *arguments],
                            capture_output=True, text=True)
    if result.returncode:
        raise ValueError('Cannot unlock the signing P12; check its format and password')
    # rcodesign reports the signing leaf first, followed by any CA chain. Read
    # only its public PEM; never export or print the P12's private key.
    pem = re.search(r'-----BEGIN CERTIFICATE-----\s*(.*?)\s*-----END CERTIFICATE-----',
                    result.stdout, re.S)
    if not pem:
        raise ValueError('Signing tool did not return a public certificate')
    certificate = base64.b64decode(''.join(pem[1].split()), validate=True)
    if hashlib.sha256(certificate).hexdigest() != CERTIFICATE_SHA256:
        raise ValueError('Signing certificate differs from the fixed KeySteer identity; refusing to change update identity')
    self_signed = re.search(r'^Subject is Issuer\?:\s*(true|false)\s*$', result.stdout, re.M)
    if not self_signed:
        raise ValueError('Signing tool did not identify the certificate issuer')
    return certificate, self_signed[1] == 'true'


def code_requirement(certificate, identifier=BUNDLE_IDENTIFIER):
    """Encode only `identifier ... and certificate leaf = H"..."`, portably."""
    # Apple's stable Requirement exprForm: big-endian words, length-prefixed
    # data padded to four bytes; opAnd=6, opIdent=2, opAnchorHash=4, leaf slot=0.
    # https://github.com/apple-oss-distributions/Security/blob/main/OSX/libsecurity_codesigning/lib/requirement.h
    # https://github.com/apple-oss-distributions/Security/blob/main/OSX/libsecurity_codesigning/lib/reqmaker.cpp
    def data(value):
        return struct.pack('>I', len(value)) + value + bytes(-len(value) % 4)
    expression = (struct.pack('>II', 6, 2) + data(identifier.encode('utf-8'))
                  + struct.pack('>II', 4, 0) + data(hashlib.sha1(certificate).digest()))
    return struct.pack('>III', 0xfade0c00, 12 + len(expression), 1) + expression


def trust_ci_certificate(certificate):
    if os.environ.get('KEYSTEER_TRUST_SIGNING_CERTIFICATE') != '1':
        return
    if sys.platform != 'darwin' or os.environ.get('GITHUB_ACTIONS') != 'true':
        raise ValueError('Automatic certificate trust is restricted to macOS GitHub Actions runners')
    subprocess.run(['sudo', '-n', '/usr/bin/security', 'add-trusted-cert', '-d',
                    '-r', 'trustRoot', '-p', 'codeSign', '-k',
                    '/Library/Keychains/System.keychain', str(certificate)], check=True)


def sign_bundle(app, *, notarize=False):
    identity = os.environ.get('KEYSTEER_CODESIGN_IDENTITY', '')
    keychain = bool(identity and identity != '-')
    if keychain and sys.platform != 'darwin':
        raise ValueError('Keychain identities require macOS; use a P12 on other systems')
    source = (nullcontext(['--keychain-domain', 'dynamic', '--keychain-fingerprint', CERTIFICATE_SHA256])
              if keychain else credentials())
    with source as arguments, tempfile.TemporaryDirectory(prefix='keysteer-signing-material-') as temporary:
        root = Path(temporary)
        requirement = root / 'requirement.bin'
        command = [*tool(), 'sign']
        if arguments:
            certificate, self_signed = check_identity(arguments)
            if self_signed and notarize:
                raise ValueError('Apple notarization requires an Apple-issued Developer ID certificate')
            public = root / 'certificate.der'
            private_write(public, certificate)
            private_write(requirement, code_requirement(certificate))
            if self_signed:
                trust_ci_certificate(public)
            command += [*arguments, '--code-signature-flags', 'runtime',
                        '--code-requirements-file', str(requirement)]
            if self_signed:
                command += ['--timestamp-url', 'none']
            if notarize:
                command += ['--for-notarization']
            if keychain:
                # Native codesign embeds a requirement set, whereas rcodesign
                # above takes a single compiled designated requirement.
                designated = (f'=designated => identifier "{BUNDLE_IDENTIFIER}" and '
                              f'certificate leaf = H"{hashlib.sha1(certificate).hexdigest()}"')
                command = ['/usr/bin/codesign', '--force', '--sign', identity,
                           '--options', 'runtime', '--requirements', designated,
                           '--timestamp=none' if self_signed else '--timestamp']
        elif notarize:
            raise ValueError('Notarization requires a signing certificate')
        subprocess.run([*command, str(app)], check=True)
        if sys.platform == 'darwin':
            verify = ['/usr/bin/codesign', '--verify', '--deep', '--strict']
            if arguments:
                verify += ['-R', str(requirement)]
            subprocess.run([*verify, str(app)], check=True)
        return arguments is not None


def import_backup(archive, destination, password):
    """Import existing keys without creating or rotating an identity."""
    if destination.exists() and any(destination.iterdir()):
        raise ValueError('The signing directory is not empty; refusing to replace an existing identity')
    with zipfile.ZipFile(archive) as zipped:
        expected = {'identity.p12', 'certificate.der', 'manifest.json'}
        if len(zipped.infolist()) != 3 or set(zipped.namelist()) != expected:
            raise ValueError('Unexpected signing backup contents')
        if any(i.file_size > 65536 for i in zipped.infolist()):
            raise ValueError('Signing backup entry is too large')
        content = {name: zipped.read(name) for name in expected}
    if hashlib.sha256(content['certificate.der']).hexdigest() != CERTIFICATE_SHA256:
        raise ValueError('Backup certificate does not match this repository signing identity')
    manifest = json.loads(content['manifest.json'])
    if hashlib.sha256(content['identity.p12']).hexdigest() != manifest['p12_sha256']:
        raise ValueError('Signing backup integrity check failed')
    with tempfile.TemporaryDirectory(prefix='keysteer-signing-import-') as temporary:
        root = Path(temporary)
        private_write(root / 'identity.p12', content['identity.p12'])
        private_write(root / 'password', (password + '\n').encode())
        certificate, _ = check_identity(['--p12-file', str(root / 'identity.p12'), '--p12-password-file', str(root / 'password')])
        if certificate != content['certificate.der']:
            raise ValueError('Backup certificate does not match its P12 signing identity')
    destination.mkdir(parents=True, exist_ok=True, mode=0o700)
    destination.chmod(0o700)
    private_write(destination / 'identity.p12', content['identity.p12'])
    private_write(destination / 'certificate.der', content['certificate.der'])
    private_write(destination / 'MACOS_SIGNING_PASSWORD.txt', (password + '\n').encode())
    private_write(destination / 'MACOS_SIGNING_P12_BASE64.txt', base64.b64encode(content['identity.p12']) + b'\n')
    print(f'Imported the existing signing identity into {destination}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    sign = commands.add_parser('sign')
    sign.add_argument('app', type=Path)
    restore = commands.add_parser('import-backup')
    restore.add_argument('archive', type=Path)
    restore.add_argument('--directory', type=Path, default=signing_directory())
    restore.add_argument('--password-file', type=Path)
    args = parser.parse_args()
    if args.command == 'sign':
        sign_bundle(args.app)
    else:
        password = args.password_file.read_text().rstrip('\r\n') if args.password_file else getpass.getpass('Signing backup password: ')
        import_backup(args.archive, args.directory, password)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from None
