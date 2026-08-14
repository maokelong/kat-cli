from __future__ import annotations

import re
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
BUILD_ORCHESTRATOR = REPOSITORY / ".github/workflows/build-payloads-ci.yml"
FULL_CI_WORKFLOW = REPOSITORY / ".github/workflows/full-ci.yml"
ASSEMBLY_WORKFLOW = REPOSITORY / ".github/workflows/payload-ci.yml"
PR_VALIDATION_CONCURRENCY = (
    "concurrency:\n"
    "  group: >-\n"
    "    ${{ github.workflow }}-${{\n"
    "      github.event_name == 'pull_request' &&\n"
    "      (github.event.action != 'labeled' || "
    "github.event.label.name == 'full-ci') &&\n"
    "      github.event.pull_request.number ||\n"
    "      github.run_id\n"
    "    }}\n"
    "  cancel-in-progress: >-\n"
    "    ${{\n"
    "      github.event_name == 'pull_request' &&\n"
    "      (github.event.action != 'labeled' || "
    "github.event.label.name == 'full-ci')\n"
    "    }}\n"
)
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

    def test_full_ci_ignores_draft_and_unrelated_labeled_events(self) -> None:
        workflow = FULL_CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "  pull_request:\n"
            "    types:\n"
            "      - labeled\n"
            "      - synchronize\n"
            "      - reopened\n"
            "      - ready_for_review\n",
            workflow,
        )
        self.assertIn(
            "    if: >-\n"
            "      github.event_name == 'workflow_dispatch' ||\n"
            "      (github.event.pull_request.draft == false &&\n"
            "      contains(github.event.pull_request.labels.*.name, 'full-ci') &&\n"
            "      (github.event.action != 'labeled' || "
            "github.event.label.name == 'full-ci'))\n",
            workflow,
        )

    def test_full_ci_unrelated_labels_do_not_cancel_active_validation(self) -> None:
        workflow = FULL_CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(PR_VALIDATION_CONCURRENCY, workflow)

    def test_payload_pr_runs_cancel_only_older_runs_for_the_same_pr(self) -> None:
        workflow = BUILD_ORCHESTRATOR.read_text(encoding="utf-8")
        self.assertIn(PR_VALIDATION_CONCURRENCY, workflow)

    def test_payload_assembly_distinguishes_plan_from_validated_app_version(
        self,
    ) -> None:
        orchestrator = BUILD_ORCHESTRATOR.read_text(encoding="utf-8")
        self.assertIn(
            "      plan:\n"
            "        description: Initial dist manifest; the host job regenerates "
            "its upload manifest.\n"
            "        required: true\n"
            "        type: string\n",
            orchestrator,
        )
        self.assertIn(
            "    outputs:\n"
            "      app-version: ${{ steps.verify.outputs.app-version }}\n",
            orchestrator,
        )
        self.assertEqual(
            orchestrator.count('echo "app-version=${version}" >> "$GITHUB_OUTPUT"'),
            1,
        )
        self.assertNotIn("effective_plan", orchestrator)
        self.assertIn(
            "    with:\n"
            "      app-version: ${{ "
            "needs.release-channel.outputs['app-version'] }}\n",
            orchestrator,
        )

        assembly = ASSEMBLY_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "    inputs:\n"
            "      plan:\n"
            "        description: Complete dist manifest for tag publication.\n"
            "        required: false\n"
            "        default: \"\"\n"
            "        type: string\n"
            "      app-version:\n"
            "        description: Validated KAT release version for the candidate.\n"
            "        required: false\n"
            "        default: \"\"\n"
            "        type: string\n",
            assembly,
        )
        self.assertIn("PLAN: ${{ inputs.plan }}", assembly)
        self.assertIn("APP_VERSION: ${{ inputs['app-version'] }}", assembly)
        self.assertIn(
            'if [[ -n "$APP_VERSION" ]]; then\n'
            '            test -z "$PLAN"\n'
            '            version="$APP_VERSION"\n'
            "          else\n"
            '            test -n "$PLAN"\n'
            "            version=$(jq -er ",
            assembly,
        )
        self.assertIn(".releases | map(.app_version)", assembly)

    def test_full_ci_pr_and_manual_dispatch_run_the_payload_pipeline(self) -> None:
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
        self.assertIn(
            "  pull_request:\n"
            "    types:\n"
            "      - labeled\n"
            "      - synchronize\n"
            "      - reopened\n"
            "      - ready_for_review\n",
            orchestrator,
        )
        self.assertIn(
            "contains(github.event.pull_request.labels.*.name, 'full-ci')",
            orchestrator,
        )
        self.assertEqual(
            orchestrator.count(
                "contains(github.event.pull_request.labels.*.name, 'full-ci')"
            ),
            2,
        )
        self.assertEqual(
            orchestrator.count(
                "github.event.action != 'labeled' || "
                "github.event.label.name == 'full-ci'"
            ),
            4,
        )
        self.assertEqual(
            orchestrator.count("github.event.pull_request.draft == false"),
            2,
        )
        self.assertIn(
            "  release-channel:\n"
            "    name: Verify the release channel contract\n"
            "    if: >-\n"
            "      github.event_name != 'pull_request' ||",
            orchestrator,
        )
        self.assertIn("permissions:\n  contents: read", orchestrator)
        self.assertNotIn("contents: write", orchestrator)
        self.assertNotIn("gh release", orchestrator)
        self.assertLess(
            orchestrator.index(
                'if [[ "$EVENT_NAME" == "workflow_dispatch" '
                '|| "$EVENT_NAME" == "pull_request" ]]'
            ),
            orchestrator.index("version=$(jq -er"),
        )
        self.assertLess(
            orchestrator.index(
                "python -I -B build/verify_release_versions.py\n"
                "            version=$(python"
            ),
            orchestrator.index('echo "app-version=${version}"'),
        )
        self.assertIn(
            "  assemble-and-smoke:\n"
            "    name: Assemble and smoke the selected candidate\n",
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
                self.assertNotIn("plan:", workflow)


if __name__ == "__main__":
    unittest.main()
