from __future__ import annotations

import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
UPLOAD_ARTIFACT_ACTION = (
    "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"
)
PAYLOAD_WORKFLOWS = (
    REPOSITORY / ".github/workflows/payload-ci.yml",
    REPOSITORY / ".github/workflows/build-linux-payload-ci.yml",
    REPOSITORY / ".github/workflows/build-windows-payload-ci.yml",
)
CLEANUP_WORKFLOW = REPOSITORY / ".github/workflows/cleanup-actions-artifacts.yml"


def upload_blocks(workflow: str) -> list[str]:
    blocks: list[str] = []
    lines = workflow.splitlines()
    for index, line in enumerate(lines):
        if f"uses: {UPLOAD_ARTIFACT_ACTION}" not in line:
            continue
        end = index + 1
        while end < len(lines) and not lines[end].startswith("      - name:"):
            end += 1
        blocks.append("\n".join(lines[index:end]))
    return blocks


class CiArtifactLifecycleTests(unittest.TestCase):
    def test_payload_artifacts_expire_after_one_day(self) -> None:
        blocks = [
            block
            for workflow in PAYLOAD_WORKFLOWS
            for block in upload_blocks(workflow.read_text(encoding="utf-8"))
        ]

        self.assertEqual(len(blocks), 4)
        for block in blocks:
            with self.subTest(block=block):
                self.assertIn("retention-days: 1", block)

    def test_completed_workflows_delete_only_their_run_artifacts(self) -> None:
        workflow = CLEANUP_WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("workflow_run:", workflow)
        self.assertIn("      - Release", workflow)
        self.assertIn("      - Verify dual-platform Skill payload", workflow)
        self.assertIn("      - completed", workflow)
        self.assertIn("permissions:\n  actions: write", workflow)
        self.assertNotIn("actions/checkout", workflow)
        self.assertIn(
            "RUN_ID: ${{ github.event.workflow_run.id }}",
            workflow,
        )
        self.assertIn(
            "/actions/runs/${RUN_ID}/artifacts",
            workflow,
        )
        self.assertIn("gh api --method DELETE", workflow)
        self.assertIn(
            "/actions/artifacts/${artifact_id}",
            workflow,
        )


if __name__ == "__main__":
    unittest.main()
