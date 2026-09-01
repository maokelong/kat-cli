from __future__ import annotations

import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest import mock


REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
import payload_builder
import build_linux_payload
import build_windows_payload


def write_workflow_wheel(
    path: Path,
    *,
    version: str = "0.1.1rc1",
    requires: str = "pyarrow==24.0.0",
) -> None:
    dist_info = f"kat_workflow-{version}.dist-info"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("kat/__init__.py", "")
        archive.writestr("_kat_runtime/__main__.py", "")
        archive.writestr(
            f"{dist_info}/METADATA",
            "Metadata-Version: 2.4\n"
            "Name: kat-workflow\n"
            f"Version: {version}\n"
            f"Requires-Dist: {requires}\n",
        )
        archive.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\n"
            "Root-Is-Purelib: true\n"
            "Tag: py3-none-any\n",
        )


def write_datasource_wheel(
    path: Path,
    *,
    version: str = "0.1.1rc1",
    tag: str = "cp314-cp314-win_amd64",
    extension: str = "kat_datasource/_native.cp314-win_amd64.pyd",
    requires: str | None = None,
) -> None:
    dist_info = f"kat_datasource-{version}.dist-info"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("kat_datasource/__init__.py", "")
        archive.writestr("kat_datasource/hitrace.py", "")
        archive.writestr(extension, b"native extension")
        metadata = (
            "Metadata-Version: 2.4\n"
            "Name: kat-datasource\n"
            f"Version: {version}\n"
        )
        if requires is not None:
            metadata += f"Requires-Dist: {requires}\n"
        archive.writestr(
            f"{dist_info}/METADATA",
            metadata,
        )
        archive.writestr(
            f"{dist_info}/WHEEL",
            "Wheel-Version: 1.0\n"
            "Root-Is-Purelib: false\n"
            f"Tag: {tag}\n",
        )


class WheelArtifactTests(unittest.TestCase):
    def test_wheel_validation_has_no_implicit_path_or_sidecar_fallback(self) -> None:
        with self.assertRaisesRegex(TypeError, "WheelArtifactInput"):
            payload_builder.validated_workflow_wheel(Path("workflow.whl"))
        with self.assertRaisesRegex(TypeError, "WheelArtifactInput"):
            payload_builder.validated_datasource_wheel(
                Path("datasource.whl"),  # type: ignore[arg-type]
                platform="windows-x86_64",
            )

    def test_workflow_artifact_requires_its_expected_version_and_sha256(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wheel = root / "kat_workflow-0.1.1rc1-py3-none-any.whl"
            write_workflow_wheel(wheel)
            digest = payload_builder.file_sha256(wheel)

            artifact = payload_builder.WheelArtifactInput(
                path=wheel,
                expected_version="0.1.1rc1",
                sha256=digest,
            )

            self.assertEqual(
                payload_builder.validated_workflow_wheel(artifact),
                wheel.resolve(),
            )

            with self.subTest(case="version mismatch"), self.assertRaisesRegex(
                ValueError, "expected version"
            ):
                payload_builder.validated_workflow_wheel(
                    payload_builder.WheelArtifactInput(
                        path=wheel,
                        expected_version="0.1.2",
                        sha256=digest,
                    )
                )

            with self.subTest(case="SHA-256 mismatch"), self.assertRaisesRegex(
                ValueError, "SHA-256"
            ):
                payload_builder.validated_workflow_wheel(
                    payload_builder.WheelArtifactInput(
                        path=wheel,
                        expected_version="0.1.1rc1",
                        sha256="0" * 64,
                    )
                )

    def test_datasource_artifact_accepts_only_the_native_platform_identity(self) -> None:
        cases = (
            (
                "windows-x86_64",
                "cp314-cp314-win_amd64",
                "kat_datasource-0.1.1rc1-cp314-cp314-win_amd64.whl",
                "kat_datasource/_native.cp314-win_amd64.pyd",
            ),
            (
                "linux-x86_64",
                "cp314-cp314-manylinux_2_28_x86_64",
                "kat_datasource-0.1.1rc1-cp314-cp314-manylinux_2_28_x86_64.whl",
                "kat_datasource/_native.cpython-314-x86_64-linux-gnu.so",
            ),
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for platform, tag, filename, extension in cases:
                with self.subTest(platform=platform):
                    wheel = root / filename
                    write_datasource_wheel(wheel, tag=tag, extension=extension)
                    artifact = payload_builder.WheelArtifactInput(
                        path=wheel,
                        expected_version="0.1.1rc1",
                        sha256=payload_builder.file_sha256(wheel),
                    )

                    self.assertEqual(
                        payload_builder.validated_datasource_wheel(
                            artifact,
                            platform=platform,
                        ),
                        wheel.resolve(),
                    )

    def test_datasource_wheel_rejects_wrong_tag_and_multiple_extensions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            wheel = (
                Path(directory)
                / "kat_datasource-0.1.1rc1-cp314-cp314-win_amd64.whl"
            )
            write_datasource_wheel(
                wheel,
                tag="cp314-cp314-manylinux_2_28_x86_64",
            )
            with self.assertRaisesRegex(ValueError, "cp314-cp314-win_amd64"):
                payload_builder.validate_datasource_wheel_archive(
                    wheel,
                    expected_version="0.1.1rc1",
                    platform="windows-x86_64",
                )

            write_datasource_wheel(wheel)
            with zipfile.ZipFile(wheel, "a") as archive:
                archive.writestr("kat_datasource/other.pyd", b"unexpected")
            with self.assertRaisesRegex(ValueError, "exactly one"):
                payload_builder.validate_datasource_wheel_archive(
                    wheel,
                    expected_version="0.1.1rc1",
                    platform="windows-x86_64",
                )

    def test_the_two_kat_wheels_do_not_depend_on_each_other(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflow = root / "kat_workflow-0.1.1rc1-py3-none-any.whl"
            write_workflow_wheel(
                workflow,
                requires="kat-datasource==0.1.1rc1",
            )
            with self.assertRaisesRegex(ValueError, "must not depend"):
                payload_builder.validate_workflow_wheel_archive(
                    workflow,
                    expected_version="0.1.1rc1",
                )

            datasource = (
                root
                / "kat_datasource-0.1.1rc1-cp314-cp314-win_amd64.whl"
            )
            write_datasource_wheel(
                datasource,
                requires="kat-workflow==0.1.1rc1",
            )
            with self.assertRaisesRegex(ValueError, "must not depend"):
                payload_builder.validate_datasource_wheel_archive(
                    datasource,
                    expected_version="0.1.1rc1",
                    platform="windows-x86_64",
                )

    def test_payload_installs_both_local_wheels_without_dependency_resolution(
        self,
    ) -> None:
        workflow = Path("workflow.whl")
        datasource = Path("datasource.whl")
        with mock.patch.object(payload_builder.subprocess, "run") as run:
            payload_builder.install_kat_wheels(
                Path("uv"),
                Path("python"),
                (workflow, datasource),
                Path("cache"),
                copy_links=False,
            )

        self.assertEqual(run.call_count, 2)
        for call, wheel in zip(run.call_args_list, (workflow, datasource), strict=True):
            command = call.args[0]
            self.assertEqual(command[:3], ["uv", "pip", "install"])
            self.assertIn("--no-deps", command)
            self.assertIn("--no-index", command)
            self.assertNotIn("--find-links", command)
            self.assertEqual(command[-1], str(wheel))

    def test_workflow_wheel_import_is_checked_before_datasource_is_present(
        self,
    ) -> None:
        with mock.patch.object(payload_builder.subprocess, "run") as run:
            payload_builder.check_isolated_workflow_install(
                Path("python"),
                "0.1.1rc1",
            )

        command = run.call_args.args[0]
        self.assertEqual(command[:4], ["python", "-I", "-B", "-c"])
        self.assertEqual(command[-1], "0.1.1rc1")
        script = command[-2]
        self.assertIn("import kat", script)
        self.assertIn("version('kat-workflow')", script)
        self.assertIn("find_spec('kat_datasource')", script)

    def test_platform_builders_require_two_explicit_wheel_artifacts(self) -> None:
        digest = "a" * 64
        for module in (build_linux_payload, build_windows_payload):
            with self.subTest(platform=module.PLATFORM):
                options = module.parse_args(
                    [
                        "--workflow-wheel",
                        "workflow.whl",
                        "--workflow-wheel-version",
                        "0.1.1rc1",
                        "--workflow-wheel-sha256",
                        digest,
                        "--datasource-wheel",
                        "datasource.whl",
                        "--datasource-wheel-version",
                        "0.1.1rc1",
                        "--datasource-wheel-sha256",
                        digest,
                    ]
                )

                self.assertEqual(
                    options.workflow_wheel,
                    payload_builder.WheelArtifactInput(
                        Path("workflow.whl"), "0.1.1rc1", digest
                    ),
                )
                self.assertEqual(
                    options.datasource_wheel,
                    payload_builder.WheelArtifactInput(
                        Path("datasource.whl"), "0.1.1rc1", digest
                    ),
                )

    def test_payload_validates_both_wheel_artifacts_before_staging(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflow = root / "kat_workflow-0.1.1rc1-py3-none-any.whl"
            datasource = (
                root
                / "kat_datasource-0.1.1rc1-cp314-cp314-win_amd64.whl"
            )
            write_workflow_wheel(workflow)
            write_datasource_wheel(datasource)
            repository = root / "repository"
            repository.mkdir()
            options = mock.Mock(
                repository=repository,
                output=root / "payload",
                download_cache=root / "downloads",
                workflow_wheel=payload_builder.WheelArtifactInput(
                    workflow,
                    "0.1.1rc1",
                    payload_builder.file_sha256(workflow),
                ),
                datasource_wheel=payload_builder.WheelArtifactInput(
                    datasource,
                    "0.1.1rc1",
                    "0" * 64,
                ),
                wheelhouse=None,
                python_archive=None,
                uv_archive=None,
                offline=False,
            )
            adapter = mock.Mock()
            adapter.spec = mock.Mock(
                key="windows-x86_64",
                label="Windows",
            )
            adapter.extra_input_paths.return_value = ()
            adapter.load_inputs.return_value = mock.Mock()

            with self.assertRaisesRegex(ValueError, "SHA-256"):
                payload_builder.build_payload(options, adapter)

            self.assertFalse(options.output.exists())
            adapter.resolve_extra_inputs.assert_not_called()

    def test_payload_requires_one_shared_expected_wheel_version(self) -> None:
        options = mock.Mock(
            workflow_wheel=payload_builder.WheelArtifactInput(
                Path("workflow.whl"), "0.1.1rc1", "a" * 64
            ),
            datasource_wheel=payload_builder.WheelArtifactInput(
                Path("datasource.whl"), "0.1.2", "b" * 64
            ),
        )
        adapter = mock.Mock()

        with self.assertRaisesRegex(ValueError, "same expected version"):
            payload_builder.build_payload(options, adapter)

        adapter.require_builder.assert_not_called()

if __name__ == "__main__":
    unittest.main()
