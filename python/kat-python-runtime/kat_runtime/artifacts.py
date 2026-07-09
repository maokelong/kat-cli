from __future__ import annotations

import re
from pathlib import Path
from typing import Any


ARTIFACT_NAME = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_-]*$")


def materialize_artifacts(result: dict[str, Any], run_dir: Path) -> list[dict[str, Any]]:
    if not isinstance(result, dict):
        raise TypeError("workflow must return dict[str, DataFrame]")

    artifacts_dir = run_dir / "artifacts"
    artifacts_dir.mkdir(parents=True, exist_ok=True)
    artifacts: list[dict[str, Any]] = []

    for name, dataframe in result.items():
        if not isinstance(name, str) or not ARTIFACT_NAME.match(name):
            raise ValueError(f"invalid artifact name: {name!r}")
        if not hasattr(dataframe, "write_parquet"):
            raise TypeError(f"artifact {name} is not a DataFusion DataFrame")
        path = artifacts_dir / f"{name}.parquet"
        dataframe.write_parquet(str(path))
        artifacts.append(
            {
                "name": name,
                "path": f"artifacts/{name}.parquet",
            }
        )

    return artifacts
