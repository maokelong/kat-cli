from __future__ import annotations

import inspect
import unittest
from pathlib import Path
from typing import Annotated, Any, Literal, Optional, Union

import kat
from _kat_runtime.inspection import compile_declared_workflow, inspect_declared_workflow


@kat.workflow(
    name="thread-time",
    description=" Inspect thread CPU time.\n\nInternal whitespace remains. ",
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


@kat.workflow(
    name="return-is-not-input",
    description="The Input Compiler must not evaluate the return annotation.",
)
def unresolved_return(ctx: kat.Context) -> UndefinedReturn:
    """The Input Compiler must not evaluate the return annotation."""


mutable_descriptions = {"value": " Original description "}


@kat.workflow(
    name="copied-declaration",
    description="The decorator owns an immutable declaration snapshot.",
    parameters=mutable_descriptions,
)
def copied_declaration(ctx: kat.Context, value: str) -> None:
    """The decorator owns an immutable declaration snapshot."""


mutable_descriptions["value"] = "Mutated"


lambda_workflow = lambda ctx: None
lambda_workflow.__annotations__ = {"ctx": kat.Context}
lambda_workflow.__doc__ = "A lambda is not a declared Workflow function."
lambda_workflow = kat.workflow(
    name="lambda-workflow", description="A lambda is not a declared Workflow function."
)(lambda_workflow)


@kat.workflow(
    name="asynchronous",
    description="Not a synchronous Workflow.",
    parameters={"value": "Value"},
)
async def asynchronous(ctx: kat.Context, value: str) -> None:
    """Not a synchronous Workflow."""


@kat.workflow(
    name="missing-description",
    description="Descriptions must match.",
    parameters={},
)
def missing_parameter_description(ctx: kat.Context, value: str) -> None:
    """Descriptions must match."""


@kat.workflow(
    name="required-bool",
    description="Bool requires a default.",
    parameters={"flag": "Flag"},
)
def required_bool(ctx: kat.Context, flag: bool) -> None:
    """Bool requires a default."""


@kat.workflow(
    name="invalid-bool-default",
    description="Invalid Click defaults remain PACK authoring failures.",
    parameters={"flag": "Flag"},
)
def invalid_bool_default(ctx: kat.Context, flag: bool = 1) -> None:  # type: ignore[assignment]
    """Invalid Click defaults remain PACK authoring failures."""


@kat.workflow(
    name="overflowing-int-default",
    description="Overflowing numeric conversion remains a PACK authoring failure.",
    parameters={"count": "Count"},
)
def overflowing_int_default(ctx: kat.Context, count: int = float("inf")) -> None:  # type: ignore[assignment]
    """Overflowing numeric conversion remains a PACK authoring failure."""


@kat.workflow(
    name="none-without-optional",
    description="None only belongs to the Optional contract.",
    parameters={"value": "Value"},
)
def none_without_optional(
    ctx: kat.Context, value: str = None  # type: ignore[assignment]
) -> None:
    """None only belongs to the Optional contract."""


@kat.workflow(
    name="unsupported-any",
    description="Any is outside the closed type set.",
    parameters={"value": "Value"},
)
def unsupported_any(ctx: kat.Context, value: Any) -> None:
    """Any is outside the closed type set."""


@kat.workflow(
    name="unsupported-annotated",
    description="Typing extras are outside the closed authoring type set.",
    parameters={"value": "Value"},
)
def unsupported_annotated(ctx: kat.Context, value: Annotated[str, "metadata"]) -> None:
    """Typing extras are outside the closed authoring type set."""


@kat.workflow(
    name="overflowing-wall-clock",
    description="The UTC conversion must remain inside the supported datetime range.",
    parameters={"at": "Boundary"},
)
def overflowing_wall_clock(
    ctx: kat.Context,
    at: kat.WallClockTimestamp = "0001-01-01T00:00:00+23:59",
) -> None:
    """The UTC conversion must remain inside the supported datetime range."""


@kat.workflow(
    name="unknown-wall-clock-offset",
    description="A wall-clock default must identify a known UTC offset.",
    parameters={"at": "Boundary"},
)
def unknown_wall_clock_offset(
    ctx: kat.Context,
    at: kat.WallClockTimestamp = "2026-07-14T08:30:00-00:00",
) -> None:
    """A wall-clock default must identify a known UTC offset."""


@kat.workflow(
    name="legacy-optional",
    description="typing.Optional resolves to the supported optional type.",
    parameters={"value": "Value"},
)
def legacy_optional(ctx: kat.Context, value: Optional[str] = None) -> None:
    """typing.Optional resolves to the supported optional type."""


@kat.workflow(
    name="legacy-union",
    description="typing.Union resolves to the supported optional type.",
    parameters={"value": "Value"},
)
def legacy_union(ctx: kat.Context, value: Union[str, None] = None) -> None:
    """typing.Union resolves to the supported optional type."""


@kat.workflow(
    name="nested-forward-reference",
    description="Nested ForwardRefs resolve through the standard typing evaluator.",
    parameters={"value": "Value"},
)
def nested_forward_reference(
    ctx: kat.Context, value: Optional["str"] = None
) -> None:
    """Nested ForwardRefs resolve through the standard typing evaluator."""


@kat.workflow(
    name="required-string",
    description="Required values omit their default.",
    parameters={"query": "Query text"},
)
def required_string(ctx: kat.Context, query: str) -> None:
    """Required values omit their default."""


class AuthoringApiTest(unittest.TestCase):
    def test_provider_is_a_metadata_only_class_decorator(self) -> None:
        class PlainProvider:
            pass

        decorated = kat.provider(
            name="postgresql",
            description=" Query a PostgreSQL service. ",
            guide=" providers/postgresql.md ",
        )(PlainProvider)

        self.assertIs(decorated, PlainProvider)
        self.assertIn("provider", kat.__all__)
        declaration = vars(PlainProvider)["__kat_provider__"]
        self.assertEqual(declaration.name, "postgresql")
        self.assertEqual(declaration.description, "Query a PostgreSQL service.")
        self.assertEqual(declaration.guide, "providers/postgresql.md")
        self.assertEqual(PlainProvider.__bases__, (object,))

        with self.assertRaisesRegex(TypeError, "name"):
            kat.provider(  # type: ignore[arg-type]
                name=1,
                description="Invalid",
                guide="providers/invalid.md",
            )
        with self.assertRaisesRegex(ValueError, "name"):
            kat.provider(
                name="  ",
                description="Invalid",
                guide="providers/invalid.md",
            )
        with self.assertRaisesRegex(TypeError, "description"):
            kat.provider(  # type: ignore[arg-type]
                name="invalid",
                description=1,
                guide="providers/invalid.md",
            )
        with self.assertRaisesRegex(TypeError, "guide"):
            kat.provider(name="invalid", description="Invalid", guide=1)  # type: ignore[arg-type]
        with self.assertRaises(TypeError):
            kat.provider(  # type: ignore[call-arg]
                name="invalid",
                description="Invalid",
                guide="providers/invalid.md",
                module="override",
            )
        with self.assertRaisesRegex(TypeError, "Provider must be a class"):
            kat.provider(name="function", description="Function", guide="function.md")(
                lambda: None
            )
        with self.assertRaisesRegex(ValueError, "description"):
            kat.provider(name="empty", description="  ", guide="empty.md")
        with self.assertRaisesRegex(ValueError, "guide"):
            kat.provider(name="empty", description="Empty", guide="  ")
        with self.assertRaisesRegex(ValueError, "only one Provider"):
            kat.provider(name="again", description="Again", guide="again.md")(
                PlainProvider
            )

    def test_dataprovider_toolkit_is_the_only_top_level_data_export(self) -> None:
        self.assertIn("dataprovider", kat.__all__)
        self.assertIsNotNone(kat.dataprovider)
        self.assertEqual(
            set(kat.dataprovider.__all__),
            {
                "Schema",
                "Table",
                "Catalog",
                "DataFusionProvider",
                "write",
                "open",
            },
        )
        self.assertFalse(hasattr(kat.dataprovider, "materialize"))

        write_signature = inspect.signature(kat.dataprovider.write)
        self.assertEqual(tuple(write_signature.parameters), ("schema", "destination"))
        self.assertEqual(
            write_signature.parameters["destination"].kind,
            inspect.Parameter.KEYWORD_ONLY,
        )
        from_rows_signature = inspect.signature(kat.dataprovider.Table.from_rows)
        self.assertEqual(tuple(from_rows_signature.parameters), ("rows", "schema"))
        self.assertEqual(
            from_rows_signature.parameters["schema"].kind,
            inspect.Parameter.KEYWORD_ONLY,
        )
        self.assertNotIn("datasource", kat.__all__)
        self.assertFalse(hasattr(kat, "datasource"))

        for name in (
            "SourceExecutor",
            "ParquetSource",
            "Provider",
            "Schema",
            "Table",
            "Catalog",
            "DataFusionProvider",
            "table",
            "from_arrow",
            "from_rows",
            "to_arrow",
            "materialize",
            "write",
            "open",
        ):
            with self.subTest(name=name):
                self.assertNotIn(name, kat.__all__)
                self.assertFalse(hasattr(kat, name))

        for name in ("table", "from_arrow", "from_rows", "to_arrow"):
            with self.subTest(dataprovider_name=name):
                self.assertFalse(hasattr(kat.dataprovider, name))

    def test_context_documents_and_types_the_execution_roots(self) -> None:
        self.assertFalse(hasattr(kat.Context, "provider"))
        self.assertFalse(hasattr(kat.Context, "session_id"))
        self.assertFalse(hasattr(kat.Context, "session_root"))
        for name in ("datasource_root", "scratch_root"):
            with self.subTest(root=name):
                root_property = getattr(kat.Context, name)
                self.assertIsInstance(root_property, property)
                self.assertIsNone(root_property.fset)
                root_getter = root_property.fget
                self.assertIsNotNone(root_getter)
                assert root_getter is not None
                signature = inspect.signature(root_getter)
                self.assertEqual(tuple(signature.parameters), ("self",))
                self.assertEqual(signature.return_annotation, "Path")

        datasource_documentation = " ".join(
            (inspect.getdoc(kat.Context.datasource_root) or "").split()
        )
        for boundary in (
            "current Analysis Session",
            "shared Datasource materialization",
            "across PACKs",
            "valid only for this Workflow execution",
            "ordinary Path",
            "not a filesystem sandbox",
        ):
            with self.subTest(datasource_boundary=boundary):
                self.assertIn(boundary, datasource_documentation)

        scratch_documentation = " ".join(
            (inspect.getdoc(kat.Context.scratch_root) or "").split()
        )
        for boundary in (
            "current candidate execution",
            "temporary",
            "cleaned when execution ends",
            "must not be reused by later Workflows",
            "valid only for this Workflow execution",
            "ordinary Path",
            "not a filesystem sandbox",
        ):
            with self.subTest(scratch_boundary=boundary):
                self.assertIn(boundary, scratch_documentation)

    def test_context_documents_and_types_the_authoring_contract(self) -> None:
        self.assertFalse(hasattr(kat.Context, "sql"))
        self.assertFalse(hasattr(kat.Context, "from_arrow"))
        self.assertFalse(hasattr(kat.Context, "convert_clock"))
        self.assertEqual(
            tuple(inspect.signature(kat.workflow).parameters),
            ("name", "description", "parameters", "guide"),
        )

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
            "description",
            "guide",
            "[a-z0-9]+(?:-[a-z0-9]+)*",
            "known explicit UTC offset",
            "absolute UTC instant, not a local civil-time value",
            "Successful decoration alone does not mean the production input Interface is valid",
            "exact, non-empty ``dict``",
            "dataprovider.Table",
            "single value",
            "``main`` Output",
            "exact Tables",
            "all-or-fail Run publication",
        ):
            with self.subTest(boundary=boundary):
                self.assertIn(boundary, documentation)
        self.assertNotIn("Table.name", documentation)

    def test_complete_interface_uses_click_converted_defaults(self) -> None:
        self.assertEqual(
            inspect_declared_workflow(analyze),
            {
                "name": "thread-time",
                "description": "Inspect thread CPU time.\n\nInternal whitespace remains.",
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
        self.assertEqual(
            str(kat.WallClockTimestamp("1677-09-21T00:12:43.145224192Z")),
            "1677-09-21T00:12:43.145224192Z",
        )
        self.assertEqual(
            str(kat.WallClockTimestamp("2262-04-11T23:47:16.854775807Z")),
            "2262-04-11T23:47:16.854775807Z",
        )
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
            "1677-09-21T00:12:43.145224191Z",
            "2262-04-11T23:47:16.854775808Z",
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
            kat.workflow(
                name="empty-description",
                description="Description",
                parameters={"value": "  "},
            )

    def test_decorator_copies_mutable_authoring_containers(self) -> None:
        interface = inspect_declared_workflow(copied_declaration)
        self.assertEqual(interface["parameters"][0]["description"], "Original description")


if __name__ == "__main__":
    unittest.main()
