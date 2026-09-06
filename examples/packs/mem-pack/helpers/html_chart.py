from __future__ import annotations

import json
from pathlib import Path

import pyarrow as pa
from kat import dataprovider as dp

_ECHARTS_URL = "https://cdn.jsdelivr.net/npm/echarts@6.0.0/dist/echarts.min.js"


class HtmlReport:
    """Manage tabs containing Table-backed chart units."""

    def __init__(self, title: str) -> None:
        self._title = title
        self._tabs: list[HtmlTab] = []

    def add_tab(self, title: str) -> HtmlTab:
        tab = HtmlTab(title)
        self._tabs.append(tab)
        return tab

    def write(self, destination: Path) -> None:
        if not isinstance(destination, Path):
            raise TypeError("report destination must be a pathlib.Path")
        if not destination.parent.is_dir():
            raise ValueError("report destination parent must be an existing directory")
        if not any(tab._units for tab in self._tabs):
            raise ValueError("report must contain at least one HTML unit")
        destination.write_text(self._render(), encoding="utf-8")

    def _render(self) -> str:
        buttons = []
        panels = []
        options = []
        for tab_index, tab in enumerate(self._tabs):
            selected = tab_index == 0
            buttons.append(
                f'<button class="tab-button" role="tab" '
                f'aria-selected="{str(selected).lower()}" '
                f'data-tab="{tab_index}">{_escape_html(tab.title)}</button>'
            )
            units = []
            for unit_index, unit in enumerate(tab._units):
                chart_id = f"chart-{tab_index}-{unit_index}"
                if unit.option is None:
                    units.append(
                        f'<section class="chart-card table-card"><h2>'
                        f'{_escape_html(unit.title)}</h2>{unit.content}</section>'
                    )
                else:
                    units.append(
                        f'<section class="chart-card"><div id="{chart_id}" '
                        f'class="chart" role="img" '
                        f'aria-label="{_escape_html(unit.title)}"></div></section>'
                    )
                    options.append({"id": chart_id, "option": unit.option})
            hidden = "" if selected else " hidden"
            panels.append(
                f'<div class="tab-panel" role="tabpanel" data-panel="{tab_index}"'
                f'{hidden}><div class="chart-grid">{"".join(units)}</div></div>'
            )

        return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{_escape_html(self._title)}</title>
  <style>
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; padding: 32px; background: #f3f6fa; color: #172033; font-family: system-ui, sans-serif; }}
    .report {{ max-width: 1200px; margin: auto; }}
    h1 {{ margin: 0 0 24px; font-size: 30px; }}
    .tabs {{ display: flex; gap: 8px; margin-bottom: 20px; border-bottom: 1px solid #dbe3ee; }}
    .tab-button {{ padding: 10px 16px; border: 0; border-bottom: 3px solid transparent; background: transparent; color: #64748b; cursor: pointer; font: inherit; }}
    .tab-button[aria-selected="true"] {{ border-bottom-color: #2563eb; color: #172033; font-weight: 650; }}
    .tab-panel[hidden] {{ display: none; }}
    .chart-grid {{ display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 20px; }}
    .chart-card {{ min-width: 0; background: white; border: 1px solid #e2e8f0; border-radius: 14px; padding: 16px; box-shadow: 0 8px 28px rgba(30, 41, 59, .07); }}
    .chart {{ height: 420px; }}
    .table-card {{ grid-column: 1 / -1; }}
    .table-card h2 {{ margin: 4px 0 16px; font-size: 18px; }}
    .table-scroll {{ overflow: auto; max-height: 420px; }}
    table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
    th, td {{ padding: 10px 12px; border-bottom: 1px solid #e2e8f0; text-align: left; white-space: nowrap; }}
    th {{ position: sticky; top: 0; background: #f8fafc; color: #475569; }}
    @media (max-width: 800px) {{ .chart-grid {{ grid-template-columns: 1fr; }} }}
  </style>
</head>
<body>
  <main class="report">
    <h1>{_escape_html(self._title)}</h1>
    <nav class="tabs" role="tablist">{"".join(buttons)}</nav>
    {"".join(panels)}
  </main>
  <script src="{_ECHARTS_URL}"></script>
  <script>
    const charts = new Map();
    for (const item of {_json_for_script(options)}) {{
      const chart = echarts.init(document.getElementById(item.id));
      chart.setOption(item.option);
      charts.set(item.id, chart);
    }}
    function activateTab(index) {{
      document.querySelectorAll(".tab-button").forEach(button =>
        button.setAttribute("aria-selected", String(button.dataset.tab === index)));
      document.querySelectorAll(".tab-panel").forEach(panel =>
        panel.hidden = panel.dataset.panel !== index);
      requestAnimationFrame(() => charts.forEach(chart => chart.resize()));
    }}
    document.querySelectorAll(".tab-button").forEach(button =>
      button.addEventListener("click", () => activateTab(button.dataset.tab)));
    window.addEventListener("resize", () => charts.forEach(chart => chart.resize()));
  </script>
</body>
</html>
"""


class HtmlTab:
    def __init__(self, title: str) -> None:
        self.title = title
        self._units: list[HtmlUnit] = []

    def add(self, unit: HtmlUnit) -> None:
        if not isinstance(unit, HtmlUnit):
            raise TypeError("tab unit must be an HtmlUnit")
        self._units.append(unit)


class HtmlUnit:
    def __init__(
        self,
        title: str,
        option: dict[str, object] | None,
        content: str = "",
    ) -> None:
        self.title = title
        self.option = option
        self.content = content


class LineChartUnit(HtmlUnit):
    def __init__(self, table: dp.Table, *, x: str, y: str, title: str) -> None:
        selected = _selected_table(table, (x, y))
        _require_numeric(selected, y)
        super().__init__(
            title,
            {
                "title": {"text": title, "left": "center"},
                "tooltip": {"trigger": "axis"},
                "xAxis": {"type": "category", "data": selected[x].to_pylist()},
                "yAxis": {"type": "value"},
                "series": [{"name": y, "type": "line", "data": selected[y].to_pylist()}],
            },
        )


class PieChartUnit(HtmlUnit):
    def __init__(
        self, table: dp.Table, *, category: str, value: str, title: str
    ) -> None:
        selected = _selected_table(table, (category, value))
        _require_numeric(selected, value)
        super().__init__(
            title,
            {
                "title": {"text": title, "left": "center"},
                "tooltip": {"trigger": "item"},
                "legend": {"orient": "vertical", "left": "left"},
                "series": [{
                    "name": value,
                    "type": "pie",
                    "radius": "65%",
                    "data": [
                        {"name": name, "value": amount}
                        for name, amount in zip(
                            selected[category].to_pylist(),
                            selected[value].to_pylist(),
                            strict=True,
                        )
                    ],
                    }],
            },
        )


class BarChartUnit(HtmlUnit):
    def __init__(
        self, table: dp.Table, *, category: str, value: str, title: str
    ) -> None:
        selected = _selected_table(table, (category, value))
        _require_numeric(selected, value)
        super().__init__(
            title,
            {
                "title": {"text": title, "left": "center"},
                "tooltip": {"trigger": "axis"},
                "xAxis": {
                    "type": "category",
                    "data": selected[category].to_pylist(),
                },
                "yAxis": {"type": "value"},
                "series": [
                    {
                        "name": value,
                        "type": "bar",
                        "data": selected[value].to_pylist(),
                    }
                ],
            },
        )


class TableUnit(HtmlUnit):
    def __init__(
        self,
        table: dp.Table,
        *,
        title: str,
        columns: tuple[str, ...] | None = None,
        max_rows: int = 100,
    ) -> None:
        if type(table) is not dp.Table:
            raise TypeError("table unit source must be a dp.Table")
        if type(max_rows) is not int or max_rows <= 0:
            raise ValueError("table unit max_rows must be a positive integer")
        arrow_table = table.to_arrow()
        selected_columns = tuple(arrow_table.column_names) if columns is None else columns
        missing = [name for name in selected_columns if name not in arrow_table.column_names]
        if missing:
            raise ValueError(f"table unit column does not exist: {missing[0]}")
        selected = arrow_table.select(selected_columns).slice(0, max_rows)
        header = "".join(f"<th>{_escape_html(name)}</th>" for name in selected_columns)
        rows = "".join(
            "<tr>"
            + "".join(f"<td>{_escape_html(_display_value(value))}</td>" for value in row.values())
            + "</tr>"
            for row in selected.to_pylist()
        )
        content = (
            '<div class="table-scroll"><table><thead><tr>'
            f"{header}</tr></thead><tbody>{rows}</tbody></table></div>"
        )
        super().__init__(title, None, content)


def _selected_table(table: dp.Table, columns: tuple[str, str]) -> pa.Table:
    if type(table) is not dp.Table:
        raise TypeError("chart table must be a dp.Table")
    arrow_table = table.to_arrow()
    missing = [name for name in columns if name not in arrow_table.column_names]
    if missing:
        raise ValueError(f"chart column does not exist: {missing[0]}")
    return arrow_table.select(columns)


def _require_numeric(table: pa.Table, column: str) -> None:
    column_type = table.schema.field(column).type
    if not (pa.types.is_integer(column_type) or pa.types.is_floating(column_type)):
        raise ValueError(f"chart value column must be numeric: {column}")


def _json_for_script(value: object) -> str:
    return json.dumps(value, ensure_ascii=False, default=_json_default).replace("<", "\\u003c")


def _json_default(value: object) -> object:
    isoformat = getattr(value, "isoformat", None)
    if callable(isoformat):
        return isoformat()
    raise TypeError(f"unsupported chart value: {type(value).__name__}")


def _display_value(value: object) -> str:
    return "—" if value is None else str(value)


def _escape_html(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&#x27;")
    )
