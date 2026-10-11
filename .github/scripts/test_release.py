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


release_tag = load("release_tag")
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


class CleanupEntryTests(unittest.TestCase):
    def setUp(self):
        self.run = {
            "id": 123, "path": ".github/workflows/start.yml", "event": "push",
            "head_sha": "a" * 40, "status": "completed", "conclusion": "success",
        }
        self.stages = {name: {"result": "success"} for name in (
            "source", "windows", "macos", "release", "package-managers", "pages",
        )}

    def test_only_corresponding_successful_entry_is_deleted(self):
        with patch.object(cleanup_entry, "api", side_effect=[self.run, None]) as api:
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

    def test_already_deleted_entry_is_safe_on_rerun(self):
        with patch.object(cleanup_entry, "api", return_value=None) as api:
            cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, self.stages)
            self.assertEqual(api.call_count, 1)

    def test_any_failed_cancelled_or_unfinished_stage_preserves_entry(self):
        for stage in self.stages:
            for result in ("failure", "cancelled", "queued"):
                with self.subTest(stage=stage, result=result), patch.object(cleanup_entry, "api") as api:
                    stages = {**self.stages, stage: {"result": result}}
                    cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, stages)
                    api.assert_not_called()

    def test_partial_build_only_run_can_clean_entry_after_pages(self):
        stages = {**self.stages, **{name: {"result": "skipped"} for name in (
            "macos", "release", "package-managers",
        )}}
        with patch.object(cleanup_entry, "api", side_effect=[self.run, None]) as api:
            cleanup_entry.cleanup("dccif/KeySteer", "123", "456", "a" * 40, stages)
            self.assertEqual(api.call_count, 2)

    def test_skipped_release_dependency_or_missing_results_preserve_entry(self):
        for stages in ({}, {**self.stages, "package-managers": {"result": "skipped"}}):
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
        for outcome, expected_calls in (("success", 2), ("failure", 0)):
            with self.subTest(outcome=outcome), patch.dict(cleanup_entry.os.environ, {**environment, "PAGES_RESULT": outcome}), patch.object(cleanup_entry, "api", side_effect=[self.run, None]) as api:
                cleanup_entry.main()
                self.assertEqual(api.call_count, expected_calls)


if __name__ == "__main__":
    unittest.main()
