from __future__ import annotations

import dataclasses
import importlib.metadata
import importlib.util
import pathlib
import struct
import tempfile
import threading
import time
import tomllib
import unittest


_PROFILER_HEADER_SIZE = 1024
_PROFILER_HEADER_MAGIC = 0x464F_5250_534F_484F


class HitraceApiContractTests(unittest.TestCase):
    def test_public_surface_is_small_and_report_is_immutable(self) -> None:
        import kat_datasource
        from kat_datasource import hitrace

        self.assertEqual(kat_datasource.__all__, ("hitrace",))
        self.assertEqual(
            hitrace.__all__,
            ("decode", "DecodeReport", "DecodeError"),
        )

        report = hitrace.DecodeReport(
            unsupported_plugins=("alpha", "zeta"),
            unsupported_section_types=(7, 23),
        )
        self.assertEqual(report.unsupported_plugins, ("alpha", "zeta"))
        self.assertEqual(report.unsupported_section_types, (7, 23))
        with self.assertRaises(dataclasses.FrozenInstanceError):
            report.unsupported_plugins = ()  # type: ignore[misc]
        self.assertEqual(
            tuple(field.name for field in dataclasses.fields(report)),
            ("unsupported_plugins", "unsupported_section_types"),
        )
        self.assertFalse(hasattr(report, "__dict__"))

        self.assertTrue(issubclass(hitrace.DecodeError, RuntimeError))
        self.assertFalse(hasattr(kat_datasource, "decode"))
        self.assertFalse(hasattr(kat_datasource, "DecodeReport"))
        self.assertFalse(hasattr(kat_datasource, "DecodeError"))

    def test_distribution_version_is_normalized_from_cargo_release_version(self) -> None:
        repository = pathlib.Path(__file__).resolve().parents[5]
        with (repository / "Cargo.toml").open("rb") as source:
            cargo_version = tomllib.load(source)["workspace"]["package"]["version"]

        self.assertEqual(
            importlib.metadata.version("kat-datasource"),
            cargo_version.replace("-rc.", "rc"),
        )

    def test_existing_destination_wins_over_missing_source(self) -> None:
        from kat_datasource import hitrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "missing.htrace"
            destination = root / "relations"
            destination.mkdir()
            sentinel = destination / "owned-by-caller"
            sentinel.write_bytes(b"keep")

            with self.assertRaisesRegex(
                hitrace.DecodeError,
                "destination already exists",
            ):
                hitrace.decode(source, destination)

            self.assertEqual(sentinel.read_bytes(), b"keep")

    def test_decode_returns_report_and_publishes_only_flat_parquet_relations(self) -> None:
        from kat_datasource import hitrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "unknown-content.htrace"
            destination = root / "relations"
            source.write_bytes(
                _profiler_section(["zeta", "alpha_config", "zeta"])
                + _profiler_section([], data_type=1000)
                + _profiler_section([], data_type=77)
                + _profiler_section([], data_type=1000)
            )

            report = hitrace.decode(source, destination)

            self.assertIs(type(report), hitrace.DecodeReport)
            self.assertEqual(report.unsupported_plugins, ("alpha", "zeta"))
            self.assertEqual(report.unsupported_section_types, (77, 1000))
            self.assertEqual(
                sorted(path.name for path in destination.iterdir()),
                ["clock_domain.parquet", "clock_snapshot.parquet"],
            )
            self.assertTrue(all(path.is_file() for path in destination.iterdir()))

    def test_corrupt_source_leaves_no_destination_or_staging(self) -> None:
        from kat_datasource import hitrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "corrupt.htrace"
            destination = root / "relations"
            source.write_bytes(b"not a Hitrace file")

            with self.assertRaises(hitrace.DecodeError):
                hitrace.decode(source, destination)

            self.assertFalse(destination.exists())
            self.assertFalse(
                any(
                    path.name.startswith(".kat-datasource-staging-")
                    for path in root.iterdir()
                )
            )

    def test_invalid_path_type_is_not_mapped_to_decode_error(self) -> None:
        from kat_datasource import hitrace

        with tempfile.TemporaryDirectory() as temporary_directory:
            with self.assertRaises(TypeError):
                hitrace.decode(  # type: ignore[arg-type]
                    42,
                    pathlib.Path(temporary_directory) / "relations",
                )

    def test_native_decode_releases_the_gil(self) -> None:
        from kat_datasource import hitrace

        frame = _profiler_frame("future-plugin")
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = pathlib.Path(temporary_directory)
            source = root / "large-unknown-content.htrace"
            destination = root / "relations"
            source.write_bytes(_profiler_section_body(frame * 500_000, data_type=0))

            stop = threading.Event()
            progress = [0]

            def advance() -> None:
                while not stop.is_set():
                    progress[0] += 1

            worker = threading.Thread(target=advance)
            worker.start()
            time.sleep(0.05)
            baseline_rate = progress[0] / 0.05
            before = progress[0]
            started = time.perf_counter()
            try:
                hitrace.decode(source, destination)
            finally:
                elapsed = time.perf_counter() - started
                after = progress[0]
                stop.set()
                worker.join()

            self.assertGreater(elapsed, 0.03)
            self.assertGreater(
                after - before,
                baseline_rate * elapsed * 0.05,
                "background Python thread did not advance during native decode",
            )

    def test_distribution_has_no_runtime_or_kat_wheel_dependency(self) -> None:
        distribution = importlib.metadata.distribution("kat-datasource")

        self.assertIn(distribution.requires, (None, []))
        self.assertIsNone(importlib.util.find_spec("kat"))
        self.assertEqual(
            [entry_point for entry_point in distribution.entry_points],
            [],
        )


def _profiler_section(names: list[str], *, data_type: int = 0) -> bytes:
    return _profiler_section_body(
        b"".join(_profiler_frame(name) for name in names),
        data_type=data_type,
    )


def _profiler_section_body(body: bytes, *, data_type: int) -> bytes:
    header = bytearray(_PROFILER_HEADER_SIZE)
    struct.pack_into("<Q", header, 0, _PROFILER_HEADER_MAGIC)
    struct.pack_into("<Q", header, 8, _PROFILER_HEADER_SIZE + len(body))
    struct.pack_into("<I", header, 56, data_type)
    return bytes(header) + body


def _profiler_frame(name: str) -> bytes:
    encoded_name = name.encode("utf-8")
    envelope = b"\x0a" + _protobuf_varint(len(encoded_name)) + encoded_name
    return struct.pack("<I", len(envelope)) + envelope


def _protobuf_varint(value: int) -> bytes:
    encoded = bytearray()
    while value >= 0x80:
        encoded.append((value & 0x7F) | 0x80)
        value >>= 7
    encoded.append(value)
    return bytes(encoded)


if __name__ == "__main__":
    unittest.main()
