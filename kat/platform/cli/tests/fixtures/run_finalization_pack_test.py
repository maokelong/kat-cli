import json
from pathlib import Path

import pytest


def test_scratch_is_finalized_before_kat_run_returns(kat_run, tmp_path):
    for workflow in ("child", "parent"):
        for mode in ("table", "file", "business_file", "output_file"):
            arguments = ["--mode", mode, "--evidence", str(tmp_path)]
            if mode == "table":
                result = kat_run(workflow=workflow, arguments=arguments)
                assert (result["main"].to_pydict() if workflow == "child" else result) == (
                    {"value": [7]} if workflow == "child" else {})
            else:
                with pytest.raises(pytest.fail.Exception) as raised:
                    kat_run(workflow=workflow, arguments=arguments)
                if mode == "business_file":
                    assert "primary execution failure" in str(raised.value)
                if mode == "output_file":
                    assert "An empty Output dict is invalid" in str(raised.value)
                assert "Scratch root could not be cleaned" not in str(raised.value)
            # 在 fixture 退出前检查，避免 Session 总清理掩盖 Run 收尾缺失。
            for evidence in tmp_path.glob("*.json"):
                scratch = Path(json.loads(evidence.read_text())["scratch"])
                assert not scratch.exists() and not scratch.is_symlink()
                candidate = scratch.parent.parent / "runs" / scratch.name
                assert (candidate / "manifest.json").is_file() == (mode == "table")
                evidence.unlink()
