from __future__ import annotations

import re
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
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
                self.assertIn("sccache --show-stats", workflow)

    def test_manual_cold_build_bypasses_both_platform_caches(self) -> None:
        orchestrator = (
            REPOSITORY / ".github/workflows/payload-ci.yml"
        ).read_text(encoding="utf-8")
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
