from __future__ import annotations

from contextlib import contextmanager
from pathlib import Path
import tempfile
import unittest

import pyarrow as pa

from _kat_runtime.datasource import WorkflowOperation


class FailingRegistrationSession:
    def __init__(self) -> None:
        self.registration_name: str | None = None
        self.registration_path: Path | None = None
        self.registration_schema: pa.Schema | None = None
        self.final_existed_during_registration = False
        self.deregistered: list[str] = []

    def table_exist(self, name: str) -> bool:
        return False

    def register_parquet(
        self,
        name: str,
        path: str,
        *,
        schema: pa.Schema,
    ) -> None:
        self.registration_name = name
        self.registration_path = Path(path)
        self.registration_schema = schema
        self.final_existed_during_registration = self.registration_path.is_file()
        raise RuntimeError("registration failure sentinel")

    def deregister_table(self, name: str) -> None:
        self.deregistered.append(name)


class SuccessfulExecutor:
    def __init__(self) -> None:
        self.close_count = 0

    @contextmanager
    def execute(self, sql: str, params: object | None, *, scratch: Path):
        table = pa.table({"value": [1]})
        yield pa.RecordBatchReader.from_batches(table.schema, table.to_batches())

    def close(self) -> None:
        self.close_count += 1


class DatasourceRuntimeTest(unittest.TestCase):
    def test_registering_one_executor_twice_still_closes_it_once(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "runs" / "candidate"
            candidate.mkdir(parents=True)
            session = FailingRegistrationSession()
            executor = SuccessfulExecutor()
            operation = WorkflowOperation(
                session,  # type: ignore[arg-type]
                candidate,
                root / "datasources" / "example",
            )

            operation.provider(executor)
            operation.provider(executor)
            operation.close_executors()

            self.assertEqual(executor.close_count, 1)

    def test_registration_failure_rolls_back_poisons_and_cleans(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "runs" / "candidate"
            candidate.mkdir(parents=True)
            session = FailingRegistrationSession()
            executor = SuccessfulExecutor()
            operation = WorkflowOperation(
                session,  # type: ignore[arg-type]
                candidate,
                root / "datasources" / "example",
            )
            provider = operation.provider(executor)

            with self.assertRaisesRegex(
                RuntimeError,
                "registration failure sentinel",
            ) as query_failure:
                provider.query("SELECT value", name="rows")

            self.assertEqual(session.registration_name, "rows")
            self.assertEqual(
                session.registration_path,
                candidate / "outputs" / "rows.parquet",
            )
            self.assertEqual(
                session.registration_schema,
                pa.schema([("value", pa.int64())]),
            )
            self.assertTrue(session.final_existed_during_registration)
            self.assertEqual(session.deregistered, ["rows"])
            with self.assertRaisesRegex(
                RuntimeError,
                "cannot publish after a Provider query failed",
            ) as poison_failure:
                operation.require_publishable()
            self.assertIs(poison_failure.exception.__cause__, query_failure.exception)

            operation.close_executors()
            operation.close_executors()
            operation.cleanup(success=False)
            operation.expire()

            self.assertEqual(executor.close_count, 1)
            self.assertFalse((candidate / "manifest.json").exists())
            output_root = candidate / "outputs"
            self.assertEqual(
                list(output_root.iterdir()) if output_root.exists() else [],
                [],
            )
            self.assertFalse((candidate / ".scratch").exists())


if __name__ == "__main__":
    unittest.main()
