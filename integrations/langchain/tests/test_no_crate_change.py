"""Slice F: the honest test of `00j-language-boundaries.md`'s claim.

"Nothing in this epic may require a change to a graph-owl crate" — asserted
structurally, not by review. Checks two things: everything not yet
committed (the fast, always-on local signal) and, when
``GRAPH_OWL_STRUCTURAL_CHECK_BASE`` names a ref (CI sets it to the PR's
base branch), everything committed since that ref too. If this epic ever
needed a crate change, per the plan's own words: "the boundary was drawn
wrong and the document should be amended rather than the test relaxed."
"""

import os
import subprocess

REPO_ROOT = subprocess.run(
    ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True
).stdout.strip()


def _changed_paths(*args: str) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--name-only", *args], capture_output=True, text=True, cwd=REPO_ROOT
    )
    if result.returncode != 0:
        return []
    return [line for line in result.stdout.splitlines() if line]


def _untracked_paths() -> list[str]:
    result = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=all"],
        capture_output=True,
        text=True,
        cwd=REPO_ROOT,
        check=True,
    )
    return [line[3:] for line in result.stdout.splitlines() if line]


def test_uncommitted_changes_touch_no_graph_owl_crate():
    changed = _changed_paths("HEAD") + _untracked_paths()
    crate_changes = [path for path in changed if path.startswith("crates/")]
    assert not crate_changes, (
        f"this epic's own rule: friction here is an API or MCP defect, "
        f"logged against Epic 14, not a crate change — but these changed: "
        f"{crate_changes}"
    )


def test_committed_history_since_the_base_touches_no_graph_owl_crate():
    base = os.environ.get("GRAPH_OWL_STRUCTURAL_CHECK_BASE")
    if not base:
        import pytest

        pytest.skip("set GRAPH_OWL_STRUCTURAL_CHECK_BASE to check committed history too")
    changed = _changed_paths(f"{base}...HEAD")
    crate_changes = [path for path in changed if path.startswith("crates/")]
    assert not crate_changes, f"changes since {base} touched: {crate_changes}"
