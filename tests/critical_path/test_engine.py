from __future__ import annotations

import sys
import unittest
from pathlib import Path

PACK_ROOT = Path(__file__).resolve().parents[2] / "packs" / "critical-path"
sys.path.insert(0, str(PACK_ROOT))

from lib.engine import analyze_critical_path
from lib.model import StateSegment, ThreadRef, TraceFacts, WakeupEdge


class CriticalPathEngineTests(unittest.TestCase):
    def test_running_root_classifies_as_self_execution(self) -> None:
        facts = TraceFacts(
            threads={1: ThreadRef(itid=1, tid=101, name="UI")},
            states=[StateSegment(id=1, itid=1, ts=0, dur=10_000_000, state="Running")],
        )

        result = analyze_critical_path(facts, root_itid=1, start_ts=0, end_ts=10_000_000)

        self.assertEqual([node.classification for node in result.nodes], ["self_execution"])
        self.assertEqual(result.edges, [])
        self.assertEqual(result.uncertainties, [])

    def test_sleeping_root_recurses_into_waker_thread(self) -> None:
        facts = TraceFacts(
            threads={
                1: ThreadRef(itid=1, tid=101, name="UI"),
                2: ThreadRef(itid=2, tid=202, name="worker"),
            },
            states=[
                StateSegment(id=1, itid=1, ts=0, dur=10_000_000, state="Sleeping"),
                StateSegment(id=2, itid=2, ts=2_000_000, dur=7_000_000, state="Running"),
            ],
            wakeups=[WakeupEdge(id=7, ts=9_000_000, target_itid=1, waker_itid=2)],
        )

        result = analyze_critical_path(facts, root_itid=1, start_ts=0, end_ts=10_000_000)

        self.assertEqual(
            [(node.depth, node.itid, node.classification) for node in result.nodes],
            [(0, 1, "waiting_for_wakeup"), (1, 2, "self_execution")],
        )
        self.assertEqual(len(result.edges), 1)
        self.assertEqual(result.edges[0].relation, "upper_lower")
        self.assertEqual(result.edges[0].from_itid, 1)
        self.assertEqual(result.edges[0].to_itid, 2)
        self.assertEqual(result.edges[0].to_node_id, 2)

    def test_runnable_without_waker_records_uncertainty(self) -> None:
        facts = TraceFacts(
            threads={1: ThreadRef(itid=1, tid=101, name="UI")},
            states=[StateSegment(id=1, itid=1, ts=0, dur=5_000_000, state="Runnable")],
        )

        result = analyze_critical_path(facts, root_itid=1, start_ts=0, end_ts=5_000_000)

        self.assertEqual(result.nodes[0].classification, "scheduler_wait")
        self.assertEqual([item.code for item in result.uncertainties], ["missing_waker"])

    def test_udk_irq_waker_stops_recursion_and_marks_io_block(self) -> None:
        facts = TraceFacts(
            threads={
                1: ThreadRef(itid=1, tid=101, name="UI"),
                9: ThreadRef(itid=9, tid=909, name="udk-irq/1"),
            },
            states=[StateSegment(id=1, itid=1, ts=0, dur=10_000_000, state="Sleeping")],
            wakeups=[WakeupEdge(id=9, ts=8_000_000, target_itid=1, waker_itid=9)],
        )

        result = analyze_critical_path(facts, root_itid=1, start_ts=0, end_ts=10_000_000)

        self.assertEqual(len(result.nodes), 1)
        self.assertEqual(result.edges[0].classification, "io_block")
        self.assertEqual([item.code for item in result.uncertainties], ["irq_cutoff"])

    def test_repeated_dependency_edge_stops_cycle(self) -> None:
        facts = TraceFacts(
            threads={
                1: ThreadRef(itid=1, tid=101, name="A"),
                2: ThreadRef(itid=2, tid=202, name="B"),
            },
            states=[
                StateSegment(id=1, itid=1, ts=0, dur=10_000_000, state="Sleeping"),
                StateSegment(id=2, itid=2, ts=0, dur=8_000_000, state="Sleeping"),
            ],
            wakeups=[
                WakeupEdge(id=1, ts=8_000_000, target_itid=1, waker_itid=2),
                WakeupEdge(id=2, ts=6_000_000, target_itid=2, waker_itid=1),
                WakeupEdge(id=3, ts=4_000_000, target_itid=1, waker_itid=2),
            ],
        )

        result = analyze_critical_path(facts, root_itid=1, start_ts=0, end_ts=10_000_000)

        self.assertIn("cycle_detected", [item.code for item in result.uncertainties])


if __name__ == "__main__":
    unittest.main()
