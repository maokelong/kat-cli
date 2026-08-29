from __future__ import annotations

from collections.abc import Mapping
from types import MappingProxyType

from datafusion import SessionContext
import pyarrow as pa
import pyarrow.dataset as pads

from ._parquet import Catalog
from ._sql import execute_sql, prepare_query, require_sql_name
from ._table import Table


class DataFusionProvider:
    """Reusable local SQL over explicitly bound eager and Parquet relations."""

    __slots__ = ("__tables", "__catalog")

    def __init__(
        self,
        *,
        tables: Mapping[str, Table] | None = None,
        catalog: Catalog | None = None,
    ) -> None:
        if tables is None:
            memory: dict[str, Table] = {}
        else:
            if not isinstance(tables, Mapping):
                raise TypeError("tables must be a Mapping of relation names to ds.Table values")
            memory = dict(tables.items())
            for relation_name, table in memory.items():
                require_sql_name(relation_name, "Fusion relation")
                if not isinstance(table, Table):
                    raise TypeError(
                        f"Fusion relation {relation_name!r} must be a ds.Table"
                    )

        if catalog is not None and not isinstance(catalog, Catalog):
            raise TypeError("catalog must be a ds.Catalog or None")
        if not memory and catalog is None:
            raise ValueError("DataFusionProvider requires tables or catalog")

        catalog_names = set(catalog.tables) if catalog is not None else set()
        overlap = catalog_names.intersection(memory)
        if overlap:
            raise ValueError(
                "memory and Catalog relation names must not overlap: "
                f"{tuple(sorted(overlap))!r}"
            )

        object.__setattr__(
            self,
            "_DataFusionProvider__tables",
            MappingProxyType(memory),
        )
        object.__setattr__(self, "_DataFusionProvider__catalog", catalog)

    def query(
        self,
        sql: str,
        *,
        params: Mapping[str, object] | None = None,
    ) -> Table:
        prepared_sql, values = prepare_query(sql, params)
        session = SessionContext()

        # Keep each exported Arrow snapshot and Dataset alive until collect()
        # completes. A later append creates another Table snapshot and is visible
        # only to the next query call.
        arrow_snapshots: list[pa.Table] = []
        datasets: list[pads.Dataset] = []
        for relation_name, table in self.__tables.items():
            snapshot = table.to_arrow()
            arrow_snapshots.append(snapshot)
            session.from_arrow(snapshot, name=relation_name)

        if self.__catalog is not None:
            for relation_name, relation in self.__catalog._relation_items():
                dataset = relation.dataset()
                datasets.append(dataset)
                session.register_table(relation_name, dataset)

        return execute_sql(session, prepared_sql, values=values)
