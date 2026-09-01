from pathlib import Path

import kat
from kat import dataprovider as dp


@kat.workflow(
    name="fuse-local-parquet",
    description="显式查询两组 Parquet，再融合 eager Table 与磁盘 Catalog。",
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
    """显式查询两组 Parquet，再融合 eager Table 与磁盘 Catalog。"""
    source = dp.open(
        tables={
            "events": Path(events_path),
            "labels": Path(labels_path),
        }
    )
    qualified_events = dp.DataFusionProvider(catalog=source).query(
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

    owners = dp.open(tables={"owners": Path(owners_path)})

    return dp.DataFusionProvider(
        tables={"qualified_events": qualified_events},
        catalog=owners,
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
