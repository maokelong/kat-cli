from pathlib import Path

_FIXTURE = Path(__file__).parent / "fixtures" / "typed.ftrace"


def test_workflow_publishes_an_eager_summary_and_reuses_materialization(kat_run, tmp_path):
    source = tmp_path / "capture.ftrace"
    source.write_bytes(_FIXTURE.read_bytes())
    arguments = (
        "--trace-path",
        str(source),
        "--clock-domain",
        "fixture_clock",
    )
    first = kat_run(workflow="summarize-ftrace", arguments=arguments)
    source.unlink()
    second = kat_run(workflow="summarize-ftrace", arguments=arguments)

    expected = [
        {
            "tracer": "nop",
            "supported_event_count": 4,
            "observed_cpu_count": 1,
        }
    ]
    assert first["main"].to_pylist() == expected
    assert second["main"].to_pylist() == expected


def test_workflow_reports_zero_supported_events(kat_run, tmp_path):
    source = tmp_path / "unknown.ftrace"
    source.write_text(
        "".join(
            line
            for line in _FIXTURE.read_text(encoding="utf-8").splitlines(keepends=True)
            if line.startswith("#") or ": custom_event:" in line
        ),
        encoding="utf-8",
    )
    result = kat_run(
        workflow="summarize-ftrace",
        arguments=(
            "--trace-path",
            str(source),
            "--clock-domain",
            "fixture_clock",
        ),
    )

    assert result["main"].to_pylist() == [
        {
            "tracer": "nop",
            "supported_event_count": 0,
            "observed_cpu_count": 0,
        }
    ]
