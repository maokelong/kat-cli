from __future__ import annotations

import inspect
import unittest

import kat
from _kat_runtime.inspection import compile_declared_workflow


@kat.workflow(
    name="inspect-events",
    description=" Inspect event rows. ",
    guide=" workflows/inspect-events.md ",
)
def inspect_events(ctx: kat.Context) -> None:
    """This docstring is not public Workflow knowledge."""
    pass


@kat.workflow(name="no-guide", description="Description")
def no_guide(ctx: kat.Context) -> None:
    pass


class WorkflowDeclarationContractTest(unittest.TestCase):
    def test_public_signature_contains_only_the_knowledge_contract(self) -> None:
        parameters = inspect.signature(kat.workflow).parameters

        self.assertEqual(
            list(parameters),
            ["name", "description", "parameters", "guide"],
        )
        self.assertTrue(
            all(parameter.kind is inspect.Parameter.KEYWORD_ONLY for parameter in parameters.values())
        )
        self.assertIs(parameters["parameters"].default, None)
        self.assertIs(parameters["guide"].default, None)

    def test_explicit_description_and_guide_are_not_derived_from_docstring(self) -> None:
        compiled = compile_declared_workflow(inspect_events)

        self.assertEqual(
            compiled.interface,
            {
                "name": "inspect-events",
                "description": "Inspect event rows.",
                "parameters": [],
            },
        )
        self.assertEqual(compiled.guide_ref, "workflows/inspect-events.md")

    def test_description_is_required_and_guide_is_optional(self) -> None:
        with self.assertRaisesRegex(ValueError, "name"):
            kat.workflow(name="  ", description="Description")
        with self.assertRaisesRegex(TypeError, "description"):
            kat.workflow(name="wrong-description", description=1)  # type: ignore[arg-type]
        with self.assertRaisesRegex(ValueError, "description"):
            kat.workflow(name="empty", description="  ")
        with self.assertRaisesRegex(ValueError, "guide"):
            kat.workflow(name="empty-guide", description="Description", guide="  ")
        with self.assertRaises(TypeError):
            kat.workflow(  # type: ignore[call-arg]
                name="legacy",
                description="Description",
                title="Legacy",
            )

        self.assertIsNone(compile_declared_workflow(no_guide).guide_ref)


if __name__ == "__main__":
    unittest.main()
