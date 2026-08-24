from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
import json
import os
from pathlib import Path
from typing import Literal

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .execution import RunWorkflowRuntimeResult, run_workflow
from .pack import (
    InspectPackRuntimeResult,
    PackInspectionError,
    _PackInspectionWorkerError,
    inspect_pack,
)
from .query import QueryRunRuntimeResult, query_dataset, query_run
from .request import (
    BindSourceRequest,
    InspectPackRequest,
    MaterializeSourceRequest,
    QueryDatasetRequest,
    QueryRunRequest,
    RunWorkflowRequest,
    RuntimeRequest,
    RuntimeRequestError,
    TestPackRequest,
    read_request,
)
from .source_commands import (
    BindSourceRuntimeResult,
    MaterializeSourceRuntimeResult,
    bind_source,
    materialize_source,
)
from .testing import PytestExitError, TestPackRuntimeResult, test_pack


@dataclass(frozen=True)
class RuntimeSuccess[R]:
    status: Literal["success"] = field(init=False, default="success")
    result: R


@dataclass(frozen=True)
class RuntimeFailure:
    status: Literal["failure"] = field(init=False, default="failure")
    error: RuntimeDiagnostic


type RuntimeResponse = (
    RuntimeSuccess[InspectPackRuntimeResult]
    | RuntimeSuccess[BindSourceRuntimeResult]
    | RuntimeSuccess[MaterializeSourceRuntimeResult]
    | RuntimeSuccess[RunWorkflowRuntimeResult]
    | RuntimeSuccess[QueryRunRuntimeResult]
    | RuntimeSuccess[TestPackRuntimeResult]
    | RuntimeFailure
)


def main() -> int:
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--request", required=True)
    parser.add_argument("--response", required=True)
    parser.add_argument("--test-report")
    arguments = parser.parse_args()
    response_path = Path(arguments.response)
    try:
        request = read_request(Path(arguments.request))
    except RuntimeRequestError as error:
        response: RuntimeResponse = RuntimeFailure(
            error=diagnostic_from_exception(
                error,
                None,
                message="Runtime Request is invalid",
                help="Use a compatible KAT CLI and Runtime deployment",
            )
        )
    else:
        response = _execute(
            request,
            Path(arguments.test_report) if arguments.test_report is not None else None,
        )
    _write_response(response_path, response)
    return 0


def _execute(
    request: RuntimeRequest, test_report_path: Path | None = None
) -> RuntimeResponse:
    if isinstance(request, InspectPackRequest):
        try:
            result = inspect_pack(request.pack_name, request.pack_path)
        except PackInspectionError as error:
            return RuntimeFailure(error=error.diagnostic)
        return RuntimeSuccess(result=result)

    if isinstance(request, QueryRunRequest):
        try:
            result = query_run(request)
        except (Exception, SystemExit) as error:
            return RuntimeFailure(
                error=diagnostic_from_exception(
                    error,
                    None,
                    message="Run Output query failed",
                    help="Correct the SQL or its inputs, then retry",
                )
            )
        return RuntimeSuccess(result=result)

    if isinstance(request, BindSourceRequest):
        try:
            result = bind_source(request)
        except PackInspectionError as error:
            return RuntimeFailure(error=error.diagnostic)
        except (Exception, SystemExit) as error:
            return RuntimeFailure(
                error=diagnostic_from_exception(
                    error,
                    request.pack_path,
                    message="Source Binding validation failed",
                    help="Correct the Source arguments or PACK, then retry",
                )
            )
        return RuntimeSuccess(result=result)

    if isinstance(request, MaterializeSourceRequest):
        try:
            result = materialize_source(request)
        except PackInspectionError as error:
            return RuntimeFailure(error=error.diagnostic)
        except (Exception, SystemExit) as error:
            return RuntimeFailure(
                error=diagnostic_from_exception(
                    error,
                    request.pack_path,
                    message="Source materialization failed",
                    help="Correct the Source, arguments, or table selection, then retry",
                )
            )
        return RuntimeSuccess(result=result)

    if isinstance(request, QueryDatasetRequest):
        try:
            result = query_dataset(request)
        except (Exception, SystemExit) as error:
            return RuntimeFailure(
                error=diagnostic_from_exception(
                    error,
                    None,
                    message="Dataset query failed",
                    help="Correct the SQL, Dataset Sources, or PACK discovery, then retry",
                )
            )
        return RuntimeSuccess(result=result)

    if isinstance(request, TestPackRequest):
        try:
            if test_report_path is None:
                raise RuntimeRequestError("test_pack requires a private test report path")
            result = test_pack(request, test_report_path)
        except (Exception, SystemExit) as error:
            if isinstance(error, PytestExitError):
                return RuntimeFailure(error={"message": error.message(), "help": error.help()})
            return RuntimeFailure(
                error=diagnostic_from_exception(
                    error,
                    request.pack_path,
                    message="PACK test Runtime failed",
                    help="Inspect the pytest terminal report and Operation log, correct the PACK, and retry",
                )
            )
        return RuntimeSuccess(result=result)

    try:
        result = run_workflow(request)
    except _PackInspectionWorkerError:
        raise
    except PackInspectionError as error:
        return RuntimeFailure(error=error.diagnostic)
    except (Exception, SystemExit) as error:
        return RuntimeFailure(
            error=diagnostic_from_exception(
                error,
                request.pack_path,
                message="Workflow execution failed",
                help="Correct the Workflow, arguments, or Dataset and retry the complete Run",
                private_values=_private_run_values(request),
            )
        )
    return RuntimeSuccess(result=result)


def _private_run_values(request: RunWorkflowRequest) -> tuple[str, ...]:
    candidate_path = request.candidate.path
    return (
        request.candidate.identifier,
        str(candidate_path),
        candidate_path.as_posix(),
    )


def _write_response(path: Path, response: RuntimeResponse) -> None:
    with path.open("x", encoding="utf-8", newline="\n") as file:
        json.dump(
            asdict(response),
            file,
            ensure_ascii=False,
            separators=(",", ":"),
            allow_nan=False,
        )
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


if __name__ == "__main__":
    raise SystemExit(main())
