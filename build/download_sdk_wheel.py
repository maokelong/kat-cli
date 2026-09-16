#!/usr/bin/env python3
"""下载并校验锁定的 SDK wheel，供两个 Payload 共用。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import payload_builder


def download_sdk_wheel(repository: Path, output: Path, *, offline: bool = False) -> Path:
    lock = json.loads((repository / "build/sdk-wheel.json").read_text("utf-8"))
    asset = payload_builder.LockedAsset.from_json(lock["artifact"], description="SDK wheel")
    wheel = payload_builder.download_locked_asset(asset, output.resolve(), offline=offline)
    payload_builder.validate_sdk_wheel_archive(wheel, expected_version=lock["version"])
    return wheel


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--offline", action="store_true", help="只使用已下载且校验通过的 wheel")
    options = parser.parse_args()
    print(download_sdk_wheel(options.repository, options.output, offline=options.offline))


if __name__ == "__main__":
    main()
