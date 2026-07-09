from __future__ import annotations

import argparse
import json
from pathlib import Path

from kat_runtime.pack_loader import discover_pack


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pack-root", required=True)
    args = parser.parse_args()

    manifest = discover_pack(Path(args.pack_root))
    print(json.dumps(manifest, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
