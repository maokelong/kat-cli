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


if __name__ == "__main__":
    unittest.main()
