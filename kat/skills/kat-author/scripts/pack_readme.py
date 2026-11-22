"""从公开 inspection 更新 PACK README 的事实区，保留作者说明。"""

from __future__ import annotations

import argparse
import html
import json
from pathlib import Path
import subprocess
import sys
import tomllib
from urllib.parse import unquote, urlsplit


START = "<!-- kat:capabilities:start -->"
END = "<!-- kat:capabilities:end -->"


def cell(value: object) -> str:
    return html.escape(str(value)).replace("|", "&#124;").replace("\n", "<br>")


def render_facts(pack: dict, entries: list[dict], workflows: list[dict]) -> str:
    lines = [START, "## 当前能力（自动同步）", "",
             f"**{cell(pack['title'])}**（`{pack['name']}`）；维护方：{cell(pack['owner'])}。",
             "", cell(pack["description"]), "", "### 知识入口", ""]
    for entry in sorted(entries, key=lambda item: item["name"]):
        name = entry["name"]
        flags = []
        if entry.get("user-invocable") is False:
            flags.append("不提供用户调用选项")
        if entry.get("disable-model-invocation") is True:
            flags.append("不由模型自动选用" if entry.get("user-invocable") is False else "只能由用户主动选用")
        policy = f"（{'；'.join(flags)}）" if flags else ""
        lines.append(f"- [{name}](skills/{name}/SKILL.md)：{cell(entry['description'])}{policy}")
    if not entries:
        lines.append("尚无已发布知识入口。")
    lines.extend(["", "### Workflow", ""])
    for workflow in sorted(workflows, key=lambda item: item["name"]):
        name = workflow["name"]
        lines.extend([f"#### `{name}`", "", cell(workflow["description"]), "",
                      f"输入合同：`kat inspect workflow --pack {pack['name']} --workflow {name}`。外部 PACK 继续携带 `--pack-dir <PACK绝对目录>`。", ""])
        if not workflow["parameters"]:
            lines.append("无需业务参数。")
        else:
            lines.extend(["| 参数 | 类型 | 必填 | 默认值 | 可选值 | 含义 |",
                          "| --- | --- | --- | --- | --- | --- |"])
            for parameter in workflow["parameters"]:
                options = parameter["option"]
                if "negative_option" in parameter:
                    options += " / " + parameter["negative_option"]
                default = json.dumps(parameter["default"], ensure_ascii=False) if "default" in parameter else "—"
                choices = json.dumps(parameter["choices"], ensure_ascii=False) if "choices" in parameter else "—"
                values = [options, parameter["type"], "是" if parameter["required"] else "否",
                          default, choices, parameter["description"]]
                lines.append("| " + " | ".join(cell(value) for value in values) + " |")
        lines.append("")
    if not workflows:
        lines.extend(["尚无已发布 Workflow。", ""])
    lines.extend(["Provider 与普通函数的复用说明见作者维护的正文；它们不是直接分析入口。", END])
    return "\n".join(lines)


def initial_readme(pack: dict) -> str:
    return (f"# {pack['title']}\n\n{pack['description']}\n\n"
            "## 使用场景与示例\n\n尚无已发布业务能力；完成能力后在此补充可直接提出的自然语言问题。\n\n"
            "## 输入、结果与限制\n\n当前仅为创作骨架，不能用于得出业务结论。\n\n"
            "## 开发者复用\n\n尚无已发布 Provider 或公共函数。\n\n"
            + render_facts(pack, [], []) + "\n")


def replace_facts(current: str, facts: str) -> str:
    newline = "\r\n" if "\r\n" in current else "\n"
    facts = facts.replace("\n", newline)
    if START not in current and END not in current:
        return current.rstrip() + newline * 2 + facts + newline
    if current.count(START) != 1 or current.count(END) != 1 or current.index(START) >= current.index(END):
        raise ValueError("README capability markers must be one ordered pair")
    before, rest = current.split(START, 1)
    _, after = rest.split(END, 1)
    return before + facts + after


def inspect(kat: Path, *arguments: str) -> dict:
    result = subprocess.run([str(kat), "inspect", *arguments], capture_output=True,
                            text=True, encoding="utf-8", check=False)
    try:
        response = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(f"inspection returned no valid Response: {result.stderr or result.stdout}") from error
    if result.returncode or response.get("status") != "success":
        raise ValueError(f"inspection failed: {response}")
    return response["result"]


def collect_facts(kat: Path, directory: Path) -> str:
    with (directory / "pack.toml").open("rb") as stream:
        name = tomllib.load(stream)["name"]
    packs = inspect(kat, "--pack-dir", str(directory))["packs"]
    pack = next((pack for pack in packs if pack["name"] == name), None)
    if pack is None:
        raise ValueError(f"PACK was not returned by inspection: {name}")
    routing = ("--pack", name, "--pack-dir", str(directory))
    entries = inspect(kat, "knowledge", *routing)["entries"]
    workflows = []
    source = directory / "workflows"
    if source.is_symlink() or (source.exists() and not source.is_dir()):
        raise ValueError("PACK workflows must be an ordinary directory")
    # 仅检查是否需要 Host；声明、重名、用途与参数始终由 Runtime 校验。
    if any(source.rglob("*.py")):
        summaries = inspect(kat, "workflow", *routing)["workflows"]
        for summary in summaries:
            workflows.append(inspect(kat, "workflow", *routing, "--workflow", summary["name"])["workflow"])
    return render_facts(pack, entries, workflows)


def validate_links(directory: Path, content: str) -> None:
    try:
        from markdown_it import MarkdownIt
    except ImportError as error:
        raise ValueError("README link validation requires markdown-it-py==4.2.0 in the script's Python environment") from error
    for block in MarkdownIt("commonmark").parse(content):
        for token in block.children or ():
            if token.type not in ("link_open", "image"):
                continue
            reference = token.attrGet("href" if token.type == "link_open" else "src") or ""
            link = urlsplit(reference)
            if link.scheme or link.netloc or not link.path:
                continue
            target = (directory / unquote(link.path)).resolve()
            if not target.is_relative_to(directory) or not target.is_file():
                raise ValueError(f"README has a broken or out-of-PACK link: {reference}")


def update_readme(kat: Path, directory: Path, *, check: bool = False) -> Path:
    directory = directory.resolve(strict=True)
    document = directory / "README.md"
    if document.is_symlink():
        raise ValueError("PACK README must be an ordinary file, not a symlink")
    current = document.read_bytes().decode("utf-8") if document.exists() else ""
    facts = collect_facts(kat, directory)
    updated = replace_facts(current, facts)
    validate_links(directory, updated)
    if check:
        if current != updated:
            raise ValueError("README capability facts are missing or stale; regenerate and review the usage text")
    elif current != updated:
        document.write_bytes(updated.encode("utf-8"))
    return document


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kat", required=True, type=Path)
    parser.add_argument("--pack-dir", required=True, type=Path)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    try:
        print(update_readme(options.kat.resolve(strict=True), options.pack_dir, check=options.check))
    except (OSError, ValueError, KeyError) as error:
        print(f"README update failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
