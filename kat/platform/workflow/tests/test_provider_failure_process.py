from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

import pyarrow as pa
import pyarrow.parquet as pq


class ProviderFailureProcessTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.candidate_index = 0

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_runtime(
        self, request: object
    ) -> tuple[subprocess.CompletedProcess[bytes], dict[str, object]]:
        token = uuid.uuid4().hex
        request_path = self.root / f"request-{token}.json"
        response_path = self.root / f"response-{token}.json"
        request_path.write_text(json.dumps(request), encoding="utf-8")
        completed = subprocess.run(
            [
                sys.executable,
                "-B",
                "-X",
                "utf8",
                "-u",
                "-m",
                "_kat_runtime",
                "--request",
                str(request_path),
                "--response",
                str(response_path),
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            env={**os.environ, "NO_COLOR": "1"},
        )
        self.assertTrue(
            response_path.is_file(),
            completed.stderr.decode(errors="replace"),
        )
        return completed, json.loads(response_path.read_text(encoding="utf-8"))

    def pack(self, body: str) -> Path:
        pack = self.root / f"pack-{uuid.uuid4().hex}"
        (pack / "workflows").mkdir(parents=True)
        (pack / "workflows" / "entry.py").write_text(body, encoding="utf-8")
        return pack.resolve()

    def candidate(self) -> tuple[str, Path, Path]:
        data_home = self.root / "data-home"
        runs = data_home / "runs"
        runs.mkdir(parents=True, exist_ok=True)
        self.candidate_index += 1
        candidate_id = f"019f6e00-0000-7000-8000-{self.candidate_index:012x}"
        candidate = runs / candidate_id
        candidate.mkdir()
        return candidate_id, candidate.resolve(), data_home.resolve()

    def request(
        self,
        pack: Path,
        candidate_id: str,
        candidate: Path,
        data_home: Path,
    ) -> dict[str, object]:
        return {
            "operation": "run_workflow",
            "pack_name": "example",
            "pack_path": str(pack),
            "workflow_name": "analyze",
            "arguments": [],
            "candidate_id": candidate_id,
            "candidate_path": str(candidate),
            "datasource_root": str(data_home / "datasources" / "example"),
        }

    def assert_failed_and_clean(
        self,
        response: dict[str, object],
        candidate: Path,
        *,
        cause: str | None = None,
    ) -> None:
        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        if cause is not None:
            self.assertIn(cause, json.dumps(response))
        self.assertFalse((candidate / "manifest.json").exists())
        output_root = candidate / "outputs"
        self.assertEqual(list(output_root.iterdir()) if output_root.exists() else [], [])
        self.assertFalse((candidate / ".scratch").exists())

    def require_symlink_capability(self, *, target_is_directory: bool = False) -> None:
        probe = self.root / "symlink-probe"
        target = self.root / "symlink-probe-target"
        if target_is_directory:
            target.mkdir()
        try:
            probe.symlink_to(target, target_is_directory=target_is_directory)
        except OSError as error:
            self.skipTest(f"symlink creation is unavailable: {error}")
        finally:
            if probe.is_symlink():
                probe.unlink()
            if target.is_dir():
                target.rmdir()

    def test_execute_failure_poison_closes_every_executor_once_and_cleans(self) -> None:
        first_close_log = self.root / "first-close-log"
        second_close_log = self.root / "second-close-log"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class SuccessfulExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        with Path({first_close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


class ExecuteFailureExecutor:
    def execute(self, sql, params, *, scratch):
        raise RuntimeError("execute failure sentinel")

    def close(self):
        with Path({second_close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A caught query failure still poisons this operation."""
    fallback = ctx.from_arrow(pa.table({{"value": [99]}}))
    ctx.provider(SuccessfulExecutor()).query("first", name="first_rows")
    try:
        ctx.provider(ExecuteFailureExecutor()).query("fails", name="failed_rows")
    except RuntimeError:
        pass
    return fallback
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assert_failed_and_clean(
            response,
            candidate,
            cause="execute failure sentinel",
        )
        self.assertEqual(first_close_log.read_text(encoding="utf-8"), "closed\n")
        self.assertEqual(second_close_log.read_text(encoding="utf-8"), "closed\n")

    def test_context_enter_failure_poisons_and_does_not_call_exit(self) -> None:
        close_log = self.root / "enter-close-log"
        exit_marker = self.root / "enter-exit-marker"
        pack = self.pack(
            f'''from pathlib import Path

import kat
import pyarrow as pa


class EnterFailureManager:
    def __enter__(self):
        raise RuntimeError("context enter failure sentinel")

    def __exit__(self, error_type, error, traceback):
        Path({exit_marker.as_posix()!r}).write_text("called", encoding="utf-8")


class EnterFailureExecutor:
    def execute(self, sql, params, *, scratch):
        return EnterFailureManager()

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """Recovering inside the function cannot unpoison publication."""
    try:
        ctx.provider(EnterFailureExecutor()).query("fails", name="rows")
    except RuntimeError:
        pass
    return ctx.from_arrow(pa.table({{"value": [99]}}))
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assert_failed_and_clean(
            response,
            candidate,
            cause="context enter failure sentinel",
        )
        self.assertFalse(exit_marker.exists())
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_stream_failure_poisons_and_removes_partial_file(self) -> None:
        close_log = self.root / "stream-close-log"
        exit_marker = self.root / "stream-exit-marker"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class StreamFailureExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("value", pa.int64())])

        def batches():
            yield pa.record_batch([[1]], schema=schema)
            raise RuntimeError("stream failure sentinel")

        try:
            yield pa.RecordBatchReader.from_batches(schema, batches())
        finally:
            Path({exit_marker.as_posix()!r}).write_text("exited", encoding="utf-8")

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A mid-stream source failure is fatal and leaves no staged output."""
    try:
        ctx.provider(StreamFailureExecutor()).query("fails", name="rows")
    except RuntimeError:
        pass
    return ctx.from_arrow(pa.table({{"value": [99]}}))
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assert_failed_and_clean(
            response,
            candidate,
            cause="stream failure sentinel",
        )
        self.assertTrue(exit_marker.is_file())
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_context_exit_cannot_suppress_a_stream_failure(self) -> None:
        close_log = self.root / "suppressing-close-log"
        exit_marker = self.root / "suppressing-exit-marker"
        pack = self.pack(
            f'''from pathlib import Path

import kat
import pyarrow as pa


class SuppressingManager:
    def __enter__(self):
        schema = pa.schema([("value", pa.int64())])

        def batches():
            yield pa.record_batch([[1]], schema=schema)
            raise RuntimeError("suppressed stream failure sentinel")

        return pa.RecordBatchReader.from_batches(schema, batches())

    def __exit__(self, error_type, error, traceback):
        Path({exit_marker.as_posix()!r}).write_text("exited", encoding="utf-8")
        return True


class SuppressingExecutor:
    def execute(self, sql, params, *, scratch):
        return SuppressingManager()

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """An executor cannot turn a failed stream into a complete Table."""
    return ctx.provider(SuppressingExecutor()).query("fails", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assert_failed_and_clean(
            response,
            candidate,
            cause="suppressed stream failure sentinel",
        )
        self.assertTrue(exit_marker.is_file())
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_parquet_sink_failure_poisons_and_removes_partial_file(self) -> None:
        close_log = self.root / "sink-close-log"
        exit_marker = self.root / "sink-exit-marker"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class UnsupportedArrowExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        schema = pa.schema([("unsupported", pa.month_day_nano_interval())])
        try:
            yield pa.RecordBatchReader.from_batches(schema, [])
        finally:
            Path({exit_marker.as_posix()!r}).write_text("exited", encoding="utf-8")

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A sink failure remains fatal after a local catch."""
    try:
        ctx.provider(UnsupportedArrowExecutor()).query("fails", name="rows")
    except Exception:
        pass
    return ctx.from_arrow(pa.table({{"value": [99]}}))
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assert_failed_and_clean(
            response,
            candidate,
            cause="Unhandled type for Arrow to Parquet schema conversion",
        )
        self.assertTrue(exit_marker.is_file())
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_context_exit_failure_poisons_and_removes_completed_partial(self) -> None:
        close_log = self.root / "exit-close-log"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class ExitFailureExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())
        raise RuntimeError("context exit failure sentinel")

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A context cleanup failure cannot publish the staged result."""
    try:
        ctx.provider(ExitFailureExecutor()).query("fails", name="rows")
    except RuntimeError:
        pass
    return ctx.from_arrow(pa.table({{"value": [99]}}))
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assert_failed_and_clean(
            response,
            candidate,
            cause="context exit failure sentinel",
        )
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_finalize_collision_poisons_and_removes_final_and_scratch(self) -> None:
        close_log = self.root / "finalize-close-log"
        finalize_marker = self.root / "finalize-reached"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class FinalizeCollisionExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        table = pa.table({{"value": [1]}})
        try:
            yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())
        finally:
            final_path = scratch.parent.parent / "outputs" / "rows.parquet"
            final_path.mkdir()
            (final_path / "blocker").write_text("block", encoding="utf-8")
            Path({finalize_marker.as_posix()!r}).write_text("reached", encoding="utf-8")

    def close(self):
        with Path({close_log.as_posix()!r}).open("a", encoding="utf-8") as log:
            log.write("closed\\n")


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A final-path collision cannot leave a published result behind."""
    try:
        ctx.provider(FinalizeCollisionExecutor()).query("fails", name="rows")
    except OSError:
        pass
    return ctx.from_arrow(pa.table({{"value": [99]}}))
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertTrue(finalize_marker.is_file())
        self.assert_failed_and_clean(response, candidate)
        self.assertEqual(close_log.read_text(encoding="utf-8"), "closed\n")

    def test_parquet_source_with_corrupt_data_page_fails_before_table(self) -> None:
        corruption_proof = self.root / "corrupt-page-proof"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa
import pyarrow.parquet as pq


class CorruptParquetExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        source = scratch / "corrupt.parquet"
        table = pa.table({{"value": list(range(10_000))}})
        pq.write_table(
            table,
            source,
            compression="zstd",
            data_page_size=1024,
            use_dictionary=False,
            write_page_checksum=True,
        )
        parquet = pq.ParquetFile(source)
        column = parquet.metadata.row_group(0).column(0)
        offset = column.data_page_offset + column.total_compressed_size - 10
        parquet.close(force=True)
        damaged_bytes = bytearray(source.read_bytes())
        damaged_bytes[offset] ^= 1
        source.write_bytes(damaged_bytes)

        damaged = pq.ParquetFile(source, page_checksum_verification=True)
        if not damaged.schema_arrow.equals(table.schema, check_metadata=False):
            raise AssertionError("corruption damaged the Parquet footer")
        try:
            damaged.read()
        except Exception as error:
            Path({corruption_proof.as_posix()!r}).write_text(
                type(error).__name__,
                encoding="utf-8",
            )
        else:
            raise AssertionError("corrupt Parquet data page remained readable")
        finally:
            damaged.close(force=True)
        yield kat.ParquetSource(source)

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A footer alone is insufficient evidence for a complete local Table."""
    return ctx.provider(CorruptParquetExecutor()).query("corrupt", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertTrue(corruption_proof.is_file())
        self.assert_failed_and_clean(response, candidate)

    def test_parquet_source_rejects_a_directory_link(self) -> None:
        source_directory = self.root / "source-directory"
        source_directory.mkdir()
        pq.write_table(pa.table({"value": [1]}), source_directory / "part.parquet")
        source_link = self.root / "source-link"
        if os.name == "nt":
            created = subprocess.run(
                [
                    "cmd.exe",
                    "/d",
                    "/c",
                    "mklink",
                    "/J",
                    str(source_link),
                    str(source_directory),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(
                created.returncode,
                0,
                created.stderr.decode(errors="replace"),
            )
        else:
            try:
                source_link.symlink_to(source_directory, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"directory link creation is unavailable: {error}")

        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat


class LinkedParquetExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        yield kat.ParquetSource(Path({source_link.as_posix()!r}))

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A ParquetSource link cannot become Runtime-owned backing."""
    return ctx.provider(LinkedParquetExecutor()).query("rows", name="rows")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        try:
            _, response = self.run_runtime(
                self.request(pack, candidate_id, candidate, data_home)
            )
        finally:
            if os.path.lexists(source_link):
                if os.name == "nt":
                    source_link.rmdir()
                else:
                    source_link.unlink()

        self.assertIn("must not be a symbolic link or junction", json.dumps(response))
        self.assert_failed_and_clean(response, candidate)

    def test_dataframe_output_rejects_a_dangling_symlink_before_writing(self) -> None:
        self.require_symlink_capability()

        external_target = self.root / "outside-candidate.parquet"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class DanglingOutputExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        output_link = scratch.parent.parent / "outputs" / "main.parquet"
        output_link.symlink_to(Path({external_target.as_posix()!r}))
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A DataFrame Output cannot follow a pre-existing dangling link."""
    ctx.provider(DanglingOutputExecutor()).query("seed", name="seed")
    return ctx.sql("SELECT * FROM seed")
'''
        )
        candidate_id, candidate, data_home = self.candidate()

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        self.assertFalse(external_target.exists())
        self.assertFalse((candidate / "manifest.json").exists())

    def test_provider_query_rejects_a_dangling_final_symlink_before_execute(
        self,
    ) -> None:
        self.require_symlink_capability()
        execute_marker = self.root / "dangling-provider-execute-marker"
        external_target = self.root / "outside-provider.parquet"
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class MarkerExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        Path({execute_marker.as_posix()!r}).write_text("entered", encoding="utf-8")
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A reserved result path cannot be replaced through a dangling link."""
    fallback = ctx.from_arrow(pa.table({{"value": [99]}}))
    try:
        ctx.provider(MarkerExecutor()).query("rows", name="rows")
    except Exception:
        pass
    return fallback
'''
        )
        candidate_id, candidate, data_home = self.candidate()
        output_root = candidate / "outputs"
        output_root.mkdir()
        output_link = output_root / "rows.parquet"
        output_link.symlink_to(external_target)

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        self.assertIn("query result backing already exists", json.dumps(response))
        self.assertFalse(execute_marker.exists())
        self.assertTrue(output_link.is_symlink())
        self.assertFalse(external_target.exists())
        self.assertFalse((candidate / "manifest.json").exists())
        self.assertFalse((candidate / ".scratch").exists())

    def test_provider_query_rejects_a_scratch_root_symlink_before_execute(
        self,
    ) -> None:
        self.require_symlink_capability(target_is_directory=True)
        execute_marker = self.root / "scratch-provider-execute-marker"
        external_scratch = self.root / "outside-scratch"
        external_scratch.mkdir()
        pack = self.pack(
            f'''from contextlib import contextmanager
from pathlib import Path

import kat
import pyarrow as pa


class MarkerExecutor:
    @contextmanager
    def execute(self, sql, params, *, scratch):
        Path({execute_marker.as_posix()!r}).write_text("entered", encoding="utf-8")
        table = pa.table({{"value": [1]}})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self):
        pass


@kat.workflow(name="analyze", title="Analyze", required_tables=[])
def analyze(ctx: kat.Context):
    """A linked scratch root cannot escape the candidate workspace."""
    fallback = ctx.from_arrow(pa.table({{"value": [99]}}))
    try:
        ctx.provider(MarkerExecutor()).query("rows", name="rows")
    except Exception:
        pass
    return fallback
'''
        )
        candidate_id, candidate, data_home = self.candidate()
        scratch_link = candidate / ".scratch"
        scratch_link.symlink_to(external_scratch, target_is_directory=True)

        _, response = self.run_runtime(
            self.request(pack, candidate_id, candidate, data_home)
        )

        self.assertEqual(response["status"], "failure", response)
        self.assertNotIn("result", response)
        self.assertIn("scratch directory must not be a link", json.dumps(response))
        self.assertFalse(execute_marker.exists())
        self.assertEqual(list(external_scratch.iterdir()), [])
        self.assertFalse((candidate / "manifest.json").exists())


if __name__ == "__main__":
    unittest.main()
