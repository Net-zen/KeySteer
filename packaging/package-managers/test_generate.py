import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("generate", Path(__file__).with_name("generate.py"))
generator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generator)


class PackageManagerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        targets = [
            "aarch64-apple-darwin", "x86_64-apple-darwin",
            "aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc",
            "x86_64-pc-windows-msvc-compatible",
            "x86_64-pc-windows-msvc-avx512",
        ]
        self.release = {"tag_name": "v1.2.3", "draft": False, "prerelease": False, "assets": []}
        for index, target in enumerate(targets):
            name = f"KeySteer-v1.2.3-{target}.zip"
            self.release["assets"].append({
                "name": name,
                "size": 100,
                "digest": "sha256:" + str(index) * 64,
                "browser_download_url": f"{generator.REPOSITORY}/releases/download/v1.2.3/{name}",
            })

    def manifest(self):
        return json.loads((self.root / "bucket/keysteer.json").read_text(encoding="utf-8"))

    def test_architecture_selection_and_layout(self):
        generator.generate(self.release, self.root)
        manifest = self.manifest()
        self.assertTrue(manifest["architecture"]["64bit"]["url"].endswith("-x86_64-pc-windows-msvc.zip"))
        self.assertEqual(manifest["architecture"]["64bit"]["hash"], "3" * 64)
        self.assertTrue(manifest["architecture"]["arm64"]["url"].endswith("aarch64-pc-windows-msvc.zip"))
        self.assertEqual(manifest["extract_dir"], "KeySteer")
        self.assertEqual(manifest["bin"], [["KeySteer.exe", "keysteer"]])
        cask = (self.root / "Casks/keysteer.rb").read_text()
        self.assertIn('app "KeySteer/KeySteer.app"', cask)
        self.assertIn('arm:   "' + "0" * 64 + '"', cask)
        self.assertIn('intel: "' + "1" * 64 + '"', cask)

    def test_native_release_without_compatible_package(self):
        self.release["assets"] = [asset for asset in self.release["assets"] if not asset["name"].endswith(("-compatible.zip", "-avx512.zip"))]
        generator.generate(self.release, self.root)
        manifest = self.manifest()
        self.assertTrue(manifest["architecture"]["64bit"]["url"].endswith("x86_64-pc-windows-msvc.zip"))

    def test_invalid_release_does_not_change_existing_manifests(self):
        generator.generate(self.release, self.root)
        before = (self.root / "bucket/keysteer.json").read_bytes()
        for field in ("draft", "prerelease"):
            with self.subTest(field=field):
                self.release[field] = True
                with self.assertRaises(ValueError):
                    generator.generate(self.release, self.root)
                self.release[field] = False
        self.release["assets"].pop(0)
        with self.assertRaises(KeyError):
            generator.generate(self.release, self.root)
        self.assertEqual((self.root / "bucket/keysteer.json").read_bytes(), before)

    def test_invalid_digest_and_untrusted_url_are_rejected(self):
        asset = self.release["assets"][0]
        asset["digest"] = "sha256:not-a-checksum"
        with self.assertRaises(ValueError):
            generator.generate(self.release, self.root)
        asset["digest"] = "sha256:" + "0" * 64
        asset["browser_download_url"] = "https://example.org/keysteer.zip"
        with self.assertRaises(ValueError):
            generator.generate(self.release, self.root)

    def test_downloaded_checksums_and_missing_api_digest(self):
        assets_dir = self.root / "assets"
        assets_dir.mkdir()
        for asset in self.release["assets"]:
            content = asset["name"].encode()
            (assets_dir / asset["name"]).write_bytes(content)
            asset["digest"] = "sha256:" + hashlib.sha256(content).hexdigest()
        self.release["assets"][0]["digest"] = None
        with self.assertRaises(ValueError):
            generator.generate(self.release, self.root)
        generator.generate(self.release, self.root, assets_dir)
        self.release["assets"][1]["digest"] = "sha256:" + "0" * 64
        with self.assertRaises(ValueError):
            generator.generate(self.release, self.root, assets_dir)

    def test_scoop_data_survives_atomic_saves_and_upgrade(self):
        import shutil
        shell = shutil.which("pwsh") or shutil.which("powershell")
        if not shell:
            self.skipTest("PowerShell is required for Scoop lifecycle verification")
        generator.generate(self.release, self.root)
        manifest = self.manifest()
        script = self.root / "lifecycle.ps1"
        script.write_text("\n".join([
            "$ErrorActionPreference = 'Stop'",
            "$dir = Join-Path $PSScriptRoot 'old'",
            "$persist_dir = Join-Path $PSScriptRoot 'persist'",
            "New-Item -ItemType Directory -Path $dir | Out-Null",
            "Set-Content (Join-Path $dir 'keysteer.default.toml') 'old shipped defaults'",
            "Set-Content (Join-Path $dir 'keysteer.custom.toml') 'custom configuration'",
            "Set-Content -LiteralPath (Join-Path $dir 'keysteer.[custom].toml') 'bracket configuration'",
            "Set-Content (Join-Path $dir 'keysteer.user.toml') 'initial user settings'",
            "Set-Content (Join-Path $dir 'saved.tmp') 'atomically saved user settings'",
            "Move-Item (Join-Path $dir 'saved.tmp') (Join-Path $dir 'keysteer.user.toml') -Force",
            "Set-Content (Join-Path $dir 'workspace.ksw') 'saved layouts'",
            "Set-Content (Join-Path $dir 'KeySteer.exe') 'old executable'",
            *manifest["pre_uninstall"],
            "$dir = Join-Path $PSScriptRoot 'new'",
            "New-Item -ItemType Directory -Path $dir | Out-Null",
            "Set-Content (Join-Path $dir 'keysteer.default.toml') 'new shipped defaults'",
            "Set-Content (Join-Path $dir 'KeySteer.exe') 'new executable'",
            *manifest["pre_install"],
            "if ((Get-Content (Join-Path $dir 'keysteer.user.toml')) -ne 'atomically saved user settings') { throw 'User configuration lost' }",
            "if ((Get-Content (Join-Path $dir 'keysteer.custom.toml')) -ne 'custom configuration') { throw 'Custom configuration lost' }",
            "if ((Get-Content -LiteralPath (Join-Path $dir 'keysteer.[custom].toml')) -ne 'bracket configuration') { throw 'Bracket configuration lost' }",
            "if ((Get-Content (Join-Path $dir 'workspace.ksw')) -ne 'saved layouts') { throw 'Layouts lost' }",
            "if ((Get-Content (Join-Path $dir 'keysteer.default.toml')) -ne 'new shipped defaults') { throw 'Stale defaults' }",
            "if ((Get-Content (Join-Path $dir 'KeySteer.exe')) -ne 'new executable') { throw 'Stale executable' }",
            # A deleted config must stay deleted on subsequent upgrades.
            "Remove-Item (Join-Path $dir 'keysteer.custom.toml')",
            *manifest["pre_uninstall"],
            "if (Test-Path (Join-Path $persist_dir 'data/keysteer.custom.toml')) { throw 'Deleted configuration resurrected' }",
            "function Get-Process { [pscustomobject]@{ Path = (Join-Path (Split-Path -Parent $dir) 'current/KeySteer.exe') } }",
            "$blocked = $false",
            "try {",
            *manifest["pre_uninstall"],
            "} catch { $blocked = $_.Exception.Message -eq 'Exit KeySteer before updating or uninstalling it.' }",
            "if (-not $blocked) { throw 'Running application was not detected' }",
        ]), encoding="utf-8")
        subprocess.run([shell, "-NoProfile", "-File", str(script)], check=True, capture_output=True)


if __name__ == "__main__":
    unittest.main()
