#!/usr/bin/env python3
"""从锁定的独立 SDK 提交构建纯 Python wheel，供两个 Payload 共用。"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

import payload_builder


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--source", type=Path, help="已检出的锁定 SDK 提交；省略时从远端获取")
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    lock = json.loads((options.repository / "build/sdk-source.json").read_text("utf-8"))
    if not re.fullmatch(r"[0-9a-f]{40}", lock["revision"]):
        raise ValueError("SDK source must pin a full Git commit")
    output = options.output.resolve()
    if output.exists():
        raise ValueError(f"SDK output already exists: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="sdk-build-") as temporary:
        source = options.source
        if source is None:
            source = Path(temporary) / "source"
            subprocess.run(["git", "clone", "--no-checkout", lock["repository"], str(source)], check=True)
            subprocess.run(["git", "-C", str(source), "checkout", "--detach", lock["revision"]], check=True)
        source = source.resolve(strict=True)
        revision = subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
        ).strip()
        if revision != lock["revision"]:
            raise ValueError("SDK checkout does not match the locked commit")
        if subprocess.check_output(
            ["git", "-C", str(source), "status", "--porcelain"], text=True
        ).strip():
            raise ValueError("SDK checkout must be clean")
        built = Path(temporary) / "wheels"
        subprocess.run(
            [sys.executable, "-m", "pip", "wheel", "--no-deps", "--wheel-dir", str(built), str(source)],
            check=True,
        )
        wheels = list(built.glob("*.whl"))
        if len(wheels) != 1:
            raise ValueError("SDK build must produce exactly one wheel")
        wheel = wheels[0]
        payload_builder.validate_sdk_wheel_archive(wheel, expected_version=lock["version"])
        output.mkdir()
        destination = output / wheel.name
        destination.write_bytes(wheel.read_bytes())
        (output / f"{wheel.name}.sha256").write_text(
            f"{payload_builder.file_sha256(destination)}  {wheel.name}\n", encoding="ascii"
        )
        print(destination)


if __name__ == "__main__":
    main()
