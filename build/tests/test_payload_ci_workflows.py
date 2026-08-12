from __future__ import annotations

import re
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
BUILD_ORCHESTRATOR = REPOSITORY / ".github/workflows/build-payloads-ci.yml"
ASSEMBLY_WORKFLOW = REPOSITORY / ".github/workflows/payload-ci.yml"
SCCACHE_ACTION = (
    "mozilla-actions/sccache-action@fc920bf0ec8de6ee65d409111f7ec508035751ba"
)


class PayloadCiWorkflowTests(unittest.TestCase):
    def test_platform_payload_builds_use_an_optional_observable_sccache(self) -> None:
        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                workflow = (
                    REPOSITORY
                    / f".github/workflows/build-{platform}-payload-ci.yml"
                ).read_text(encoding="utf-8")

                self.assertIn(f"uses: {SCCACHE_ACTION}", workflow)
                self.assertRegex(
                    workflow,
                    re.compile(
                        r"id: sccache\n"
                        r"(?:.*\n){0,4}?\s+continue-on-error: true\n"
                        rf"\s+uses: {re.escape(SCCACHE_ACTION)}",
                    ),
                )
                self.assertIn("SCCACHE_GHA_ENABLED=true", workflow)
                self.assertIn("SCCACHE_IGNORE_SERVER_IO_ERROR=1", workflow)
                self.assertIn("RUSTC_WRAPPER=sccache", workflow)
                self.assertNotIn("sccache --show-stats", workflow)

    def test_manual_cold_build_bypasses_both_platform_caches(self) -> None:
        orchestrator = BUILD_ORCHESTRATOR.read_text(encoding="utf-8")
        self.assertRegex(
            orchestrator,
            re.compile(
                r"workflow_dispatch:\n"
                r"\s+inputs:\n"
                r"\s+cold-build:\n"
                r"\s+description: .+\n"
                r"\s+required: false\n"
                r"\s+type: boolean\n"
                r"\s+default: false\n"
            ),
        )
        self.assertEqual(
            orchestrator.count(
                "cold-build: ${{ github.event_name == 'workflow_dispatch' "
                "&& inputs['cold-build'] }}"
            ),
            2,
        )
        self.assertNotIn("  pull_request:", orchestrator)
        self.assertIn("permissions:\n  contents: read", orchestrator)
        self.assertNotIn("contents: write", orchestrator)
        self.assertNotIn("gh release", orchestrator)
        self.assertLess(
            orchestrator.index('if [[ "$EVENT_NAME" == "workflow_dispatch" ]]'),
            orchestrator.index("version=$(jq -er"),
        )
        self.assertLess(
            orchestrator.index(
                "python -I -B build/verify_release_versions.py\n"
                "            version=$(python"
            ),
            orchestrator.index("effective_plan=$(jq -cn"),
        )
        self.assertIn(
            "  manual-assemble-and-smoke:\n"
            "    name: Assemble and smoke the manual diagnostic build\n"
            "    if: ${{ github.event_name == 'workflow_dispatch' }}",
            orchestrator,
        )
        self.assertEqual(
            orchestrator.count("uses: ./.github/workflows/payload-ci.yml"),
            1,
        )

        assembly = ASSEMBLY_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("  workflow_call:\n", assembly)
        self.assertNotIn("workflow_dispatch:", assembly)
        self.assertNotIn("pull_request:", assembly)
        self.assertNotIn("cold-build", assembly)
        self.assertNotIn("build-linux-payload-ci.yml", assembly)
        self.assertNotIn("build-windows-payload-ci.yml", assembly)

        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                workflow = (
                    REPOSITORY
                    / f".github/workflows/build-{platform}-payload-ci.yml"
                ).read_text(encoding="utf-8")
                self.assertRegex(
                    workflow,
                    re.compile(
                        r"workflow_call:\n"
                    ),
                )
                self.assertIn("cold-build:", workflow)
                self.assertIn("if: ${{ !inputs['cold-build'] }}", workflow)


if __name__ == "__main__":
    unittest.main()
