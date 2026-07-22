from __future__ import annotations

import inspect
import math
import re
import types
import typing
from dataclasses import dataclass
from typing import Any, Literal, get_args, get_origin

import click
import kat
from kat._workflow import _normalize_required_tables


_WORKFLOW_NAME = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*\Z")
_MISSING = inspect.Signature.empty


class _FiniteFloat(click.ParamType):
    name = "float"

    def convert(self, value: object, param: click.Parameter | None, ctx: click.Context | None) -> float:
        try:
            converted = float(value)
        except (TypeError, ValueError):
            self.fail(f"{value!r} is not a floating-point value", param, ctx)
        if not math.isfinite(converted):
            self.fail(f"{value!r} is not finite", param, ctx)
        return converted


class _DurationType(click.ParamType):
    name = "duration"

    def convert(self, value: object, param: click.Parameter | None, ctx: click.Context | None) -> kat.Duration:
        if isinstance(value, kat.Duration):
            return value
        try:
            return kat.Duration(value)  # type: ignore[arg-type]
        except (TypeError, ValueError) as error:
            self.fail(str(error), param, ctx)


class _WallClockType(click.ParamType):
    name = "wall-clock timestamp"

    def convert(self, value: object, param: click.Parameter | None, ctx: click.Context | None) -> kat.WallClockTimestamp:
        if isinstance(value, kat.WallClockTimestamp):
            return value
        try:
            return kat.WallClockTimestamp(value)  # type: ignore[arg-type]
        except (TypeError, ValueError) as error:
            self.fail(str(error), param, ctx)


@dataclass(frozen=True)
class CompiledWorkflow:
    function: typing.Callable[..., Any]
    interface: dict[str, Any]
    command: click.Command

    def parse_arguments(self, arguments: typing.Sequence[str]) -> dict[str, Any]:
        try:
            context = self.command.make_context(
                self.interface["name"], list(arguments), resilient_parsing=False
            )
        except click.ClickException as error:
            raise ValueError(error.format_message()) from error
        try:
            return {
                parameter["name"]: context.params[parameter["name"]]
                for parameter in self.interface["parameters"]
            }
        finally:
            context.close()


def inspect_declared_workflow(function: typing.Callable[..., Any]) -> dict[str, Any]:
    return compile_declared_workflow(function).interface


def compile_declared_workflow(function: typing.Callable[..., Any]) -> CompiledWorkflow:
    if (
        not inspect.isfunction(function)
        or function.__name__ == "<lambda>"
        or function.__qualname__ != function.__name__
    ):
        raise ValueError("Workflow must be a module-top-level function")
    if inspect.iscoroutinefunction(function) or inspect.isgeneratorfunction(function) or inspect.isasyncgenfunction(function):
        raise ValueError("Workflow must be a plain synchronous function")
    declaration = getattr(function, "__kat_workflow__", None)
    if declaration is None:
        raise ValueError("Workflow function is missing @kat.workflow(...)")
    if _WORKFLOW_NAME.fullmatch(declaration.name) is None:
        raise ValueError(f"invalid Workflow name: {declaration.name!r}")
    required_tables = list(_normalize_required_tables(declaration.required_tables))

    description = inspect.cleandoc(function.__doc__ or "").strip()
    if not description:
        raise ValueError("Workflow docstring must not be empty")
    try:
        import annotationlib
    except ImportError:
        signature = inspect.signature(function, eval_str=False)
    else:
        signature = inspect.signature(
            function,
            eval_str=False,
            annotation_format=annotationlib.Format.FORWARDREF,
        )
    parameters = list(signature.parameters.values())
    if not parameters:
        raise ValueError("Workflow must start with ctx")
    ctx = parameters[0]
    if (
        ctx.name != "ctx"
        or ctx.kind is not inspect.Parameter.POSITIONAL_OR_KEYWORD
        or ctx.default is not _MISSING
        or _resolve_annotation(function, ctx) is not kat.Context
    ):
        raise ValueError("first Workflow parameter must be ctx: kat.Context")

    user_parameters = parameters[1:]
    descriptions = {} if declaration.parameters is None else dict(declaration.parameters)
    expected_names = {parameter.name for parameter in user_parameters}
    if set(descriptions) != expected_names:
        missing = sorted(expected_names - set(descriptions))
        unknown = sorted(set(descriptions) - expected_names)
        raise ValueError(f"Workflow parameter descriptions do not match; missing={missing}, unknown={unknown}")
    for name, description_text in descriptions.items():
        if not description_text.strip():
            raise ValueError(f"Workflow parameter {name!r} description must not be empty")

    options: list[click.Option] = []
    projections: list[dict[str, Any]] = []
    for parameter in user_parameters:
        if parameter.kind not in (
            inspect.Parameter.POSITIONAL_OR_KEYWORD,
            inspect.Parameter.KEYWORD_ONLY,
        ):
            raise ValueError(f"Workflow parameter {parameter.name!r} has an unsupported kind")
        annotation = _resolve_annotation(function, parameter)
        option, projection = _compile_parameter(parameter, annotation, descriptions[parameter.name].strip())
        options.append(option)
        projections.append(projection)

    command = click.Command(
        name=declaration.name,
        params=options,
        add_help_option=False,
        no_args_is_help=False,
    )
    context = click.Context(command)
    for option, projection in zip(options, projections, strict=True):
        if not projection["required"]:
            try:
                effective = option.process_value(context, option.get_default(context, call=True))
            except click.ClickException as error:
                raise ValueError(
                    f"Workflow parameter {projection['name']!r} default is invalid: {error.format_message()}"
                ) from error
            projection["default"] = _project_default(projection["type"], effective)

    return CompiledWorkflow(
        function=function,
        interface={
            "name": declaration.name,
            "title": declaration.title,
            "description": description,
            "required_tables": required_tables,
            "parameters": projections,
        },
        command=command,
    )


def _resolve_annotation(function: typing.Callable[..., Any], parameter: inspect.Parameter) -> object:
    if parameter.annotation is _MISSING:
        raise ValueError(f"Workflow parameter {parameter.name!r} is missing an annotation")
    annotation = parameter.annotation
    try:
        import annotationlib
    except ImportError:
        if not isinstance(annotation, str):
            return annotation
        try:
            return eval(annotation, function.__globals__, {})
        except Exception as error:
            raise ValueError(f"cannot resolve Workflow parameter {parameter.name!r} annotation") from error
    try:
        if isinstance(annotation, str):
            annotation = typing.ForwardRef(annotation)
        if isinstance(annotation, annotationlib.ForwardRef):
            return typing.evaluate_forward_ref(
                annotation, owner=function, globals=function.__globals__
            )
        return annotation
    except Exception as error:
        raise ValueError(f"cannot resolve Workflow parameter {parameter.name!r} annotation") from error


def _compile_parameter(
    parameter: inspect.Parameter, annotation: object, description: str
) -> tuple[click.Option, dict[str, Any]]:
    optional = False
    origin = get_origin(annotation)
    if origin in (types.UnionType, typing.Union):
        arguments = get_args(annotation)
        non_none = [item for item in arguments if item is not type(None)]
        if len(arguments) != 2 or len(non_none) != 1 or non_none[0] is bool:
            raise ValueError(f"Workflow parameter {parameter.name!r} has an unsupported union")
        if parameter.default is not None:
            raise ValueError(f"optional Workflow parameter {parameter.name!r} must default to None")
        annotation = non_none[0]
        optional = True
        origin = get_origin(annotation)

    if parameter.default is None and not optional:
        raise ValueError(
            f"Workflow parameter {parameter.name!r} can default to None only when annotated T | None"
        )

    choices: list[str] | None = None
    if origin is Literal:
        literal_values = get_args(annotation)
        if not literal_values or any(type(value) is not str for value in literal_values):
            raise ValueError(f"Workflow parameter {parameter.name!r} Literal must contain strings")
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
            raise ValueError(f"boolean Workflow parameter {parameter.name!r} requires a default")
        click_type = click.BOOL
        public_type = "boolean"
    elif annotation is kat.Duration:
        click_type = _DurationType()
        public_type = "duration"
    elif annotation is kat.WallClockTimestamp:
        click_type = _WallClockType()
        public_type = "wall_clock_timestamp"
    else:
        raise ValueError(f"Workflow parameter {parameter.name!r} has an unsupported annotation")

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
    projection: dict[str, Any] = {
        "name": parameter.name,
        "option": option_name,
        "type": public_type,
        "required": required,
        "description": description,
    }
    if annotation is bool:
        negative = "--no-" + parameter.name.replace("_", "-")
        declarations = [f"{option_name}/{negative}", parameter.name]
        option_arguments["is_flag"] = True
        projection["negative_option"] = negative
    if choices is not None:
        projection["choices"] = choices
    return click.Option(declarations, **option_arguments), projection


def _project_default(public_type: str, value: object) -> object:
    if value is None:
        return None
    if public_type == "int64":
        return str(value)
    if public_type == "float64":
        return float(value)  # type: ignore[arg-type]
    if public_type == "boolean":
        return bool(value)
    if public_type in ("duration", "wall_clock_timestamp"):
        return str(value)
    return value
