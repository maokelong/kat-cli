from __future__ import annotations

from pathlib import Path
import tempfile
import unittest

import pyarrow as pa

from _clock_dataset import write_clock_dataset
from _kat_runtime.clock import ClockResolver
from _kat_runtime.request import ResolvedDatasetRef


class ClockResolverTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def dataset(
        self,
        *,
        definitions: list[tuple[str, str, int]] | None = None,
        snapshots: list[tuple[int, str, int]] | None = None,
    ) -> ResolvedDatasetRef:
        return ResolvedDatasetRef(
            path=self.root,
            tables=write_clock_dataset(
                self.root,
                definitions=definitions,
                snapshots=snapshots,
            ),
        )

    def convert(
        self,
        dataset: ResolvedDatasetRef,
        domains: list[str | None],
        values: list[int | None],
        target: str,
    ) -> pa.Array:
        return ClockResolver(dataset).convert_batch(
            pa.array(domains, type=pa.string()),
            pa.array(values, type=pa.uint64()),
            pa.array([target] * len(domains), type=pa.string()),
        )

    def test_cross_domain_and_all_null_rows_preserve_existing_semantics(self) -> None:
        dataset = self.dataset(
            definitions=[
                ("monotonic", "monotonic", 1_000_000_000),
                ("realtime", "realtime", 1_000_000_000),
            ],
            snapshots=[
                (0, "monotonic", 100),
                (0, "realtime", 1_000),
            ],
        )

        result = self.convert(
            dataset,
            ["monotonic", None],
            [105, None],
            "realtime",
        )

        self.assertEqual(result.to_pylist(), [1005, None])

    def test_same_domain_does_not_require_snapshot(self) -> None:
        dataset = self.dataset(
            definitions=[("monotonic", "monotonic", 1_000_000_000)]
        )

        result = self.convert(dataset, ["monotonic"], [105], "monotonic")

        self.assertEqual(result.to_pylist(), [105])

    def test_half_null_clock_fails(self) -> None:
        dataset = self.dataset(
            definitions=[("monotonic", "monotonic", 1_000_000_000)]
        )

        with self.assertRaisesRegex(ValueError, "must be null together"):
            self.convert(dataset, [None], [105], "monotonic")

    def test_missing_definition_and_snapshot_fail_at_their_owned_boundaries(self) -> None:
        with self.assertRaisesRegex(ValueError, "clock_domain evidence"):
            self.convert(self.dataset(), ["monotonic"], [105], "monotonic")

        definitions_only = self.dataset(
            definitions=[
                ("monotonic", "monotonic", 1_000_000_000),
                ("realtime", "realtime", 1_000_000_000),
            ]
        )
        with self.assertRaisesRegex(ValueError, "baseline is incomplete"):
            self.convert(definitions_only, ["monotonic"], [105], "realtime")

    def test_invalid_frequency_fails_the_complete_conversion(self) -> None:
        dataset = self.dataset(
            definitions=[
                ("monotonic", "monotonic", 1_000_000_000),
                ("realtime", "realtime", 1),
            ]
        )

        with self.assertRaisesRegex(ValueError, "definitions are invalid"):
            self.convert(dataset, ["monotonic"], [105], "monotonic")

    def test_checked_translation_rejects_unsigned_overflow(self) -> None:
        maximum = 2**64 - 1
        dataset = self.dataset(
            definitions=[
                ("monotonic", "monotonic", 1_000_000_000),
                ("realtime", "realtime", 1_000_000_000),
            ],
            snapshots=[
                (0, "monotonic", 0),
                (0, "realtime", maximum),
            ],
        )

        with self.assertRaises(pa.ArrowInvalid):
            self.convert(dataset, ["monotonic"], [1], "realtime")


if __name__ == "__main__":
    unittest.main()
