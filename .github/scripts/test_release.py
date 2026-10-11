import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(f"{name}.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


release_tag = load("release")
cleanup_entry = load("cleanup_entry")


class ReleaseTagTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.repo = root / "source"
        remote = root / "remote.git"
        self.git("init", "--bare", str(remote), cwd=root)
        self.git("init", str(self.repo), cwd=root)
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.org")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")
        self.git("remote", "add", "origin", str(remote))
        self.git("commit", "--allow-empty", "-m", "Initial")
        self.sha = self.git("rev-parse", "HEAD")
        self.git("push", "origin", "HEAD:refs/heads/main")

    def git(self, *args, cwd=None):
        return subprocess.run(
            ["git", *args], cwd=cwd or self.repo, check=True,
            capture_output=True, text=True,
        ).stdout.strip()

    def test_missing_tag_is_created(self):
        release_tag.ensure_tag("v1.2.3", self.sha, self.repo)
        self.assertTrue(self.git("ls-remote", "origin", "refs/tags/v1.2.3").startswith(self.sha))

    def test_existing_annotated_tag_object_is_preserved(self):
        self.git("tag", "-a", "v1.2.3", "-m", "Release annotation")
        self.git("push", "origin", "refs/tags/v1.2.3")
        before = self.git("ls-remote", "origin", "refs/tags/v1.2.3*")
        release_tag.ensure_tag("v1.2.3", self.sha, self.repo)
        self.assertEqual(self.git("ls-remote", "origin", "refs/tags/v1.2.3*"), before)

    def test_mismatched_tag_is_rejected_without_modification(self):
        self.git("tag", "v1.2.3")
        self.git("push", "origin", "refs/tags/v1.2.3")
        before = self.git("ls-remote", "origin", "refs/tags/v1.2.3")
        self.git("commit", "--allow-empty", "-m", "New commit")
        with self.assertRaises(ValueError):
            release_tag.ensure_tag("v1.2.3", self.git("rev-parse", "HEAD"), self.repo)
        self.assertEqual(self.git("ls-remote", "origin", "refs/tags/v1.2.3"), before)


class PublishTests(unittest.TestCase):
    def test_release_names_and_latest_policy(self):
        identity = release_tag.release_identity
        self.assertEqual(identity("1.2.3", "", "release", "cross", "7"),
                         ("v1.2.3", "KeySteer v1.2.3", False, ["--latest"]))
        self.assertEqual(identity("1.2.3", "", "pre-release", "native", "7"),
                         ("v1.2.3-pre.7", "KeySteer v1.2.3 pre-release #7", True, ["--latest=false"]))
        self.assertEqual(identity("1.2.3", "v1.2.3-beta", "release", "cross", "7"),
                         ("v1.2.3-beta", "KeySteer v1.2.3-beta", True, ["--latest=false"]))
        self.assertEqual(identity("1.2.3", "", "release", "native", "7")[-1], [])

    def test_incomplete_unexpected_and_empty_archives_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            targets = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
                       "aarch64-apple-darwin", "x86_64-apple-darwin"]
            for target in targets:
                (directory / f"KeySteer-v1.2.3-{target}.zip").write_bytes(b"archive")
            self.assertEqual(len(release_tag.verify_assets(directory, "1.2.3", "native")), 4)
            with self.assertRaises(ValueError):
                release_tag.verify_assets(directory, "1.2.3", "cross")
            for suffix in ("compatible", "avx512"):
                (directory / f"KeySteer-v1.2.3-x86_64-pc-windows-msvc-{suffix}.zip").write_bytes(b"archive")
            self.assertEqual(len(release_tag.verify_assets(directory, "1.2.3", "cross")), 6)
            for filename in ("unexpected.zip", "checksum.sha256"):
                path = directory / filename
                path.write_bytes(b"extra")
                with self.assertRaises(ValueError):
                    release_tag.verify_assets(directory, "1.2.3", "cross")
                path.unlink()
            next(directory.glob("*.zip")).write_bytes(b"")
            with self.assertRaises(ValueError):
                release_tag.verify_assets(directory, "1.2.3", "cross")

    def test_tag_mismatch_prevents_all_release_changes(self):
        with patch.object(release_tag, "ensure_tag", side_effect=ValueError("wrong commit")), patch.object(release_tag.subprocess, "run") as run:
            with self.assertRaises(ValueError):
                release_tag.publish_release("v1.2.3", "a" * 40, "Release", False, [], [], True)
            run.assert_not_called()

    def test_native_duplicate_is_preserved_and_cross_replaces_only_release(self):
        for replace in (False, True):
            with self.subTest(replace=replace), patch.object(release_tag, "ensure_tag"), patch.object(release_tag.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
                if replace:
                    release_tag.publish_release("v1.2.3", "a" * 40, "Release", False, ["--latest"], [Path("archive.zip")], True)
                    self.assertEqual(run.call_args_list[1].args[0], ["gh", "release", "delete", "v1.2.3", "--yes"])
                    self.assertIn("--verify-tag", run.call_args_list[2].args[0])
                    self.assertIn("archive.zip", run.call_args_list[2].args[0])
                else:
                    with self.assertRaises(ValueError):
                        release_tag.publish_release("v1.2.3", "a" * 40, "Release", False, [], [], False)
                    self.assertEqual(run.call_count, 1)


class CleanupEntryTests(unittest.TestCase):
    def setUp(self):
        self.run = {
            "id": 123, "path": ".github/workflows/build.yml", "event": "push",
            "head_sha": "a" * 40, "status": "completed", "conclusion": "success",
        }
        self.stages = {name: {"result": "success"} for name in (
            "source", "windows", "macos", "release", "pages",
        )}
        self.stages["source"]["outputs"] = {"build_mode": "cross"}
        for name in ("native-windows", "native-macos"):
            self.stages[name] = {"result": "skipped"}
        self.entry_jobs = {"total_count": 2, "jobs": [
            {"name": "Prepare", "conclusion": "success"},
            {"name": "Release", "conclusion": "skipped"},
        ]}

    def test_only_corresponding_successful_entry_is_deleted(self):
        with patch.object(cleanup_entry, "api", side_effect=[self.run, self.entry_jobs, None]) as api:
            cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, self.stages)
        self.assertEqual(api.call_args_list[-1].kwargs, {"method": "DELETE"})
        self.assertEqual(api.call_args_list[-1].args, ("dccif/KeySteer", "123"))

    def test_failed_unrelated_running_and_build_runs_are_preserved(self):
        for key, value in (
            ("id", 789), ("conclusion", "failure"), ("status", "in_progress"),
            ("path", ".github/workflows/cross-build.yml"), ("head_sha", "b" * 40),
            ("event", "workflow_run"),
        ):
            with self.subTest(key=key), patch.object(cleanup_entry, "api", return_value={**self.run, key: value}) as api:
                with self.assertRaises(ValueError):
                    cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, self.stages)
                self.assertEqual(api.call_count, 1)

    def test_self_and_invalid_run_ids_are_rejected(self):
        for entry in ("456", "../456", "123?x=1"):
            with self.subTest(entry=entry), patch.object(cleanup_entry, "api") as api:
                with self.assertRaises(ValueError):
                    cleanup_entry.cleanup("dccif/KeySteer", entry, "456", "a" * 40, self.stages)
                api.assert_not_called()

    def test_same_workflow_actual_build_run_is_preserved(self):
        jobs = {"total_count": 2, "jobs": [
            {"name": "Prepare", "conclusion": "success"},
            {"name": "Cross Windows x64 avx2", "conclusion": "success"},
        ]}
        with patch.object(cleanup_entry, "api", side_effect=[self.run, jobs]) as api:
            with self.assertRaises(ValueError):
                cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, self.stages)
            self.assertEqual(api.call_count, 2)

    def test_native_success_and_cross_native_exclusion(self):
        stages = {**self.stages, "source": {"result": "success", "outputs": {"build_mode": "native"}}}
        for name in ("windows", "macos"):
            stages[name] = {"result": "skipped"}
        for name in ("native-windows", "native-macos"):
            stages[name] = {"result": "success"}
        self.assertTrue(cleanup_entry.stages_succeeded(stages))
        for name in ("windows", "macos"):
            self.assertFalse(cleanup_entry.stages_succeeded({**stages, name: {"result": "success"}}))
        self.assertFalse(cleanup_entry.stages_succeeded({**stages, "native-macos": {"result": "failure"}}))

    def test_already_deleted_entry_is_safe_on_rerun(self):
        with patch.object(cleanup_entry, "api", return_value=None) as api:
            cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, self.stages)
            self.assertEqual(api.call_count, 1)

    def test_any_failed_cancelled_or_unfinished_stage_preserves_entry(self):
        for stage in ("source", "windows", "macos", "release", "pages"):
            for result in ("failure", "cancelled", "queued"):
                with self.subTest(stage=stage, result=result), patch.object(cleanup_entry, "api") as api:
                    stages = {**self.stages, stage: {"result": result}}
                    cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, stages)
                    api.assert_not_called()

    def test_partial_build_only_run_can_clean_entry_after_pages(self):
        stages = {**self.stages, **{name: {"result": "skipped"} for name in (
            "macos", "release",
        )}}
        with patch.object(cleanup_entry, "api", side_effect=[self.run, self.entry_jobs, None]) as api:
            cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, stages)
            self.assertEqual(api.call_count, 3)

    def test_skipped_release_dependency_or_missing_results_preserve_entry(self):
        for stages in ({}, {**self.stages, "macos": {"result": "skipped"}}):
            with patch.object(cleanup_entry, "api") as api:
                cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, stages)
                api.assert_not_called()

    def test_pages_step_uses_actual_deployment_outcome(self):
        stages = {name: value for name, value in self.stages.items() if name != "pages"}
        environment = {
            "GITHUB_REPOSITORY": "dccif/KeySteer", "GITHUB_RUN_ID": "456",
            "ENTRY_RUN_ID": "123", "ENTRY_SHA": "a" * 40,
            "STAGE_RESULTS": json.dumps(stages),
        }
        for outcome, expected_calls in (("success", 3), ("failure", 0)):
            with self.subTest(outcome=outcome), patch.dict(cleanup_entry.os.environ, {**environment, "PAGES_RESULT": outcome}), patch.object(cleanup_entry, "api", side_effect=[self.run, self.entry_jobs, None]) as api:
                cleanup_entry.main()
                self.assertEqual(api.call_count, expected_calls)


if __name__ == "__main__":
    unittest.main()
