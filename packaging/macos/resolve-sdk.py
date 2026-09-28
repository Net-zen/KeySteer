#!/usr/bin/env python3
"""Select the latest SDK in the community archive used by setup-osxcross."""
import json
import os
import re
import subprocess
from urllib.parse import urlparse


def select_sdk(entries):
    candidates = []
    for entry in entries:
        match = re.fullmatch(r'macOS (\d+(?:\.\d+)+)', entry.get('version', ''))
        if not match or not {'x86_64', 'arm64'}.issubset(entry.get('architectures', [])):
            continue
        version = tuple(map(int, match[1].split('.')))
        if version[0] < 14:
            continue
        url = entry['github_download_url']
        expected = f'https://github.com/joseluisq/macosx-sdks/releases/download/{match[1]}/MacOSX{match[1]}.sdk.tar.xz'
        if url != expected:
            raise ValueError('Unexpected SDK archive URL in upstream manifest')
        digest = entry['github_download_sha256sum']
        if not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('SDK archive is missing a SHA256 digest')
        candidates.append((version, url, digest, match[1]))
    if not candidates:
        raise ValueError('No macOS 14+ SDK with both target architectures was found')
    _, url, digest, tag = max(candidates)
    return url, digest, tag


def main():
    url = os.environ.get('MACOS_SDK_URL', '')
    digest = os.environ.get('MACOS_SDK_SHA256', '')
    tag = ''
    if url or digest:
        if urlparse(url).scheme != 'https' or not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('Custom SDK requires both an HTTPS MACOS_SDK_URL and MACOS_SDK_SHA256')
    else:
        manifest = subprocess.check_output([
            'gh', 'api', 'repos/joseluisq/macosx-sdks/contents/macosx_sdks.json',
            '-H', 'Accept: application/vnd.github.raw+json',
        ], text=True)
        url, digest, tag = select_sdk(json.loads(manifest))
        print(f'Using macOS {tag} SDK from community archive joseluisq/macosx-sdks (not Apple-hosted)')
    if any(c in url for c in '\r\n'):
        raise ValueError('SDK URL cannot contain newlines')
    with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
        output.write(f'url={url}\nsha256={digest}\ntag={tag}\n')


if __name__ == '__main__':
    main()
