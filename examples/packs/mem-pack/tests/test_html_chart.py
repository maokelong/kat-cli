from pathlib import Path

import pyarrow as pa
import pytest
from kat import dataprovider as dp
from kat.pack.helpers.html_chart import (
    BarChartUnit,
    HtmlReport,
    LineChartUnit,
    PieChartUnit,
    TableUnit,
)


def _table() -> dp.Table:
    return dp.Table.from_arrow(
        pa.table({"sample": ["first", "second"], "duration": [10, 25]})
    )


def test_writes_multiple_chart_units_across_tabs(tmp_path: Path):
    destination = tmp_path / "report.html"
    report = HtmlReport("Duration report")
    overview = report.add_tab("Overview")
    overview.add(LineChartUnit(_table(), x="sample", y="duration", title="Trend"))
    overview.add(
        PieChartUnit(_table(), category="sample", value="duration", title="Share")
    )
    overview.add(
        BarChartUnit(_table(), category="sample", value="duration", title="Bars")
    )
    report.add_tab("Details").add(
        TableUnit(_table(), title="Details", columns=("sample", "duration"))
    )

    report.write(destination)

    html = destination.read_text(encoding="utf-8")
    assert html.count('class="tab-button"') == 2
    assert html.count('<section class="chart-card') == 4
    assert '"type": "line"' in html
    assert '"type": "pie"' in html
    assert '"type": "bar"' in html
    assert "<th>sample</th>" in html
    assert "<td>first</td>" in html
    assert "activateTab" in html


def test_rejects_report_without_chart_units(tmp_path: Path):
    report = HtmlReport("Empty")
    report.add_tab("Overview")

    with pytest.raises(ValueError, match="report must contain at least one HTML unit"):
        report.write(tmp_path / "report.html")


def test_rejects_missing_column():
    report = HtmlReport("Missing")

    with pytest.raises(ValueError, match="chart column does not exist: missing"):
        report.add_tab("Overview").add(
            LineChartUnit(_table(), x="sample", y="missing", title="Missing")
        )


def test_table_unit_limits_rows(tmp_path: Path):
    report = HtmlReport("Table")
    report.add_tab("Details").add(TableUnit(_table(), title="Rows", max_rows=1))

    destination = tmp_path / "table.html"
    report.write(destination)

    html = destination.read_text(encoding="utf-8")
    assert "<td>first</td>" in html
    assert "<td>second</td>" not in html
