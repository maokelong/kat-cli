"""KAT's standard eager table and local Parquet datasource toolkit."""

from ._parquet import Catalog, open, write
from ._schema import Schema
from ._table import Table, from_arrow, table, to_arrow

__all__ = [
    "Catalog",
    "Schema",
    "Table",
    "from_arrow",
    "open",
    "table",
    "to_arrow",
    "write",
]
