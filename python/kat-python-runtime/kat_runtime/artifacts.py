from __future__ import annotations

import os
import re
from dataclasses import dataclass
from pathlib import Path

from datafusion import DataFrame
from datafusion.dataframe import DataFrameWriteOptions


ARTIFACT_NAME = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_-]*")


@dataclass(frozen=True)
class ArtifactPlan:
    name: str
    dataframe: DataFrame
    path: Path
    relative_path: str


def validate_artifacts(result: object, run_dir: Path) -> list[ArtifactPlan]:
    if not isinstance(result, dict):
        raise TypeError("workflow must return dict[str, DataFrame]")

    plans: list[ArtifactPlan] = []
    for name, dataframe in result.items():
        if not isinstance(name, str) or ARTIFACT_NAME.fullmatch(name) is None:
            raise ValueError(f"invalid artifact name: {name!r}")
        if not isinstance(dataframe, DataFrame):
            raise TypeError(f"artifact {name} is not a DataFusion DataFrame")
        relative_path = f"artifacts/{name}.parquet"
        plans.append(
            ArtifactPlan(
                name=name,
                dataframe=dataframe,
                path=run_dir / relative_path,
                relative_path=relative_path,
            )
        )
    return plans


def materialize_artifacts(plans: list[ArtifactPlan]) -> list[dict[str, str]]:
    for plan in plans:
        if os.path.lexists(plan.path):
            raise FileExistsError(f"artifact target already exists: {plan.path}")

    artifacts: list[dict[str, str]] = []
    write_options = DataFrameWriteOptions(single_file_output=True)
    for plan in plans:
        plan.path.parent.mkdir(parents=True, exist_ok=True)
        plan.dataframe.write_parquet(plan.path, write_options=write_options)
        artifacts.append({"name": plan.name, "path": plan.relative_path})
    return artifacts
