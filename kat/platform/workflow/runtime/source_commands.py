from __future__ import annotations

from dataclasses import dataclass

from datafusion.dataframe import DataFrameWriteOptions

from .pack import ProductionPack, SOURCE_OPERATION_PROFILE
from .request import BindSourceRequest, MaterializeSourceRequest
from .sources import SourceArgumentOverride, open_source_operation


@dataclass(frozen=True)
class BindSourceRuntimeResult:
    pass


@dataclass(frozen=True)
class MaterializeSourceRuntimeResult:
    tables: list[str]


def bind_source(request: BindSourceRequest) -> BindSourceRuntimeResult:
    pack = ProductionPack.open(
        request.pack_name,
        request.pack_path,
        profile=SOURCE_OPERATION_PROFILE,
    )
    source = pack.load_source(request.source_name)
    source.parse_arguments(
        request.arguments,
        argument_base=request.argument_base,
    )
    return BindSourceRuntimeResult()


def materialize_source(
    request: MaterializeSourceRequest,
) -> MaterializeSourceRuntimeResult:
    pack = ProductionPack.open(
        request.pack_name,
        request.pack_path,
        profile=SOURCE_OPERATION_PROFILE,
    )
    override = SourceArgumentOverride.create(
        request.arguments,
        argument_base=request.argument_base,
    )
    with open_source_operation(
        current_pack=pack,
        dataset=None,
        overrides={request.source_name: override},
    ) as operation:
        schema = operation.schema(pack.name, request.source_name)
        available = tuple(schema.table_names)  # type: ignore[attr-defined]
        selected = request.tables or available
        if not selected:
            raise ValueError(
                f"Source {pack.name}.{request.source_name} provides no tables to materialize"
            )
        unknown = sorted(set(selected) - set(available))
        if unknown:
            names = ", ".join(unknown)
            choices = ", ".join(available) or "none"
            raise ValueError(
                f"Source {pack.name}.{request.source_name} does not provide tables "
                f"{names}; available: {choices}"
            )
        for table in selected:
            frame = operation.session.sql(
                "SELECT * FROM "
                f"{_quoted(pack.name)}.{_quoted(request.source_name)}.{_quoted(table)}"
            )
            path = request.export_path / f"{table}.parquet"
            frame.write_parquet(
                path,
                write_options=DataFrameWriteOptions(single_file_output=True),
            )
    return MaterializeSourceRuntimeResult(tables=list(selected))


def _quoted(identifier: str) -> str:
    return '"' + identifier.replace('"', '""') + '"'
