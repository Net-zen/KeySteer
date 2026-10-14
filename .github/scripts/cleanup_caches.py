"""Remove repository caches that have not been used within the retention window."""

from datetime import datetime, timedelta, timezone
import json
import os
import subprocess


def cleanup(repository, max_idle_days, now=None):
    if max_idle_days < 1:
        raise ValueError("CACHE_MAX_IDLE_DAYS must be a positive integer")
    cutoff = (now or datetime.now(timezone.utc)) - timedelta(days=max_idle_days)
    # Read all pages before deleting; deletion would otherwise shift later pages.
    response = subprocess.run(
        ["gh", "api", "--paginate", "--slurp",
         f"repos/{repository}/actions/caches?per_page=100"],
        check=True, stdout=subprocess.PIPE, text=True,
    )
    stale = [
        cache
        for page in json.loads(response.stdout)
        for cache in page["actions_caches"]
        if cache.get("last_accessed_at")
        and datetime.fromisoformat(cache["last_accessed_at"].replace("Z", "+00:00")) < cutoff
    ]
    for cache in stale:
        # IDs identify one cache, even when several refs share its key.
        subprocess.run(
            ["gh", "cache", "delete", str(cache["id"]), "--repo", repository],
            check=True,
        )
    print(f"Removed {len(stale)} caches unused for more than {max_idle_days} days.")
    return len(stale)


if __name__ == "__main__":
    cleanup(os.environ["GITHUB_REPOSITORY"], int(os.environ.get("CACHE_MAX_IDLE_DAYS", "7")))
