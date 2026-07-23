from __future__ import annotations

import inspect
import unittest
from typing import Annotated, Any, Literal, Optional, Union

import kat
from _kat_runtime.inspection import compile_declared_workflow, inspect_declared_workflow


@kat.workflow(
    name="thread-time",
    title=" Thread time ",
    required_tables=["sched_slice", "thread", "sched_slice"],
    parameters={
        "label": " Label filter ",
        "count": "Signed count",
        "ratio": "Finite ratio",
        "enabled": "Include idle",
        "window": "Elapsed window",
        "at": "Wall-clock boundary",
        "mode": "Aggregation mode",
        "optional_label": "Optional label",
    },
)
def analyze(
    ctx: kat.Context,
    label: str = "",
    count: int = 7,
    ratio: float = 1,
    enabled: bool = False,
    window: kat.Duration = "5ms",
    at: kat.WallClockTimestamp = "2026-07-14T16:30:00+08:00",
    mode: Literal["sum", "mean", "sum"] = "sum",
    optional_label: str | None = None,
) -> None:
    """Inspect thread CPU time.

    Internal whitespace remains.
    """


@kat.workflow(name="return-is-not-input", title="Return", required_tables=[])
def unresolved_return(ctx: kat.Context) -> UndefinedReturn:
    """The Input Compiler must not evaluate the return annotation."""


mutable_tables = ["thread"]
mutable_descriptions = {"value": " Original description "}


@kat.workflow(
    name="copied-declaration",
    title="Copied declaration",
    required_tables=mutable_tables,
    parameters=mutable_descriptions,
)
def copied_declaration(ctx: kat.Context, value: str) -> None:
    """The decorator owns an immutable declaration snapshot."""


mutable_tables.append("sched_slice")
mutable_descriptions["value"] = "Mutated"


lambda_workflow = lambda ctx: None
lambda_workflow.__annotations__ = {"ctx": kat.Context}
lambda_workflow.__doc__ = "A lambda is not a declared Workflow function."
lambda_workflow = kat.workflow(
    name="lambda-workflow", title="Lambda", required_tables=[]
)(lambda_workflow)


@kat.workflow(
    name="asynchronous",
    title="Asynchronous",
    required_tables=[],
    parameters={"value": "Value"},
)
async def asynchronous(ctx: kat.Context, value: str) -> None:
    """Not a synchronous Workflow."""


@kat.workflow(
    name="missing-description",
    title="Missing description",
    required_tables=[],
    parameters={},
)
def missing_parameter_description(ctx: kat.Context, value: str) -> None:
    """Descriptions must match."""


@kat.workflow(
    name="required-bool",
    title="Required bool",
    required_tables=[],
    parameters={"flag": "Flag"},
)
def required_bool(ctx: kat.Context, flag: bool) -> None:
    """Bool requires a default."""


@kat.workflow(
    name="invalid-bool-default",
    title="Invalid bool default",
    required_tables=[],
    parameters={"flag": "Flag"},
)
def invalid_bool_default(ctx: kat.Context, flag: bool = 1) -> None:  # type: ignore[assignment]
    """Invalid Click defaults remain PACK authoring failures."""


@kat.workflow(
    name="overflowing-int-default",
    title="Overflowing int default",
    required_tables=[],
    parameters={"count": "Count"},
)
def overflowing_int_default(ctx: kat.Context, count: int = float("inf")) -> None:  # type: ignore[assignment]
    """Overflowing numeric conversion remains a PACK authoring failure."""


@kat.workflow(
    name="none-without-optional",
    title="None without optional",
    required_tables=[],
    parameters={"value": "Value"},
)
def none_without_optional(
    ctx: kat.Context, value: str = None  # type: ignore[assignment]
) -> None:
    """None only belongs to the Optional contract."""


@kat.workflow(
    name="unsupported-any",
    title="Unsupported Any",
    required_tables=[],
    parameters={"value": "Value"},
)
def unsupported_any(ctx: kat.Context, value: Any) -> None:
    """Any is outside the closed type set."""


@kat.workflow(
    name="unsupported-annotated",
    title="Unsupported Annotated",
    required_tables=[],
    parameters={"value": "Value"},
)
def unsupported_annotated(ctx: kat.Context, value: Annotated[str, "metadata"]) -> None:
    """Typing extras are outside the closed authoring type set."""


@kat.workflow(
    name="overflowing-wall-clock",
    title="Overflowing wall clock",
    required_tables=[],
    parameters={"at": "Boundary"},
)
def overflowing_wall_clock(
    ctx: kat.Context,
    at: kat.WallClockTimestamp = "0001-01-01T00:00:00+23:59",
) -> None:
    """The UTC conversion must remain inside the supported datetime range."""


@kat.workflow(
    name="unknown-wall-clock-offset",
    title="Unknown wall clock offset",
    required_tables=[],
    parameters={"at": "Boundary"},
)
def unknown_wall_clock_offset(
    ctx: kat.Context,
    at: kat.WallClockTimestamp = "2026-07-14T08:30:00-00:00",
) -> None:
    """A wall-clock default must identify a known UTC offset."""


@kat.workflow(
    name="legacy-optional",
    title="Legacy Optional",
    required_tables=[],
    parameters={"value": "Value"},
)
def legacy_optional(ctx: kat.Context, value: Optional[str] = None) -> None:
    """typing.Optional resolves to the supported optional type."""


@kat.workflow(
    name="legacy-union",
    title="Legacy Union",
    required_tables=[],
    parameters={"value": "Value"},
)
def legacy_union(ctx: kat.Context, value: Union[str, None] = None) -> None:
    """typing.Union resolves to the supported optional type."""


@kat.workflow(
    name="nested-forward-reference",
    title="Nested forward reference",
    required_tables=[],
    parameters={"value": "Value"},
)
def nested_forward_reference(
    ctx: kat.Context, value: Optional["str"] = None
) -> None:
    """Nested ForwardRefs resolve through the standard typing evaluator."""


@kat.workflow(
    name="required-string",
    title="Required string",
    required_tables=[],
    parameters={"query": "Query text"},
)
def required_string(ctx: kat.Context, query: str) -> None:
    """Required values omit their default."""


class AuthoringApiTest(unittest.TestCase):
    def test_public_decorator_documents_the_authoring_contract(self) -> None:
        documentation = inspect.getdoc(kat.workflow)
        self.assertIsNotNone(documentation)
        assert documentation is not None
        documentation = " ".join(documentation.split())
        for boundary in (
            "module-top-level synchronous",
            "ctx: kat.Context",
            "every remaining parameter exactly one description",
            "Non-boolean parameters without defaults are required",
            "Boolean parameters require a default",
            "optional parameters must default to None",
            "required_tables",
        ):
            with self.subTest(boundary=boundary):
                self.assertIn(boundary, documentation)

    def test_complete_interface_uses_click_converted_defaults(self) -> None:
        self.assertEqual(
            inspect_declared_workflow(analyze),
            {
                "name": "thread-time",
                "title": "Thread time",
                "description": "Inspect thread CPU time.\n\nInternal whitespace remains.",
                "required_tables": ["sched_slice", "thread"],
                "parameters": [
                    {"name": "label", "option": "--label", "type": "string", "required": False, "description": "Label filter", "default": ""},
                    {"name": "count", "option": "--count", "type": "int64", "required": False, "description": "Signed count", "default": "7"},
                    {"name": "ratio", "option": "--ratio", "type": "float64", "required": False, "description": "Finite ratio", "default": 1.0},
                    {"name": "enabled", "option": "--enabled", "negative_option": "--no-enabled", "type": "boolean", "required": False, "description": "Include idle", "default": False},
                    {"name": "window", "option": "--window", "type": "duration", "required": False, "description": "Elapsed window", "default": "5ms"},
                    {"name": "at", "option": "--at", "type": "wall_clock_timestamp", "required": False, "description": "Wall-clock boundary", "default": "2026-07-14T08:30:00Z"},
                    {"name": "mode", "option": "--mode", "type": "string", "required": False, "description": "Aggregation mode", "choices": ["mean", "sum"], "default": "sum"},
                    {"name": "optional_label", "option": "--optional-label", "type": "string", "required": False, "description": "Optional label", "default": None},
                ],
            },
        )

        effective = compile_declared_workflow(analyze).parse_arguments(
            [
                "--count",
                "-9",
                "--ratio",
                "2.5",
                "--enabled",
                "--window",
                "8us",
                "--at",
                "2026-07-14T08:30:00Z",
                "--mode",
                "mean",
            ]
        )
        self.assertEqual(effective["count"], -9)
        self.assertEqual(effective["ratio"], 2.5)
        self.assertIs(effective["enabled"], True)
        self.assertEqual(effective["window"], kat.Duration("8us"))
        self.assertEqual(effective["at"], kat.WallClockTimestamp("2026-07-14T08:30:00Z"))
        self.assertEqual(effective["mode"], "mean")
        self.assertIsNone(effective["optional_label"])
        self.assertEqual(inspect_declared_workflow(unresolved_return)["parameters"], [])
        with self.assertRaises(ValueError):
            compile_declared_workflow(analyze).parse_arguments(
                ["--at", "2026-07-14T08:30:00-00:00"]
            )

        required = compile_declared_workflow(required_string)
        self.assertEqual(
            required.interface["parameters"],
            [
                {
                    "name": "query",
                    "option": "--query",
                    "type": "string",
                    "required": True,
                    "description": "Query text",
                }
            ],
        )
        with self.assertRaises(ValueError):
            required.parse_arguments([])

    def test_temporal_constructors_are_strict_immutable_values(self) -> None:
        self.assertEqual(str(kat.Duration("0.125ms")), "0.125ms")
        self.assertEqual(str(kat.WallClockTimestamp("2026-07-14T16:30:00.120000000+08:00")), "2026-07-14T08:30:00.12Z")
        for invalid in ["-1ms", "1MS", "1.0000000001s", "1", " 1ms"]:
            with self.subTest(invalid=invalid), self.assertRaises((TypeError, ValueError)):
                kat.Duration(invalid)
        with self.assertRaises(TypeError):
            kat.Duration(1)  # type: ignore[arg-type]
        for invalid in [
            "2026-07-14T08:30:00",
            "2026-07-14T08:30:00-00:00",
            "0001-01-01T00:00:00+23:59",
            "9999-12-31T23:59:59-23:59",
        ]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                kat.WallClockTimestamp(invalid)

    def test_runtime_equivalent_optional_annotations_share_the_contract(self) -> None:
        expected = [
            {
                "name": "value",
                "option": "--value",
                "type": "string",
                "required": False,
                "description": "Value",
                "default": None,
            }
        ]
        for function in [legacy_optional, legacy_union, nested_forward_reference]:
            with self.subTest(function=function.__name__):
                self.assertEqual(inspect_declared_workflow(function)["parameters"], expected)

    def test_invalid_workflow_shapes_fail_during_inspection(self) -> None:
        for function in [
            lambda_workflow,
            asynchronous,
            missing_parameter_description,
            required_bool,
            invalid_bool_default,
            overflowing_int_default,
            none_without_optional,
            unsupported_any,
            unsupported_annotated,
            overflowing_wall_clock,
            unknown_wall_clock_offset,
        ]:
            with self.subTest(function=function.__name__), self.assertRaises(ValueError):
                inspect_declared_workflow(function)

        with self.assertRaises(ValueError):
            kat.workflow(name="invalid-table", title="Invalid table", required_tables=["CON"])
        with self.assertRaises(ValueError):
            kat.workflow(
                name="empty-description",
                title="Empty description",
                required_tables=[],
                parameters={"value": "  "},
            )

    def test_decorator_copies_mutable_authoring_containers(self) -> None:
        interface = inspect_declared_workflow(copied_declaration)
        self.assertEqual(interface["required_tables"], ["thread"])
        self.assertEqual(interface["parameters"][0]["description"], "Original description")


if __name__ == "__main__":
    unittest.main()
