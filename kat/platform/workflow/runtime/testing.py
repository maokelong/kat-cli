from __future__ import annotations

from collections import Counter
from collections.abc import Callable, Iterator, Sequence
from contextlib import contextmanager, redirect_stdout
from dataclasses import dataclass
import os
from pathlib import Path
import sys
import tempfile
from typing import Any
import uuid

import pyarrow as pa
import pyarrow.parquet as pq
import pytest

from .execution import WorkflowExecutionFailure, run_loaded_workflow
from .inspection import CompiledWorkflow
from .pack import ProductionPack, TEST_PROFILE
from .request import ResolvedDatasetRef, RunCandidateRef, TestPackRequest
from .sources import SourceArgumentOverride


class PytestExitError(Exception):
    def __init__(self, exit_code: pytest.ExitCode) -> None:
        self.exit_code = exit_code
        super().__init__("pytest did not complete successfully")

    def message(self) -> str:
        return {
            pytest.ExitCode.TESTS_FAILED: "PACK tests failed",
            pytest.ExitCode.INTERRUPTED: "PACK tests were interrupted",
            pytest.ExitCode.INTERNAL_ERROR: "PACK test Runtime failed",
            pytest.ExitCode.USAGE_ERROR: "PACK test configuration failed",
            pytest.ExitCode.NO_TESTS_COLLECTED: "PACK contains no collected tests",
        }.get(self.exit_code, "PACK tests failed")

    def help(self) -> str:
        return "Inspect the pytest terminal report and Operation log, correct the PACK, and retry"


class KatPytestPlugin:
    def __init__(
        self,
        *,
        pack: ProductionPack,
        workflows: dict[str, CompiledWorkflow],
        datasets: dict[str, ResolvedDatasetRef],
    ) -> None:
        self._pack = pack
        self._workflows = workflows
        self._datasets = datasets
        self._summary: Counter[str] = Counter()
        self._config: pytest.Config | None = None
        self._temporary_roots: dict[str, Path] = {}
        self._failed_nodes: set[str] = set()

    def pytest_configure(self, config: pytest.Config) -> None:
        self._config = config

    @pytest.fixture
    def kat_run(self, tmp_path: Path, request: pytest.FixtureRequest) -> Callable[..., dict[str, pa.Table]]:
        self._temporary_roots[request.node.nodeid] = tmp_path

        def run(
            *,
            workflow: str,
            dataset: str | None = None,
            sources: dict[str, Sequence[str]] | None = None,
            arguments: Sequence[str] = (),
        ) -> dict[str, pa.Table]:
            selected_dataset = None
            if dataset is not None:
                if type(dataset) is not str or not dataset:
                    raise TypeError("kat_run dataset must be a non-empty string or None")
                try:
                    selected_dataset = self._datasets[dataset]
                except KeyError:
                    available = ", ".join(sorted(self._datasets)) or "none"
                    pytest.fail(
                        f"KAT Workflow test execution failed\n"
                        f"caused by: unknown Test Dataset {dataset!r}; available: {available}\n"
                        "help: select a tests/datasets/ candidate carried by this PACK test",
                        pytrace=False,
                    )
            source_overrides = self._source_overrides(sources)
            candidate_id = str(uuid.uuid4())
            run_path = tmp_path / candidate_id
            run_path.mkdir()
            try:
                try:
                    selected_workflow = self._workflows[workflow]
                except KeyError:
                    raise WorkflowExecutionFailure() from ValueError(
                        f"Workflow {workflow!r} was not found in the selected PACK"
                    )
                result = run_loaded_workflow(
                    selected_workflow,
                    pack=self._pack,
                    workflow_name=workflow,
                    dataset=selected_dataset,
                    arguments=list(arguments),
                    candidate=RunCandidateRef(
                        identifier=candidate_id,
                        path=run_path.resolve(strict=True),
                    ),
                    source_overrides=source_overrides,
                )
            # 仅名称查找未命中和生产 Workflow 已知解析/执行失败归属 pytest call phase；
            # 非法 fixture 实参及 harness 异常保留 pytest 原始 traceback。
            except WorkflowExecutionFailure as error:
                pytest.fail(_test_workflow_diagnostic(error), pytrace=False)
            return {
                name: pq.read_table(run_path / "outputs" / f"{name}.parquet")
                for name in result.outputs
            }

        return run

    def _source_overrides(
        self,
        sources: dict[str, Sequence[str]] | None,
    ) -> dict[str, SourceArgumentOverride]:
        if sources is None:
            return {}
        if type(sources) is not dict:
            raise TypeError("kat_run sources must be a dict or None")
        available = {
            entry.interface["name"] for entry in self._pack.source_entries
        }
        overrides: dict[str, SourceArgumentOverride] = {}
        for name, arguments in sources.items():
            if type(name) is not str or name not in available:
                choices = ", ".join(sorted(available)) or "none"
                raise ValueError(
                    f"kat_run Source {name!r} is unknown; available: {choices}"
                )
            overrides[name] = SourceArgumentOverride.create(
                arguments,
                argument_base=self._pack.root,
            )
        return overrides

    def pytest_runtest_logreport(self, report: pytest.TestReport) -> None:
        if report.when == "call" or report.skipped:
            self._record_status(report)
        if report.failed:
            self._failed_nodes.add(report.nodeid)

    def pytest_collectreport(self, report: pytest.CollectReport) -> None:
        if report.skipped:
            self._record_status(report)

    def pytest_terminal_summary(self, terminalreporter: Any) -> None:
        retained = {
            self._temporary_roots[node_id]
            for node_id in self._failed_nodes
            if node_id in self._temporary_roots
        }
        for root in sorted(retained, key=str):
            terminalreporter.write_line(f"KAT retained test root: {root}")

    def summary(self) -> dict[str, int]:
        return {category: self._summary[category] for category in sorted(self._summary)}

    def _record_status(self, report: object) -> None:
        if self._config is None:
            raise RuntimeError("KAT pytest plugin was not configured")
        status = self._config.hook.pytest_report_teststatus(
            report=report, config=self._config
        )
        if status is None:
            return
        category = status[0]
        if type(category) is str and category:
            self._summary[category] += 1


@dataclass(frozen=True)
class TestPackRuntimeResult:
    summary: dict[str, int]


def test_pack(request: TestPackRequest, test_report_path: Path) -> TestPackRuntimeResult:
    pack = ProductionPack.open(
        request.pack_name,
        request.pack_path,
        profile=TEST_PROFILE,
    )
    workflows = pack.load_all()
    plugin = KatPytestPlugin(
        pack=pack,
        workflows=workflows,
        datasets=request.datasets,
    )
    with tempfile.TemporaryDirectory(prefix="kat-pytest-config-") as temporary:
        config_path = Path(temporary) / "pytest.ini"
        config_path.write_text("[pytest]\n", encoding="utf-8", newline="\n")
        with _isolated_pytest_environment(), redirect_stdout(sys.stderr):
            exit_code = pytest.main(
                _pytest_arguments(request, config_path, test_report_path),
                plugins=[plugin],
            )
    exit_code = pytest.ExitCode(exit_code)
    summary = plugin.summary()
    if exit_code != pytest.ExitCode.OK:
        raise PytestExitError(exit_code)
    return TestPackRuntimeResult(summary=summary)


def _pytest_arguments(
    request: TestPackRequest, config_path: Path, test_report_path: Path
) -> list[str]:
    pack_path = request.pack_path
    targets = request.tests or ["tests"]
    return [
        "-q",
        "--color=no",
        "--code-highlight=no",
        "--disable-plugin-autoload",
        "-p",
        "no:cacheprovider",
        "--import-mode=importlib",
        f"--rootdir={pack_path}",
        f"--confcutdir={pack_path}",
        "-c",
        str(config_path),
        "-o",
        "tmp_path_retention_policy=failed",
        "-o",
        "junit_logging=no",
        f"--junitxml={test_report_path}",
        *targets,
    ]


@contextmanager
def _isolated_pytest_environment() -> Iterator[None]:
    inherited = {
        name: value for name, value in os.environ.items() if name.startswith("PYTEST_")
    }
    for name in inherited:
        os.environ.pop(name, None)
    try:
        yield
    finally:
        for name in list(os.environ):
            if name.startswith("PYTEST_"):
                os.environ.pop(name, None)
        os.environ.update(inherited)


def _test_workflow_diagnostic(error: BaseException) -> str:
    causes: list[str] = []
    current: BaseException | None = error
    while current is not None:
        rendered = str(current).strip()
        if rendered:
            causes.append(rendered)
        current = current.__cause__ or current.__context__
    details = "\n".join(f"caused by: {cause}" for cause in causes)
    if details:
        details += "\n"
    return (
        "KAT Workflow test execution failed\n"
        f"{details}"
        "help: correct the Workflow, arguments, or Test Dataset and retry"
    )
