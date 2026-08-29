from pathlib import Path

import kat
from kat import datasource as ds

from kat.pack.datasources.parquet import LocalParquetProvider


@kat.workflow(
    name="fuse-local-parquet",
    title="Fuse explicitly mapped local Parquet tables",
    required_tables=[],
    parameters={
        "events_path": "Parquet file containing events.",
        "labels_path": "Parquet file or sharded directory containing event labels.",
        "owners_path": "Parquet file containing owner names.",
        "minimum_score": "Inclusive minimum event score.",
    },
)
def fuse_local_parquet(
    ctx: kat.Context,
    events_path: str,
    labels_path: str,
    owners_path: str,
    minimum_score: int = 0,
):
    """显式查询本地 Provider，再融合它们返回的 eager Table。"""
    qualified_events = LocalParquetProvider(
        tables={
            "events": Path(events_path),
            "labels": Path(labels_path),
        },
    ).query(
        """
        SELECT
            event.event_id,
            event.owner_id,
            label.label,
            event.score
        FROM events AS event
        JOIN labels AS label USING (event_id)
        WHERE event.score >= $minimum_score
        """,
        params={"minimum_score": minimum_score},
    )

    owners = LocalParquetProvider(
        tables={"owners": Path(owners_path)},
    ).query(
        "SELECT owner_id, owner_name FROM owners",
    )

    return ds.DataFusionProvider(
        tables={
            "qualified_events": qualified_events,
            "owners": owners,
        }
    ).query(
        """
        SELECT
            event.event_id,
            event.label,
            owner.owner_name,
            event.score
        FROM qualified_events AS event
        JOIN owners AS owner USING (owner_id)
        ORDER BY event.event_id
        """
    )
