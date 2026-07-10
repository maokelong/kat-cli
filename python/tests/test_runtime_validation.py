import json
import os
import sys
import traceback
from pathlib import Path

import pytest
from datafusion import SessionContext


REPO_ROOT = Path(__file__).resolve().parents[2]
RUNTIME_ROOT = REPO_ROOT / "python" / "kat-python-runtime"
sys.path.insert(0, str(RUNTIME_ROOT))

from kat_runtime.artifacts import materialize_artifacts, validate_artifacts
from kat_runtime.dataset import register_dataset


def write_catalog(dataset: Path, tables: object) -> None:
    (dataset / "catalog.json").write_text(
        json.dumps({"tables": tables}),
        encoding="utf-8",
    )


def write_parquet(path: Path, value: int = 405) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    SessionContext().sql(f"select {value} as itid").write_parquet(str(path))


def test_register_dataset_registers_valid_parquet(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    write_parquet(dataset / "tables" / "thread.parquet")
    write_catalog(
        dataset,
        [{"name": "thread", "path": "tables/thread.parquet", "kind": "source"}],
    )
    ctx = SessionContext()

    register_dataset(ctx, dataset)

    assert ctx.table("thread").to_pydict() == {"itid": [405]}


@pytest.mark.parametrize(
    ("catalog", "message"),
    [
        ([], "catalog must be a JSON object"),
        ({"tables": {}}, "catalog tables must be a list"),
    ],
)
def test_register_dataset_rejects_invalid_catalog_shape(tmp_path, catalog, message):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    (dataset / "catalog.json").write_text(json.dumps(catalog), encoding="utf-8")

    with pytest.raises(TypeError, match=message):
        register_dataset(SessionContext(), dataset)


@pytest.mark.parametrize(
    ("tables", "message"),
    [
        (["not-an-object"], "must be an object"),
        ([{"name": "", "path": "tables/a.parquet"}], "non-empty name"),
        ([{"name": "a", "path": ""}], "non-empty path"),
        ([{"name": "a", "path": "../outside.parquet"}], "invalid relative path"),
        ([{"name": "a", "path": "tables/missing.parquet"}], "does not exist"),
    ],
)
def test_register_dataset_rejects_invalid_catalog_entries(tmp_path, tables, message):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    write_catalog(dataset, tables)

    with pytest.raises((TypeError, ValueError, FileNotFoundError), match=message):
        register_dataset(SessionContext(), dataset)


def test_register_dataset_rejects_duplicate_table_names(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    write_parquet(dataset / "tables" / "a.parquet", 1)
    write_parquet(dataset / "tables" / "b.parquet", 2)
    write_catalog(
        dataset,
        [
            {"name": "duplicate", "path": "tables/a.parquet"},
            {"name": "duplicate", "path": "tables/b.parquet"},
        ],
    )

    with pytest.raises(ValueError, match="duplicate dataset table name"):
        register_dataset(SessionContext(), dataset)


def test_register_dataset_rejects_absolute_and_symlink_escape(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    outside = tmp_path / "outside.parquet"
    write_parquet(outside)

    write_catalog(dataset, [{"name": "outside", "path": str(outside)}])
    with pytest.raises(ValueError, match="invalid relative path"):
        register_dataset(SessionContext(), dataset)

    link = dataset / "linked.parquet"
    try:
        link.symlink_to(outside)
    except OSError as error:
        pytest.skip(f"symlink creation is unavailable: {error}")
    write_catalog(dataset, [{"name": "outside", "path": "linked.parquet"}])
    with pytest.raises(ValueError, match="escapes dataset root"):
        register_dataset(SessionContext(), dataset)


@pytest.mark.skipif(sys.platform != "win32", reason="Windows root-relative path")
def test_register_dataset_rejects_windows_root_relative_path(tmp_path):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    parquet_path = dataset / "tables" / "thread.parquet"
    write_parquet(parquet_path)
    resolved_path = parquet_path.resolve()
    root_relative_path = str(resolved_path)[len(resolved_path.drive) :]
    parsed_path = Path(root_relative_path)
    assert parsed_path.root
    assert not parsed_path.drive
    assert not parsed_path.is_absolute()
    write_catalog(dataset, [{"name": "thread", "path": root_relative_path}])

    with pytest.raises(ValueError, match="invalid relative path"):
        register_dataset(SessionContext(), dataset)


def test_register_dataset_preserves_datafusion_error_with_table_and_path_context(
    tmp_path,
):
    dataset = tmp_path / "dataset"
    dataset.mkdir()
    bad = dataset / "tables" / "bad.parquet"
    bad.parent.mkdir()
    bad.write_text("not parquet", encoding="utf-8")
    table_name = "bad_table"
    write_catalog(
        dataset,
        [{"name": table_name, "path": "tables/bad.parquet"}],
    )

    with pytest.raises(Exception) as direct_error:
        SessionContext().register_parquet(table_name, str(bad.resolve()))

    with pytest.raises(Exception) as registered_error:
        register_dataset(SessionContext(), dataset)

    assert type(registered_error.value) is type(direct_error.value)
    assert str(registered_error.value) == str(direct_error.value)
    formatted_traceback = "".join(traceback.format_exception(registered_error.value))
    assert table_name in formatted_traceback
    assert str(bad.resolve()) in formatted_traceback


def test_register_dataset_preserves_error_without_add_note_support(tmp_path):
    class RegistrationError(Exception):
        add_note = None

    class FailingContext:
        def __init__(self, error):
            self.error = error

        def register_parquet(self, _name, _path):
            raise self.error

    dataset = tmp_path / "dataset"
    dataset.mkdir()
    parquet_path = dataset / "tables" / "table.parquet"
    parquet_path.parent.mkdir()
    parquet_path.write_text("stub", encoding="utf-8")
    table_name = "python_310_table"
    write_catalog(
        dataset,
        [{"name": table_name, "path": "tables/table.parquet"}],
    )
    original_error = RegistrationError("registration failed")

    with pytest.raises(RegistrationError) as raised_error:
        register_dataset(FailingContext(original_error), dataset)

    assert raised_error.value is original_error
    assert str(raised_error.value) == "registration failed"
    formatted_traceback = "".join(traceback.format_exception(raised_error.value))
    assert table_name in formatted_traceback
    assert str(parquet_path.resolve()) in formatted_traceback


class FakeDataFrame:
    def write_parquet(self, path):
        Path(path).write_text("fake", encoding="utf-8")


@pytest.mark.parametrize(
    "name",
    ["", "-nodes", "path.nodes", "path/nodes", "path\\nodes", "路径"],
)
def test_validate_artifacts_rejects_unsafe_names_before_writing(tmp_path, name):
    dataframe = SessionContext().sql("select 1 as node_id")

    with pytest.raises(ValueError, match="invalid artifact name"):
        validate_artifacts({name: dataframe}, tmp_path / "run")

    assert not (tmp_path / "run" / "artifacts").exists()


def test_validate_artifacts_rejects_duck_typed_value_before_writing(tmp_path):
    result = {
        "valid": SessionContext().sql("select 1 as node_id"),
        "fake": FakeDataFrame(),
    }

    with pytest.raises(TypeError, match="not a DataFusion DataFrame"):
        validate_artifacts(result, tmp_path / "run")

    assert not (tmp_path / "run" / "artifacts").exists()


def test_materialize_artifacts_preflights_all_targets_before_writing(tmp_path):
    run_dir = tmp_path / "run"
    artifacts_dir = run_dir / "artifacts"
    artifacts_dir.mkdir(parents=True)
    (artifacts_dir / "second.parquet").write_text("keep", encoding="utf-8")
    ctx = SessionContext()
    plans = validate_artifacts(
        {
            "first": ctx.sql("select 1 as value"),
            "second": ctx.sql("select 2 as value"),
        },
        run_dir,
    )

    with pytest.raises(FileExistsError, match="artifact target already exists"):
        materialize_artifacts(plans)

    assert not (artifacts_dir / "first.parquet").exists()
    assert (artifacts_dir / "second.parquet").read_text(encoding="utf-8") == "keep"


def test_materialize_artifacts_preflights_lexists_target_before_writing(
    tmp_path, monkeypatch
):
    run_dir = tmp_path / "run"
    ctx = SessionContext()
    plans = validate_artifacts(
        {
            "first": ctx.sql("select 1 as value"),
            "second": ctx.sql("select 2 as value"),
        },
        run_dir,
    )
    reported_target = plans[1].path
    real_lexists = os.path.lexists

    def report_later_target(path):
        return Path(path) == reported_target or real_lexists(path)

    monkeypatch.setattr(os.path, "lexists", report_later_target)

    with pytest.raises(FileExistsError, match="artifact target already exists"):
        materialize_artifacts(plans)

    assert not plans[0].path.exists()


def test_materialize_artifacts_writes_queryable_single_files(tmp_path):
    run_dir = tmp_path / "run"
    plans = validate_artifacts(
        {"path_nodes": SessionContext().sql("select 405 as itid, 'main' as name")},
        run_dir,
    )

    artifacts = materialize_artifacts(plans)
    artifact_path = run_dir / artifacts[0]["path"]
    reader = SessionContext()
    reader.register_parquet("path_nodes", str(artifact_path))

    assert artifacts == [
        {"name": "path_nodes", "path": "artifacts/path_nodes.parquet"}
    ]
    assert artifact_path.is_file()
    assert reader.table("path_nodes").to_pydict() == {
        "itid": [405],
        "name": ["main"],
    }
