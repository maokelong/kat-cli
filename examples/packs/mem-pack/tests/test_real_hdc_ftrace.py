from __future__ import annotations

import os
import subprocess

import pytest
from kat.pack.datasources.ftrace import FtraceProvider

_TARGET = os.environ.get("KAT_HDC_TARGET")
_REMOTE_TRACE = "/data/local/tmp/kat-ftrace-provider-real.ftrace"


pytestmark = pytest.mark.skipif(
    not _TARGET,
    reason="requires an explicit KAT_HDC_TARGET",
)


def _run_hdc(*arguments: str) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        ["hdc", *arguments],
        shell=False,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        text=True,
        timeout=30,
    )
    if completed.returncode != 0:
        pytest.fail(
            f"hdc command failed with {completed.returncode}: "
            f"{completed.stdout[-2000:]}"
        )
    return completed


def test_real_hdc_capture_converts_queries_and_reuses_content_hash(tmp_path):
    target = _TARGET
    assert target is not None
    inventory = _run_hdc("list", "targets", "-v").stdout.splitlines()
    matching = [
        line
        for line in inventory
        if line.split()[:1] == [target] and "Connected" in line
    ]
    assert len(matching) == 1, f"target is not uniquely Connected: {inventory}"
    assert _run_hdc("-t", target, "shell", "echo", "ok").stdout.strip() == "ok"

    local_trace = tmp_path / "real.ftrace"
    captured = False
    try:
        _run_hdc(
            "-t",
            target,
            "shell",
            "hitrace",
            "--text",
            "--trace_clock",
            "boot",
            "-b",
            "4096",
            "-t",
            "2",
            "-o",
            _REMOTE_TRACE,
            "sched",
            "freq",
            "idle",
            "irq",
            "binder",
            "graphic",
            "app",
            "ability",
        )
        captured = True
        _run_hdc("-t", target, "shell", "stat", _REMOTE_TRACE)
        _run_hdc("-t", target, "file", "recv", _REMOTE_TRACE, str(local_trace))
    finally:
        if captured:
            _run_hdc("-t", target, "shell", "stat", _REMOTE_TRACE)
            _run_hdc("-t", target, "shell", "rm", "-f", _REMOTE_TRACE)

    assert local_trace.stat().st_size > 0
    workspace_root = tmp_path / "workspace"
    workspace_root.mkdir()
    provider = FtraceProvider(
        source=local_trace,
        clock_domain="boot",
        workspace_root=workspace_root,
    )

    [header] = provider.query("SELECT * FROM text_ftrace_header").to_rows()
    [summary] = provider.query(
        """
        SELECT COUNT(*) AS event_count,
               COUNT(DISTINCT cpu) AS observed_cpu_count,
               MIN(clock_domain) AS clock_domain
        FROM text_ftrace_event
        """
    ).to_rows()
    assert header["entries_in_buffer"] > 0
    assert header["cpu_count"] > 0
    assert summary["event_count"] > 0
    assert summary["observed_cpu_count"] > 0
    assert summary["clock_domain"] == "boot"

    cache_root = workspace_root / ".ftrace-cache"
    [catalog_root] = list(cache_root.iterdir())
    materialized_at = catalog_root.stat().st_mtime_ns

    reused = FtraceProvider(
        source=local_trace,
        clock_domain="boot",
        workspace_root=workspace_root,
    )
    assert reused.query("SELECT COUNT(*) AS count FROM text_ftrace_event").to_rows() == [
        {"count": summary["event_count"]}
    ]
    assert catalog_root.stat().st_mtime_ns == materialized_at
