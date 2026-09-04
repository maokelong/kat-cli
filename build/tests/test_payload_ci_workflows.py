from __future__ import annotations

import json
import re
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
BUILD_ORCHESTRATOR = REPOSITORY / ".github/workflows/build-payloads-ci.yml"
FULL_CI_WORKFLOW = REPOSITORY / ".github/workflows/full-ci.yml"
ASSEMBLY_WORKFLOW = REPOSITORY / ".github/workflows/payload-ci.yml"
PREPARE_WORKFLOW = REPOSITORY / ".github/workflows/prepare-payload-ci.yml"
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
    def test_payload_smoke_hitrace_fixture_is_small_and_deterministic(self) -> None:
        generator = REPOSITORY / "build/fixtures/create_payload_smoke_hitrace.py"
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory) / "smoke.htrace"
            subprocess.run(
                [sys.executable, "-I", "-B", str(generator), str(fixture)],
                check=True,
            )
            content = fixture.read_bytes()

        self.assertEqual(len(content), 1024)
        self.assertEqual(struct.unpack_from("<Q", content, 0)[0], 0x464F5250534F484F)
        self.assertEqual(struct.unpack_from("<Q", content, 8)[0], len(content))
        self.assertEqual(struct.unpack_from("<I", content, 56)[0], 0)
        self.assertEqual(struct.unpack_from("<Q", content, 60)[0], 123456)

    def test_payload_smoke_verifier_fails_closed_under_optimized_python(self) -> None:
        verifier = REPOSITORY / "build/fixtures/verify_payload_smoke.py"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ndjson = root / "result.ndjson"
            ndjson.write_bytes(b'{"clock_domain":"wrong","clock_value":0}\n')
            responses = {
                "inspect": {"status": "success"},
                "test": {
                    "status": "success",
                    "result": {"summary": {"passed": 1}},
                },
                "first-run": {
                    "status": "success",
                    "result": {
                        "session_id": "session-1",
                        "run_id": "run-1",
                        "outputs": {"main": {"row_count": 1}},
                    },
                },
                "second-run": {
                    "status": "success",
                    "result": {
                        "session_id": "session-1",
                        "run_id": "run-2",
                        "outputs": {"main": {"row_count": 1}},
                    },
                },
                "query": {
                    "status": "success",
                    "result": {"format": "ndjson", "path": str(ndjson)},
                },
            }
            paths = []
            for name, response in responses.items():
                path = root / f"{name}.json"
                path.write_text(json.dumps(response), encoding="utf-8")
                paths.append(path)

            result = subprocess.run(
                [
                    sys.executable,
                    "-O",
                    "-I",
                    "-B",
                    str(verifier),
                    *(str(path) for path in paths),
                ],
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertNotEqual(result.returncode, 0)

    def test_payload_smoke_verifier_accepts_the_complete_mechanism_chain(
        self,
    ) -> None:
        verifier = REPOSITORY / "build/fixtures/verify_payload_smoke.py"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ndjson = root / "result.ndjson"
            ndjson.write_bytes(
                b'{"clock_domain":"boottime","clock_value":123456}\n'
            )
            responses = {
                "inspect": {"status": "success"},
                "test": {
                    "status": "success",
                    "result": {"summary": {"passed": 1}},
                },
                "first-run": {
                    "status": "success",
                    "result": {
                        "session_id": "session-1",
                        "run_id": "run-1",
                        "outputs": {"main": {"row_count": 1}},
                    },
                },
                "second-run": {
                    "status": "success",
                    "result": {
                        "session_id": "session-1",
                        "run_id": "run-2",
                        "outputs": {"main": {"row_count": 1}},
                    },
                },
                "query": {
                    "status": "success",
                    "result": {"format": "ndjson", "path": str(ndjson)},
                },
            }
            paths = []
            for name, response in responses.items():
                path = root / f"{name}.json"
                path.write_text(json.dumps(response), encoding="utf-8")
                paths.append(path)

            result = subprocess.run(
                [
                    sys.executable,
                    "-O",
                    "-I",
                    "-B",
                    str(verifier),
                    *(str(path) for path in paths),
                ],
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_dual_wheel_identity_flows_through_each_native_payload_job(self) -> None:
        orchestrator = BUILD_ORCHESTRATOR.read_text(encoding="utf-8")
        self.assertIn(
            "      normalized-version: ${{ steps.verify.outputs.normalized-version }}\n",
            orchestrator,
        )
        self.assertIn(
            'echo "normalized-version=${version/-rc./rc}" >> "$GITHUB_OUTPUT"',
            orchestrator,
        )
        self.assertIn(
            "      expected-version: ${{ needs.release-channel.outputs['normalized-version'] }}\n",
            orchestrator,
        )
        self.assertEqual(
            orchestrator.count(
                "      workflow-wheel-sha256: ${{ "
                "needs.prepare.outputs['workflow-wheel-sha256'] }}\n"
            ),
            2,
        )

        prepare = PREPARE_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("      expected-version:\n", prepare)
        self.assertIn("      workflow-wheel-sha256:\n", prepare)
        self.assertIn(
            "        value: ${{ jobs.workflow-wheel.outputs.sha256 }}\n",
            prepare,
        )
        self.assertIn(
            "          --expected-version ${{ inputs['expected-version'] }}\n",
            prepare,
        )
        self.assertIn('echo "sha256=${sha256}" >> "$GITHUB_OUTPUT"', prepare)

        for platform in ("linux", "windows"):
            with self.subTest(platform=platform):
                workflow = (
                    REPOSITORY
                    / f".github/workflows/build-{platform}-payload-ci.yml"
                ).read_text(encoding="utf-8")
                self.assertIn("      expected-version:\n", workflow)
                self.assertIn("      workflow-wheel-sha256:\n", workflow)
                self.assertIn("build/build_datasource_wheel.py", workflow)
                self.assertIn(f"--platform {platform}-x86_64", workflow)
                self.assertIn("--workflow-wheel-version", workflow)
                self.assertIn("--workflow-wheel-sha256", workflow)
                self.assertIn("--datasource-wheel-version", workflow)
                self.assertIn("--datasource-wheel-sha256", workflow)
                self.assertIn("-m venv", workflow)
                self.assertIn("--no-deps --no-index", workflow)
                self.assertIn(
                    "kat/platform/datasource/tests/python/test_hitrace_api.py",
                    workflow,
                )
                self.assertIn(
                    "kat/platform/datasource/tests/python/test_text_ftrace_api.py",
                    workflow,
                )

    def test_payload_smoke_uses_hitrace_provider_and_validates_ndjson(self) -> None:
        assembly = ASSEMBLY_WORKFLOW.read_text(encoding="utf-8")
        self.assertNotIn(" import trace-streamer ", assembly)
        self.assertNotIn("--dataset", assembly)
        self.assertIn("build/fixtures/create_payload_smoke_hitrace.py", assembly)
        self.assertIn("build/fixtures/payload-smoke-pack", assembly)
        self.assertIn("build/fixtures/payload-smoke-consumer-pack", assembly)
        self.assertEqual(assembly.count(" test --pack-dir "), 2)
        self.assertEqual(assembly.count("--workflow summarize-hitrace-clock"), 2)
        self.assertEqual(assembly.count("--workflow reuse-hitrace-clock"), 2)
        self.assertEqual(assembly.count(" query --session "), 2)
        self.assertEqual(
            assembly.count("verify_payload_materialization_race.py"), 2
        )
        self.assertIn(
            'run --session "$session_id" --pack payload-smoke-consumer',
            assembly,
        )
        self.assertIn(
            "run --session $firstRun.result.session_id "
            "--pack payload-smoke-consumer",
            assembly,
        )
        self.assertIn('/bin/rm -- "$fixture"', assembly)
        self.assertIn("Remove-Item -LiteralPath $fixture", assembly)
        self.assertLess(
            assembly.index('/bin/rm -- "$fixture"'),
            assembly.index(
                'run --session "$session_id" --pack payload-smoke-consumer'
            ),
        )
        self.assertLess(
            assembly.index("Remove-Item -LiteralPath $fixture"),
            assembly.index(
                "run --session $firstRun.result.session_id "
                "--pack payload-smoke-consumer"
            ),
        )
        self.assertIn("SELECT clock_domain, clock_value FROM output.main", assembly)
        verifier = (
            REPOSITORY / "build/fixtures/verify_payload_smoke.py"
        ).read_text(encoding="utf-8")
        self.assertIn('result["format"] != "ndjson"', verifier)
        self.assertIn(
            '{"clock_domain":"boottime","clock_value":123456}',
            verifier,
        )

        provider = (
            REPOSITORY
            / "build/fixtures/payload-smoke-pack/datasources/hitrace.py"
        ).read_text(encoding="utf-8")
        workflow = (
            REPOSITORY
            / "build/fixtures/payload-smoke-pack/workflows/summarize_hitrace_clock.py"
        ).read_text(encoding="utf-8")
        consumer = (
            REPOSITORY
            / "build/fixtures/payload-smoke-consumer-pack/workflows/reuse_hitrace_clock.py"
        ).read_text(encoding="utf-8")
        pack_test = (
            REPOSITORY
            / "build/fixtures/payload-smoke-pack/tests/test_payload_smoke.py"
        )
        self.assertTrue(pack_test.is_file())
        self.assertIn("from kat import dataprovider as dp", provider)
        self.assertIn("from kat_datasource import hitrace", provider)
        self.assertIn("hitrace.decode", provider)
        self.assertIn("dp.open(root=", provider)
        self.assertIn("dp.DataFusionProvider", provider)
        self.assertIn("source.stem", provider)
        self.assertIn("pq.read_schema", provider)
        self.assertIn("field.nullable", provider)
        self.assertIn("MATERIALIZATION_VERSION_METADATA_KEY", provider)
        self.assertIn("MATERIALIZATION_VERSION", provider)
        self.assertIn("KAT_PAYLOAD_SMOKE_BARRIER", provider)
        self.assertNotIn("unsupported_plugins", provider)
        self.assertNotIn("unsupported_section_types", provider)
        self.assertIn("HitraceProvider", workflow)
        self.assertIn("return provider.query", workflow)
        self.assertIn("dp.open(root=", consumer)
        self.assertIn("dp.DataFusionProvider", consumer)
        self.assertIn("source.stem", consumer)
        self.assertIn("pq.read_schema", consumer)
        self.assertIn("field.nullable", consumer)
        self.assertIn('b"kat.materialization.version"', consumer)
        self.assertIn('b"hitrace-v1"', consumer)
        self.assertNotIn("kat_datasource", consumer)
        self.assertNotIn("HitraceProvider", consumer)
        self.assertNotIn("decode", consumer)

        race_verifier = (
            REPOSITORY / "build/fixtures/verify_payload_materialization_race.py"
        ).read_text(encoding="utf-8")
        self.assertIn("subprocess.Popen", race_verifier)
        self.assertIn('_CONTENDER_CLOCK_VALUES = (111_111, 222_222)', race_verifier)
        self.assertIn('outcomes != ["published", "reused"]', race_verifier)
        self.assertIn("Source-free reuse replaced", race_verifier)

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
