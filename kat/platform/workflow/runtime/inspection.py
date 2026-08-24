from __future__ import annotations

import inspect
import math
import os
from pathlib import Path
import re
import types
import typing
from dataclasses import dataclass
from typing import Any, Literal, NotRequired, TypedDict, get_args, get_origin

import click
import kat
from kat._identifiers import valid_source_name


_WORKFLOW_NAME = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")
_MISSING = inspect.Signature.empty

ParameterDefault = str | float | bool | None | list[str]


class _ParameterProjection(TypedDict):
    name: str
    option: str
    type: str
    required: bool
    negative_option: NotRequired[str]
    choices: NotRequired[list[str]]
    repeatable: NotRequired[bool]
    default: NotRequired[ParameterDefault]


class WorkflowParameter(_ParameterProjection):
    description: str


class SourceParameter(_ParameterProjection):
    pass


class WorkflowInputInterface(TypedDict):
    name: str
    title: str
    description: str
    parameters: list[WorkflowParameter]


class SourceInputInterface(TypedDict):
    name: str
    parameters: list[SourceParameter]


class _FiniteFloat(click.ParamType):
    name = "float"

    def convert(
        self,
        value: object,
        param: click.Parameter | None,
        ctx: click.Context | None,
    ) -> float:
        try:
            converted = float(value)
        except (TypeError, ValueError):
            self.fail(f"{value!r} is not a floating-point value", param, ctx)
        if not math.isfinite(converted):
            self.fail(f"{value!r} is not finite", param, ctx)
        return converted


class _DurationType(click.ParamType):
    name = "duration"

    def convert(
        self,
        value: object,
        param: click.Parameter | None,
        ctx: click.Context | None,
    ) -> kat.Duration:
        if isinstance(value, kat.Duration):
            return value
        try:
            return kat.Duration(value)  # type: ignore[arg-type]
        except (TypeError, ValueError) as error:
            self.fail(str(error), param, ctx)


class _WallClockType(click.ParamType):
    name = "wall-clock timestamp"

    def convert(
        self,
        value: object,
        param: click.Parameter | None,
        ctx: click.Context | None,
    ) -> kat.WallClockTimestamp:
        if isinstance(value, kat.WallClockTimestamp):
            return value
        try:
            return kat.WallClockTimestamp(value)  # type: ignore[arg-type]
        except (TypeError, ValueError) as error:
            self.fail(str(error), param, ctx)


@dataclass(frozen=True)
class CompiledWorkflow:
    function: typing.Callable[..., Any]
    interface: WorkflowInputInterface
    command: click.Command

    def parse_arguments(self, arguments: typing.Sequence[str]) -> dict[str, Any]:
        return _parse_arguments(self.command, self.interface, arguments)


@dataclass(frozen=True)
class CompiledSource:
    function: typing.Callable[..., Any]
    interface: SourceInputInterface
    command: click.Command

    def parse_arguments(
        self,
        arguments: typing.Sequence[str],
        *,
        argument_base: Path,
    ) -> dict[str, Any]:
        if not isinstance(argument_base, Path) or not argument_base.is_absolute():
            raise ValueError("Source argument_base must be an absolute Path")
        parsed = _parse_arguments(self.command, self.interface, arguments)
        for parameter in self.interface["parameters"]:
            if parameter["type"] != "path":
                continue
            name = parameter["name"]
            value = parsed[name]
            if parameter.get("repeatable", False):
                parsed[name] = tuple(
                    _lexical_absolute(item, argument_base) for item in value
                )
            elif value is not None:
                parsed[name] = _lexical_absolute(value, argument_base)
        return parsed


def inspect_declared_workflow(
    function: typing.Callable[..., Any],
) -> WorkflowInputInterface:
    return compile_declared_workflow(function).interface


def inspect_declared_source(
    function: typing.Callable[..., Any],
) -> SourceInputInterface:
    return compile_declared_source(function).interface


def compile_declared_workflow(function: typing.Callable[..., Any]) -> CompiledWorkflow:
    _validate_plain_entry(function, "Workflow")
    declaration = getattr(function, "__kat_workflow__", None)
    if declaration is None:
        raise ValueError("Workflow function is missing @kat.workflow(...)")
    if _WORKFLOW_NAME.fullmatch(declaration.name) is None:
        raise ValueError(f"invalid Workflow name: {declaration.name!r}")

    description = inspect.cleandoc(function.__doc__ or "").strip()
    if not description:
        raise ValueError("Workflow docstring must not be empty")
    parameters = list(_safe_signature(function, "Workflow").parameters.values())
    if not parameters:
        raise ValueError("Workflow must start with ctx")
    ctx = parameters[0]
    if (
        ctx.name != "ctx"
        or ctx.kind is not inspect.Parameter.POSITIONAL_OR_KEYWORD
        or ctx.default is not _MISSING
        or _resolve_annotation(function, ctx, "Workflow") is not kat.Context
    ):
        raise ValueError("first Workflow parameter must be ctx: kat.Context")

    user_parameters = parameters[1:]
    descriptions = {} if declaration.parameters is None else dict(declaration.parameters)
    expected_names = {parameter.name for parameter in user_parameters}
    if set(descriptions) != expected_names:
        missing = sorted(expected_names - set(descriptions))
        unknown = sorted(set(descriptions) - expected_names)
        raise ValueError(
            "Workflow parameter descriptions do not match; "
            f"missing={missing}, unknown={unknown}"
        )

    options: list[click.Option] = []
    projections: list[WorkflowParameter] = []
    for parameter in user_parameters:
        _validate_parameter_kind(parameter, "Workflow")
        description_text = descriptions[parameter.name].strip()
        if not description_text:
            raise ValueError(
                f"Workflow parameter {parameter.name!r} description must not be empty"
            )
        option, projection = _compile_parameter(
            parameter,
            _resolve_annotation(function, parameter, "Workflow"),
            owner="Workflow",
            allow_path=False,
        )
        workflow_projection = typing.cast(WorkflowParameter, projection)
        workflow_projection["description"] = description_text
        options.append(option)
        projections.append(workflow_projection)

    command = _command(declaration.name, options)
    _project_defaults(command, options, projections, "Workflow")
    return CompiledWorkflow(
        function=function,
        interface={
            "name": declaration.name,
            "title": declaration.title,
            "description": description,
            "parameters": projections,
        },
        command=command,
    )


def compile_declared_source(function: typing.Callable[..., Any]) -> CompiledSource:
    _validate_plain_entry(function, "Source")
    declaration = getattr(function, "__kat_source__", None)
    if declaration is None:
        raise ValueError("Source function is missing @kat.source(...)")
    if declaration.name == "information_schema":
        raise ValueError(
            "information_schema 是 DataFusion 保留的系统 schema，不能用作 Source name"
        )
    if not valid_source_name(declaration.name):
        raise ValueError(f"invalid Source name: {declaration.name!r}")

    parameters = list(_safe_signature(function, "Source").parameters.values())
    options: list[click.Option] = []
    projections: list[SourceParameter] = []
    for parameter in parameters:
        _validate_parameter_kind(parameter, "Source")
        option, projection = _compile_parameter(
            parameter,
            _resolve_annotation(function, parameter, "Source"),
            owner="Source",
            allow_path=True,
        )
        options.append(option)
        projections.append(typing.cast(SourceParameter, projection))

    command = _command(declaration.name, options)
    _project_defaults(command, options, projections, "Source")
    return CompiledSource(
        function=function,
        interface={"name": declaration.name, "parameters": projections},
        command=command,
    )


def _validate_plain_entry(function: object, owner: str) -> None:
    if (
        not inspect.isfunction(function)
        or function.__name__ == "<lambda>"
        or function.__qualname__ != function.__name__
    ):
        raise ValueError(f"{owner} must be a module-top-level function")
    if (
        inspect.iscoroutinefunction(function)
        or inspect.isgeneratorfunction(function)
        or inspect.isasyncgenfunction(function)
    ):
        raise ValueError(f"{owner} must be a plain synchronous function")


def _safe_signature(
    function: typing.Callable[..., Any], owner: str
) -> inspect.Signature:
    try:
        import annotationlib
    except ImportError:
        return inspect.signature(function, eval_str=False)
    try:
        return inspect.signature(
            function,
            eval_str=False,
            annotation_format=annotationlib.Format.STRING,
        )
    except (Exception, SystemExit) as error:
        raise ValueError(f"cannot inspect {owner} annotations") from error


def _resolve_annotation(
    function: typing.Callable[..., Any],
    parameter: inspect.Parameter,
    owner: str,
) -> object:
    if parameter.annotation is _MISSING:
        raise ValueError(f"{owner} parameter {parameter.name!r} is missing an annotation")

    def annotation_holder() -> None:
        pass

    annotation_holder.__annotations__ = {parameter.name: parameter.annotation}
    try:
        return typing.get_type_hints(
            annotation_holder,
            globalns=function.__globals__,
            localns={},
            include_extras=True,
        )[parameter.name]
    except (Exception, SystemExit) as error:
        raise ValueError(
            f"cannot resolve {owner} parameter {parameter.name!r} annotation"
        ) from error


def _validate_parameter_kind(parameter: inspect.Parameter, owner: str) -> None:
    if parameter.kind not in (
        inspect.Parameter.POSITIONAL_OR_KEYWORD,
        inspect.Parameter.KEYWORD_ONLY,
    ):
        raise ValueError(f"{owner} parameter {parameter.name!r} has an unsupported kind")


def _compile_parameter(
    parameter: inspect.Parameter,
    annotation: object,
    *,
    owner: str,
    allow_path: bool,
) -> tuple[click.Option, _ParameterProjection]:
    optional = False
    origin = get_origin(annotation)
    if origin in (types.UnionType, typing.Union):
        arguments = get_args(annotation)
        non_none = [item for item in arguments if item is not type(None)]
        if len(arguments) != 2 or len(non_none) != 1 or non_none[0] is bool:
            raise ValueError(f"{owner} parameter {parameter.name!r} has an unsupported union")
        if parameter.default is not None:
            raise ValueError(
                f"optional {owner} parameter {parameter.name!r} must default to None"
            )
        annotation = non_none[0]
        optional = True
        origin = get_origin(annotation)

    if parameter.default is None and not optional:
        raise ValueError(
            f"{owner} parameter {parameter.name!r} can default to None only when annotated T | None"
        )

    choices: list[str] | None = None
    repeatable = False
    if origin is Literal:
        literal_values = get_args(annotation)
        if not literal_values or any(type(value) is not str for value in literal_values):
            raise ValueError(
                f"{owner} parameter {parameter.name!r} Literal must contain strings"
            )
        choices = sorted(set(literal_values))
        click_type: click.ParamType = click.Choice(choices, case_sensitive=True)
        public_type = "string"
    elif annotation is str:
        click_type = click.STRING
        public_type = "string"
    elif annotation is int:
        click_type = click.IntRange(-(2**63), 2**63 - 1)
        public_type = "int64"
    elif annotation is float:
        click_type = _FiniteFloat()
        public_type = "float64"
    elif annotation is bool:
        if parameter.default is _MISSING:
            raise ValueError(f"boolean {owner} parameter {parameter.name!r} requires a default")
        click_type = click.BOOL
        public_type = "boolean"
    elif annotation is kat.Duration:
        click_type = _DurationType()
        public_type = "duration"
    elif annotation is kat.WallClockTimestamp:
        click_type = _WallClockType()
        public_type = "wall_clock_timestamp"
    elif allow_path and annotation is Path:
        click_type = click.Path(
            path_type=Path,
            exists=False,
            resolve_path=False,
            allow_dash=True,
        )
        public_type = "path"
    elif allow_path and origin is tuple and get_args(annotation) == (Path, Ellipsis):
        if optional:
            raise ValueError(
                f"optional {owner} parameter {parameter.name!r} cannot be a repeated Path"
            )
        click_type = click.Path(
            path_type=Path,
            exists=False,
            resolve_path=False,
            allow_dash=True,
        )
        public_type = "path"
        repeatable = True
    else:
        raise ValueError(f"{owner} parameter {parameter.name!r} has an unsupported annotation")

    required = parameter.default is _MISSING and not optional
    option_name = "--" + parameter.name.replace("_", "-")
    declarations = [option_name, parameter.name]
    option_arguments: dict[str, Any] = {
        "type": click_type,
        "required": required,
        "show_default": False,
    }
    if parameter.default is not _MISSING:
        option_arguments["default"] = parameter.default
    if repeatable:
        option_arguments["multiple"] = True
    projection: _ParameterProjection = {
        "name": parameter.name,
        "option": option_name,
        "type": public_type,
        "required": required,
    }
    if annotation is bool:
        negative = "--no-" + parameter.name.replace("_", "-")
        declarations = [f"{option_name}/{negative}", parameter.name]
        option_arguments["is_flag"] = True
        projection["negative_option"] = negative
    if choices is not None:
        projection["choices"] = choices
    if repeatable:
        projection["repeatable"] = True
    return click.Option(declarations, **option_arguments), projection


def _command(name: str, options: list[click.Option]) -> click.Command:
    return click.Command(
        name=name,
        params=options,
        add_help_option=False,
        no_args_is_help=False,
    )


def _project_defaults(
    command: click.Command,
    options: list[click.Option],
    projections: list[_ParameterProjection],
    owner: str,
) -> None:
    context = click.Context(command)
    for option, projection in zip(options, projections, strict=True):
        if projection["required"]:
            continue
        try:
            effective = option.type_cast_value(context, option.get_default(context, call=True))
        except (Exception, SystemExit) as error:
            detail = (
                error.format_message()
                if isinstance(error, click.ClickException)
                else str(error)
            )
            raise ValueError(
                f"{owner} parameter {projection['name']!r} default is invalid: "
                f"{detail or type(error).__name__}"
            ) from error
        projection["default"] = _project_default(
            projection["type"],
            effective,
            repeatable=projection.get("repeatable", False),
        )


def _parse_arguments(
    command: click.Command,
    interface: WorkflowInputInterface | SourceInputInterface,
    arguments: typing.Sequence[str],
) -> dict[str, Any]:
    try:
        context = command.make_context(
            interface["name"], list(arguments), resilient_parsing=False
        )
    except click.ClickException as error:
        raise ValueError(error.format_message()) from error
    try:
        return {
            parameter["name"]: context.params[parameter["name"]]
            for parameter in interface["parameters"]
        }
    finally:
        context.close()


def _project_default(
    public_type: str,
    value: object,
    *,
    repeatable: bool,
) -> ParameterDefault:
    if value is None:
        return None
    if repeatable:
        return [str(item) for item in typing.cast(tuple[Path, ...], value)]
    if public_type == "int64":
        return str(value)
    if public_type == "float64":
        return float(value)  # type: ignore[arg-type]
    if public_type == "boolean":
        return bool(value)
    if public_type in ("duration", "wall_clock_timestamp", "path"):
        return str(value)
    return typing.cast(str, value)


def _lexical_absolute(value: Path, argument_base: Path) -> Path:
    if not isinstance(value, Path):
        raise TypeError("Source Path compiler produced a non-Path value")
    combined = value if value.is_absolute() else argument_base / value
    return Path(os.path.normpath(os.fspath(combined)))
