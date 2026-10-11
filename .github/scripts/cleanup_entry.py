"""Remove only this build's successful Start run after every stage succeeds."""

import json
import os
import re
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def api(repository, run_id, method="GET"):
    request = Request(
        f"https://api.github.com/repos/{repository}/actions/runs/{run_id}",
        headers={
            "Authorization": f"Bearer {os.environ['GH_TOKEN']}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
        },
        method=method,
    )
    try:
        with urlopen(request, timeout=30) as response:
            return json.load(response) if method == "GET" else None
    except HTTPError as error:
        if method == "GET" and error.code == 404:
            return None  # Already removed by a previous successful attempt.
        raise


def stages_succeeded(stages):
    required = ("source", "windows", "macos", "release", "package-managers", "pages")
    results = {name: stages.get(name, {}).get("result") for name in required}
    if results["source"] != "success" or results["pages"] != "success":
        return False
    builds = (results["windows"], results["macos"])
    if "success" not in builds or any(result not in ("success", "skipped") for result in builds):
        return False
    if results["release"] == "success":
        return builds == ("success", "success") and results["package-managers"] == "success"
    return results["release"] == "skipped" and results["package-managers"] == "skipped"


def cleanup(repository, entry_run_id, current_run_id, entry_sha, stages):
    if not stages_succeeded(stages):
        print("Keeping Start run: build, release, packages or Pages did not all succeed.")
        return
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Invalid repository")
    if not re.fullmatch(r"[0-9]+", entry_run_id) or entry_run_id == current_run_id:
        raise ValueError("Expected another workflow run ID")
    run = api(repository, entry_run_id)
    if run is None:
        print("Start run has already been removed.")
        return
    if (
        str(run["id"]) != entry_run_id
        or run["path"].split("@")[0] != ".github/workflows/start.yml"
        or run["event"] not in ("push", "workflow_dispatch")
        or run["head_sha"] != entry_sha
        or run["status"] != "completed"
        or run["conclusion"] != "success"
    ):
        raise ValueError("Refusing to remove a run that is not this build's successful Start entry")
    api(repository, entry_run_id, method="DELETE")
    print(f"Removed Start run {entry_run_id}; kept this build and the release tag.")


def main():
    stages = json.loads(os.environ["STAGE_RESULTS"])
    stages["pages"] = {"result": os.environ["PAGES_RESULT"]}
    cleanup(
        os.environ["GITHUB_REPOSITORY"], os.environ["ENTRY_RUN_ID"],
        os.environ["GITHUB_RUN_ID"], os.environ["ENTRY_SHA"],
        stages,
    )


if __name__ == "__main__":
    main()
