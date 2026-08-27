from pathlib import Path

import kat

from kat.pack.helpers.datasources import parquet


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
    """Query local catalogs independently, then fuse their localized results."""
    qualified_events = parquet.provider(
        ctx,
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
        name="qualified_events",
    )

    parquet.provider(
        ctx,
        tables={"owners": Path(owners_path)},
    ).query(
        "SELECT owner_id, owner_name FROM owners",
        name="owners",
    )

    return ctx.sql(
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
