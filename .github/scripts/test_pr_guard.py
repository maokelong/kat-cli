#!/usr/bin/env python3
"""Unit tests for pr_guard.py."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

import pr_guard  # noqa: E402


def issue_context(*labels: str) -> pr_guard.PullRequestContext:
    return pr_guard.PullRequestContext(labels=set(labels), body="Closes #123")


class PrGuardTests(unittest.TestCase):
    def test_review_budget_thresholds_allow_moderate_ai_assisted_prs(self) -> None:
        self.assertEqual(pr_guard.MAX_CHANGED_FILES, 30)
        self.assertEqual(pr_guard.MAX_ADDITIONS, 1600)
        self.assertEqual(pr_guard.MAX_TOTAL_DIFF, 2400)
        self.assertEqual(pr_guard.MAX_SOURCE_FILE_ADDITIONS, 800)
        self.assertEqual(pr_guard.WARN_DOC_ADDITIONS, 800)

    def test_parse_numstat_parses_text_and_binary_files(self) -> None:
        stats = pr_guard.parse_numstat(
            "12\t3\tsrc/lib.rs\n"
            "-\t-\tassets/logo.png\n"
            "7\t0\tdocs/readme.md\n"
        )

        self.assertEqual(stats["src/lib.rs"], (12, 3, False))
        self.assertEqual(stats["assets/logo.png"], (0, 0, True))
        self.assertEqual(stats["docs/readme.md"], (7, 0, False))

    def test_has_linked_issue_accepts_closing_issue_urls_only(self) -> None:
        self.assertTrue(
            pr_guard.has_linked_issue(
                "Closes https://github.com/example/project/issues/42"
            )
        )
        self.assertTrue(
            pr_guard.has_linked_issue(
                "fixes https://github.com/example/project/issues/43"
            )
        )
        self.assertFalse(
            pr_guard.has_linked_issue(
                "Related https://github.com/example/project/issues/44"
            )
        )

    def test_missing_issue_fails_and_no_issue_needed_bypasses_it(self) -> None:
        changed_files = [pr_guard.ChangedFile(path="src/lib.rs", additions=1)]

        missing = pr_guard.evaluate(
            pr_guard.PullRequestContext(body="Related #123"),
            changed_files,
        )
        bypassed = pr_guard.evaluate(
            pr_guard.PullRequestContext(labels={pr_guard.NO_ISSUE_LABEL}, body=""),
            changed_files,
        )

        self.assertFalse(missing.ok)
        self.assertIn("Missing linked issue", "\n".join(missing.failures))
        self.assertTrue(bypassed.ok)

    def test_large_change_label_only_bypasses_size_failures(self) -> None:
        changed_files = [
            pr_guard.ChangedFile(
                path="src/lib.rs",
                additions=pr_guard.MAX_ADDITIONS + 1,
            ),
            pr_guard.ChangedFile(path="tmp/probe-output.txt", additions=1),
        ]

        without_label = pr_guard.evaluate(issue_context(), changed_files)
        with_label = pr_guard.evaluate(
            issue_context(pr_guard.LARGE_CHANGE_LABEL),
            changed_files,
        )

        self.assertIn("Additions", "\n".join(without_label.failures))
        self.assertIn("Temporary, probe, or AI intermediate", "\n".join(without_label.failures))
        self.assertNotIn("Additions", "\n".join(with_label.failures))
        self.assertIn("Temporary, probe, or AI intermediate", "\n".join(with_label.failures))

    def test_large_deletions_and_doc_additions_are_warnings(self) -> None:
        evaluation = pr_guard.evaluate(
            issue_context(pr_guard.LARGE_CHANGE_LABEL),
            [
                pr_guard.ChangedFile(
                    path="src/old.rs",
                    deletions=pr_guard.WARN_DELETIONS + 1,
                ),
                pr_guard.ChangedFile(
                    path="docs/guide.md",
                    additions=pr_guard.WARN_DOC_ADDITIONS + 1,
                ),
            ],
        )

        self.assertTrue(evaluation.ok)
        warnings = "\n".join(evaluation.warnings)
        self.assertIn("Deletions", warnings)
        self.assertIn("Document `docs/guide.md`", warnings)

    def test_binary_files_require_approval_label(self) -> None:
        changed_files = [
            pr_guard.ChangedFile(path="assets/build.bin", binary=True),
        ]

        without_label = pr_guard.evaluate(issue_context(), changed_files)
        with_label = pr_guard.evaluate(
            issue_context(pr_guard.BINARY_LABEL),
            changed_files,
        )

        self.assertFalse(without_label.ok)
        self.assertIn("Binary files are not allowed", "\n".join(without_label.failures))
        self.assertTrue(with_label.ok)

    def test_golden_paths_are_fixtures(self) -> None:
        self.assertTrue(pr_guard.is_fixture_file("tests/golden/output.snap"))

    def test_ai_path_component_detection_is_precise(self) -> None:
        self.assertFalse(pr_guard.is_forbidden_intermediate_path("src/ai/model.rs"))
        self.assertTrue(
            pr_guard.is_forbidden_intermediate_path("reports/ai-output/result.json")
        )

    def test_ai_agent_commit_author_or_committer_fails(self) -> None:
        changed_files = [pr_guard.ChangedFile(path="src/lib.rs", additions=1)]
        commits = [
            pr_guard.CommitIdentity(
                sha="abc1234",
                subject="feat: add query",
                author_name="Codex",
                author_email="codex@openai.com",
                committer_name="Alice",
                committer_email="alice@example.com",
            ),
            pr_guard.CommitIdentity(
                sha="def5678",
                subject="fix: wire table",
                author_name="Bob",
                author_email="bob@example.com",
                committer_name="Claude",
                committer_email="claude@example.com",
            ),
        ]

        evaluation = pr_guard.evaluate(
            issue_context(
                pr_guard.NO_ISSUE_LABEL,
                pr_guard.LARGE_CHANGE_LABEL,
            ),
            changed_files,
            commit_identities=commits,
        )

        self.assertFalse(evaluation.ok)
        failures = "\n".join(evaluation.failures)
        self.assertIn("AI agent commit identities are not allowed", failures)
        self.assertIn("abc1234 author Codex <codex@openai.com>", failures)
        self.assertIn("def5678 committer Claude <claude@example.com>", failures)

    def test_human_commit_identity_passes(self) -> None:
        evaluation = pr_guard.evaluate(
            issue_context(),
            [pr_guard.ChangedFile(path="src/lib.rs", additions=1)],
            commit_identities=[
                pr_guard.CommitIdentity(
                    sha="abc1234",
                    subject="feat: add query",
                    author_name="Alice",
                    author_email="alice@example.com",
                    committer_name="Bob",
                    committer_email="bob@example.com",
                ),
            ],
        )

        self.assertTrue(evaluation.ok)

    def test_generated_dump_paths_are_forbidden(self) -> None:
        self.assertTrue(pr_guard.is_forbidden_intermediate_path("generated/output.json"))
        self.assertTrue(pr_guard.is_forbidden_intermediate_path("reports/debug-dump.json"))
        self.assertTrue(
            pr_guard.is_forbidden_intermediate_path("fixtures/generated-dump.json")
        )

    def test_pull_request_trigger_detection(self) -> None:
        self.assertTrue(pr_guard.has_pull_request_trigger("on: pull_request\n"))
        self.assertFalse(pr_guard.has_pull_request_trigger("on: pull_request_target\n"))
        self.assertFalse(
            pr_guard.has_pull_request_trigger(
                "name: docs\nmessage: pull_request should be configured elsewhere\n"
            )
        )

    def test_workflow_disabled_detection(self) -> None:
        job_level = """
on: pull_request
jobs:
  test:
    if: false
    steps:
      - run: cargo test
"""
        step_level = """
on: pull_request
jobs:
  test:
    steps:
      - if: false
        run: cargo test
"""
        ignored_inline = """
on:
  pull_request:
    paths-ignore: ['**']
"""
        ignored_block = """
on:
  pull_request:
    branches-ignore:
      - '**'
"""

        self.assertIn("job:test:if:false", pr_guard.workflow_disabled_patterns(job_level))
        self.assertEqual(pr_guard.workflow_disabled_patterns(step_level), [])
        self.assertIn("paths-ignore:**", pr_guard.workflow_disabled_patterns(ignored_inline))
        self.assertIn("branches-ignore:**", pr_guard.workflow_disabled_patterns(ignored_block))

    def test_pr_guard_workflow_signals_are_required_when_present(self) -> None:
        old = """
on: pull_request
jobs:
  pr-guard:
    steps:
      - run: python .github/scripts/test_pr_guard.py
      - run: python .github/scripts/pr_guard.py
"""
        weakened = """
on: pull_request
jobs:
  pr-guard:
    steps:
      - run: echo skipped
"""
        missing_guard_only = """
on: pull_request
jobs:
  pr-guard:
    steps:
      - run: python .github/scripts/test_pr_guard.py
"""

        self.assertTrue(pr_guard.contains_required_workflow_signal(old, "pr guard tests"))
        self.assertTrue(pr_guard.contains_required_workflow_signal(old, "pr guard"))
        self.assertFalse(
            pr_guard.contains_required_workflow_signal(weakened, "pr guard tests")
        )
        self.assertFalse(pr_guard.contains_required_workflow_signal(weakened, "pr guard"))
        self.assertTrue(
            pr_guard.contains_required_workflow_signal(
                missing_guard_only, "pr guard tests"
            )
        )
        self.assertFalse(
            pr_guard.contains_required_workflow_signal(missing_guard_only, "pr guard")
        )

    def test_commit_identity_guard_workflow_signal_is_required_when_present(self) -> None:
        old = """
on: push
jobs:
  test:
    steps:
      - run: python .github/scripts/pr_guard.py --identity-only --base "$BASE" --head "$HEAD"
"""
        weakened = """
on: push
jobs:
  test:
    steps:
      - run: echo skipped
"""

        self.assertTrue(
            pr_guard.contains_required_workflow_signal(old, "commit identity guard")
        )
        self.assertFalse(pr_guard.contains_required_workflow_signal(old, "pr guard"))
        self.assertFalse(
            pr_guard.contains_required_workflow_signal(weakened, "commit identity guard")
        )

    def test_workflow_signal_can_move_to_another_workflow(self) -> None:
        old_ci = """
on:
  pull_request:
  push:
    branches:
      - main
jobs:
  test:
    steps:
      - run: python .github/scripts/pr_guard.py --identity-only --base "$BASE" --head "$HEAD"
      - run: cargo check --locked
      - run: cargo test --locked
"""
        fast_ci = """
on:
  pull_request:
  push:
    branches:
      - main
jobs:
  test:
    steps:
      - run: python .github/scripts/pr_guard.py --identity-only --base "$BASE" --head "$HEAD"
      - run: cargo check --locked
"""
        full_ci = """
on:
  pull_request:
    types: [labeled]
  workflow_dispatch:
jobs:
  full-test:
    steps:
      - run: cargo test --locked
"""

        failures = pr_guard.workflow_signal_failures(
            ".github/workflows/ci.yml",
            old_ci,
            fast_ci,
            [full_ci],
        )
        failures_without_replacement = pr_guard.workflow_signal_failures(
            ".github/workflows/ci.yml",
            old_ci,
            fast_ci,
            [],
        )

        self.assertNotIn(
            "CI workflow `.github/workflows/ci.yml` weakens required signal `cargo test`.",
            failures,
        )
        self.assertIn(
            "CI workflow `.github/workflows/ci.yml` weakens required signal `cargo test`.",
            failures_without_replacement,
        )


if __name__ == "__main__":
    unittest.main()
