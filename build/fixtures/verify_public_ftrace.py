"""Verify public Ftrace knowledge and PACK consumption from an installed Payload."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


def main() -> None:
    cli = Path(sys.argv[1]).resolve(strict=True)
    repository = Path(__file__).resolve().parents[2]
    pack = repository / "examples/packs/mem-pack"

    def invoke(*arguments: str, success: bool = True) -> dict:
        completed = subprocess.run(
            [str(cli), *arguments],
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        response = json.loads(completed.stdout)
        assert (completed.returncode == 0) == success, (response, completed.stderr)
        assert response["status"] == ("success" if success else "failure"), response
        return response

    listing = invoke("inspect", "provider")["result"]["providers"]
    assert [item["name"] for item in listing] == [
        "ftrace-text",
        "trace-streamer-sqlite",
    ], listing
    detail = invoke("inspect", "provider", "--provider", "ftrace-text")["result"][
        "provider"
    ]
    assert detail["module"] == "kat.dataprovider.ftrace", detail
    assert detail["qualname"] == "FtraceProvider", detail
    assert "text_ftrace_event_sched_switch" in detail["guide"], detail
    assert "clock_domain" in detail["guide"], detail
    selected = ("inspect", "provider", "--pack", "mem-pack", "--pack-dir", str(pack))
    assert invoke(*selected)["result"]["providers"] == []
    invoke(*selected, "--provider", "ftrace-text", success=False)
    tested = invoke("test", "--pack-dir", str(pack))
    assert tested["result"]["summary"].get("passed", 0) >= 2, tested
    subprocess.run(
        [
            sys.executable,
            "-I",
            "-B",
            "-m",
            "pytest",
            "-q",
            "-p",
            "no:cacheprovider",
            str(repository / "kat/platform/workflow/tests/ftrace"),
        ],
        check=True,
    )
    print(
        "Public Ftrace guide, PACK migration and Provider behavior verified from installed Payload"
    )


if __name__ == "__main__":
    main()
