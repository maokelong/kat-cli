#!/usr/bin/env python3
"""用已安装 Host 验证所有 Bundled PACK 和无依赖组合示例。"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli", type=Path)
    parser.add_argument("bundled_packs", type=Path)
    parser.add_argument("composition_pack", type=Path)
    parser.add_argument("work", type=Path)
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=False)
    data_home = args.work.resolve() / "data-home"
    data_home.mkdir()
    environment = {**os.environ, "KAT_DATA_HOME": str(data_home)}
    packs = sorted(path.parent for path in args.bundled_packs.glob("*/pack.toml"))
    if not packs:
        raise RuntimeError("No Bundled PACKs found")
    for pack in [*packs, args.composition_pack]:
        completed = subprocess.run(
            [str(args.cli.resolve()), "test", "--pack-dir", str(pack.resolve())],
            env=environment, capture_output=True, timeout=180,
        )
        response = json.loads(completed.stdout)
        (args.work / f"{pack.name}.json").write_text(
            json.dumps(response, ensure_ascii=False, indent=2), encoding="utf-8",
        )
        if completed.returncode or response["status"] != "success":
            raise RuntimeError(f"{pack.name}: {response}\n{completed.stderr.decode(errors='replace')}")
        if response["result"]["summary"].get("passed", 0) < 1:
            raise RuntimeError(f"{pack.name} ran no successful tests")
        print(f"{pack.name}: {response['result']['summary']}")
    if (data_home / "sessions").exists():
        raise RuntimeError("kat test leaked test Sessions into the configured Data Home")


if __name__ == "__main__":
    main()
