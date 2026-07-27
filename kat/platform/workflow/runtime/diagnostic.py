from __future__ import annotations

from pathlib import Path
from typing import Iterator, NotRequired, TypedDict


class DiagnosticPosition(TypedDict):
    line: int
    column: int


class DiagnosticLocation(TypedDict):
    source: str
    start: DiagnosticPosition
    end: DiagnosticPosition


class RuntimeDiagnostic(TypedDict):
    message: str
    help: str
    causes: NotRequired[list[str]]
    location: NotRequired[DiagnosticLocation]


def diagnostic_from_exception(
    error: BaseException,
    pack_path: Path | None,
    *,
    message: str,
    help: str,
    private_values: tuple[str, ...] = (),
) -> RuntimeDiagnostic:
    causes: list[str] = []
    for current in _exception_chain(error):
        try:
            rendered = str(current).strip()
        except BaseException:
            continue
        if rendered:
            for private in sorted(
                (value for value in private_values if value),
                key=len,
                reverse=True,
            ):
                rendered = rendered.replace(private, "<private>")
            causes.append(rendered)
    diagnostic: RuntimeDiagnostic = {"message": message, "help": help}
    if causes:
        diagnostic["causes"] = causes
    try:
        location = _syntax_error_location(error, pack_path)
    except BaseException:
        location = None
    if location is not None:
        diagnostic["location"] = location
    return diagnostic


def _syntax_error_location(
    error: BaseException, pack_path: Path | None
) -> DiagnosticLocation | None:
    syntax_error = next(
        (current for current in _exception_chain(error) if isinstance(current, SyntaxError)),
        None,
    )
    if syntax_error is None or pack_path is None or syntax_error.filename is None:
        return None
    positions = (
        syntax_error.lineno,
        syntax_error.offset,
        syntax_error.end_lineno,
        syntax_error.end_offset,
    )
    if any(type(value) is not int or value <= 0 for value in positions):
        return None
    start_line, start_column, end_line, end_column = positions
    if (end_line, end_column) < (start_line, start_column):
        return None
    try:
        root = pack_path.resolve(strict=True)
        relative = Path(syntax_error.filename).resolve(strict=True).relative_to(root)
    except (OSError, ValueError):
        return None
    if relative == Path():
        return None
    return {
        "source": relative.as_posix(),
        "start": {"line": start_line, "column": start_column},
        "end": {"line": end_line, "column": end_column},
    }


def _exception_chain(error: BaseException) -> Iterator[BaseException]:
    seen: set[int] = set()
    current: BaseException | None = error
    while current is not None and id(current) not in seen:
        seen.add(id(current))
        yield current
        cause = BaseException.__cause__.__get__(current, BaseException)
        if cause is not None:
            current = cause
        elif BaseException.__suppress_context__.__get__(current, BaseException):
            current = None
        else:
            current = BaseException.__context__.__get__(current, BaseException)
