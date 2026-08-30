from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parents[2]
SCRIPT = REPOSITORY / "build/verify_release_versions.py"


def write_release_versions(
    repository: Path,
    *,
    release: str = "1.2.3",
    rust: str = "1.2.3",
    workflow: str = "1.2.3",
) -> None:
    (repository / "release/kat").mkdir(parents=True)
    (repository / "kat/platform/workflow").mkdir(parents=True)
    (repository / "release/kat/dist.toml").write_text(
        f'[package]\nname = "kat"\nversion = "{release}"\n', encoding="utf-8"
    )
    (repository / "Cargo.toml").write_text(
        f'[workspace.package]\nversion = "{rust}"\n', encoding="utf-8"
    )
    (repository / "kat/platform/workflow/pyproject.toml").write_text(
        f'[project]\nname = "kat-workflow"\nversion = "{workflow}"\n',
        encoding="utf-8",
    )


class ReleaseVersionTests(unittest.TestCase):
    def run_verifier(
        self, repository: Path, *, tag: str | None = None
    ) -> subprocess.CompletedProcess[str]:
        command = [
            sys.executable,
            "-I",
            "-B",
            str(SCRIPT),
            "--repository",
            str(repository),
        ]
        if tag is not None:
            command.extend(("--tag", tag))
        return subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
        )

    def test_matching_release_versions_succeed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(repository)

            result = self.run_verifier(repository)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "release versions match: 1.2.3\n")
            self.assertEqual(result.stderr, "")

    def test_exact_stable_release_tag_succeeds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(repository)

            result = self.run_verifier(repository, tag="kat/1.2.3")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout,
                "release versions and tag match: kat/1.2.3\n",
            )
            self.assertEqual(result.stderr, "")

    def test_exact_prerelease_tag_succeeds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(
                repository,
                release="1.2.3-rc.1",
                rust="1.2.3-rc.1",
                workflow="1.2.3-rc.1",
            )

            result = self.run_verifier(repository, tag="kat/1.2.3-rc.1")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                result.stdout,
                "release versions and tag match: kat/1.2.3-rc.1\n",
            )
            self.assertEqual(result.stderr, "")

    def test_prerelease_sources_reject_the_corresponding_stable_tag(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(
                repository,
                release="1.2.3-rc.1",
                rust="1.2.3-rc.1",
                workflow="1.2.3-rc.1",
            )

            result = self.run_verifier(repository, tag="kat/1.2.3")

            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, "")
            self.assertIn(
                "release tag must be exactly kat/1.2.3-rc.1",
                result.stderr,
            )

    def test_non_contract_release_tags_fail(self) -> None:
        invalid_tags = (
            "kat1.2.3",
            "kat/v1.2.3",
            "kat/foo/1.2.3",
            "validation/pr-161/kat/1.2.3",
            "v1.2.3",
            "kat/1.2.4",
        )
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(repository)

            for tag in invalid_tags:
                with self.subTest(tag=tag):
                    result = self.run_verifier(repository, tag=tag)

                    self.assertEqual(result.returncode, 1)
                    self.assertEqual(result.stdout, "")
                    self.assertIn(
                        "release tag must be exactly kat/1.2.3",
                        result.stderr,
                    )

    def test_rust_version_drift_reports_all_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(repository, rust="1.2.2")

            result = self.run_verifier(repository)

            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, "")
            self.assertIn("release/kat/dist.toml: 1.2.3", result.stderr)
            self.assertIn("Cargo.toml: 1.2.2", result.stderr)
            self.assertIn("kat/platform/workflow/pyproject.toml: 1.2.3", result.stderr)

    def test_workflow_version_drift_reports_all_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_release_versions(repository, workflow="1.2.4")

            result = self.run_verifier(repository)

            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, "")
            self.assertIn("release/kat/dist.toml: 1.2.3", result.stderr)
            self.assertIn("Cargo.toml: 1.2.3", result.stderr)
            self.assertIn("kat/platform/workflow/pyproject.toml: 1.2.4", result.stderr)


if __name__ == "__main__":
    unittest.main()
