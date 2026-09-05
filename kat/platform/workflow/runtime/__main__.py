from __future__ import annotations

import argparse
from contextlib import contextmanager
from dataclasses import asdict, dataclass, field
import json
import os
from pathlib import Path
import sys
import traceback
from typing import Iterator, Literal

from .diagnostic import RuntimeDiagnostic, diagnostic_from_exception
from .execution import RunWorkflowRuntimeResult, run_workflow
from .pack import (
    InspectWorkflowRuntimeResult,
    PackInspectionError,
    _PackInspectionWorkerError,
    inspect_workflow,
)
from .provider_inspection import (
    InspectProviderRuntimeResult,
    ProviderInspectionError,
    inspect_provider,
)
from .query import QueryRunRuntimeResult, query_run
from .request import (
    InspectProviderRequest,
    InspectWorkflowRequest,
    QueryRunRequest,
    RunWorkflowRequest,
    RuntimeRequest,
    RuntimeRequestError,
    TestPackRequest,
    read_request,
)
from .rpc import _NestedRunClient
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
    RuntimeSuccess[InspectWorkflowRuntimeResult]
    | RuntimeSuccess[InspectProviderRuntimeResult]
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
        test_report_path = (
            Path(arguments.test_report) if arguments.test_report is not None else None
        )
        if isinstance(request, (RunWorkflowRequest, TestPackRequest)):
            with _nested_run_standard_streams() as nested_runs:
                response = _execute(request, test_report_path, nested_runs=nested_runs)
                _write_response(response_path, response)
            return 0
        response = _execute(request, test_report_path)
    _write_response(response_path, response)
    return 0


def _execute(
    request: RuntimeRequest,
    test_report_path: Path | None = None,
    *,
    nested_runs: _NestedRunClient | None = None,
) -> RuntimeResponse:
    if isinstance(request, InspectWorkflowRequest):
        try:
            result = inspect_workflow(
                request.pack_name,
                request.pack_path,
                request.workflow_name,
            )
        except PackInspectionError as error:
            return RuntimeFailure(error=error.diagnostic)
        return RuntimeSuccess(result=result)

    if isinstance(request, InspectProviderRequest):
        try:
            result = inspect_provider(
                request.pack_name,
                request.pack_path,
                request.provider_name,
            )
        except ProviderInspectionError as error:
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

    if isinstance(request, TestPackRequest):
        try:
            if test_report_path is None:
                raise RuntimeRequestError("test_pack requires a private test report path")
            if nested_runs is None:
                raise RuntimeRequestError("test_pack requires a private Host control channel")
            result = test_pack(request, test_report_path, nested_runs)
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
        result = run_workflow(request, nested_runs)
    except _PackInspectionWorkerError:
        raise
    except PackInspectionError as error:
        return RuntimeFailure(error=error.diagnostic)
    except (Exception, SystemExit) as error:
        traceback.print_exception(error, file=sys.stderr)
        return RuntimeFailure(
            error=diagnostic_from_exception(
                error,
                request.pack_path,
                message="Workflow execution failed",
                help="Correct the Workflow or arguments and retry the complete Run",
                private_values=_private_run_values(request),
            )
        )
    return RuntimeSuccess(result=result)


@contextmanager
def _nested_run_standard_streams() -> Iterator[_NestedRunClient]:
    """Reserve stdin/stdout for RPC while PACK-visible streams remain diagnostic."""
    sys.stdout.flush()
    sys.stderr.flush()
    protocol_input_fd = os.dup(0)
    protocol_output_fd = os.dup(1)
    protocol_reader = os.fdopen(protocol_input_fd, "rb", buffering=0)
    protocol_writer = os.fdopen(protocol_output_fd, "wb", buffering=0)
    null_input_fd = os.open(os.devnull, os.O_RDONLY)
    try:
        os.dup2(null_input_fd, 0)
    finally:
        os.close(null_input_fd)
    os.dup2(2, 1)
    nested_runs = _NestedRunClient(protocol_reader, protocol_writer)
    try:
        yield nested_runs
    finally:
        reader_is_quiescent = nested_runs.close()
        protocol_writer.close()
        if reader_is_quiescent:
            protocol_reader.close()


def _private_run_values(request: RunWorkflowRequest) -> tuple[str, ...]:
    candidate_path = request.candidate.path
    datasource_root = request.datasource_root
    scratch_root = request.scratch_root
    session_root = candidate_path.parent.parent
    return (
        request.candidate.identifier,
        session_root.name,
        str(candidate_path),
        candidate_path.as_posix(),
        str(datasource_root),
        datasource_root.as_posix(),
        str(scratch_root),
        scratch_root.as_posix(),
        str(session_root),
        session_root.as_posix(),
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
