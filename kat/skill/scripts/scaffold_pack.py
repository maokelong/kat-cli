"""在指定 PACK 集合目录中创建最小、可发现的 PACK 创作骨架。"""

from __future__ import annotations

import argparse
import json
import shutil
import tempfile
from pathlib import Path


class ScaffoldError(ValueError):
    """请求无法安全地生成唯一 PACK 目录。"""


_WINDOWS_DEVICE_NAMES = {"con", "prn", "aux", "nul"} | {
    f"{prefix}{number}"
    for prefix in ("com", "lpt")
    for number in range(1, 10)
}

_DIRECTORIES = (
    ("workflows", "存放带 @kat.workflow 声明的 Workflow 入口。"),
    ("datasources", "存放 PACK 私有的数据源解析与 Provider 实现。"),
    ("helpers", "存放仅供当前 PACK 复用的普通 Python 模块。"),
    ("knowledge", "汇总当前 PACK 随源码发布的 Agent 创作知识。"),
    ("knowledge/workflows", "存放 Workflow 的分析与结果解释 guide。"),
    ("knowledge/providers", "存放 Provider 的数据结构与接入 guide。"),
    ("tests", "存放通过 kat test 执行的 PACK 测试。"),
)


def _validate_name(name: str) -> None:
    valid = bool(name) and all(
        segment
        and all(
            character.isascii() and (character.islower() or character.isdigit())
            for character in segment
        )
        for segment in name.split("-")
    )
    if not valid:
        raise ScaffoldError("PACK name 必须是小写 ASCII kebab-case")
    if name in _WINDOWS_DEVICE_NAMES:
        raise ScaffoldError("PACK name 不能是 Windows 保留设备名")


def _required_text(value: str, field: str) -> str:
    normalized = value.strip()
    if not normalized:
        raise ScaffoldError(f"{field} 不能为空")
    return normalized


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def scaffold_pack(
    *, packs_directory: Path, name: str, title: str, description: str, owner: str
) -> dict[str, object]:
    """创建一个新 PACK，并返回供 Agent 回报的结构化目录说明。"""
    _validate_name(name)
    title = _required_text(title, "title")
    description = _required_text(description, "description")
    owner = _required_text(owner, "owner")

    if not packs_directory.is_absolute():
        packs_directory = Path.cwd() / packs_directory
    packs_directory = packs_directory.resolve()
    if packs_directory.exists() and not packs_directory.is_dir():
        raise ScaffoldError(f"PACK 集合路径不是目录: {packs_directory}")
    packs_directory.mkdir(parents=True, exist_ok=True)

    target = packs_directory / name
    if target.exists() or target.is_symlink():
        raise ScaffoldError(f"目标 PACK 已存在，拒绝覆盖: {target}")

    staging = Path(tempfile.mkdtemp(prefix=f".{name}-scaffold-", dir=packs_directory))
    entries: list[dict[str, str]] = [
        {"path": "pack.toml", "purpose": "声明 PACK 的名称、展示信息和维护责任。"},
        {"path": "README.md", "purpose": "记录 PACK 定位、目录约定和后续创作入口。"},
    ]
    try:
        (staging / "pack.toml").write_text(
            "\n".join(
                (
                    f"name = {_toml_string(name)}",
                    f"title = {_toml_string(title)}",
                    f"description = {_toml_string(description)}",
                    f"owner = {_toml_string(owner)}",
                    "",
                )
            ),
            encoding="utf-8",
        )
        directory_lines = []
        for relative_path, purpose in _DIRECTORIES:
            (staging / relative_path).mkdir(parents=True, exist_ok=True)
            entries.append({"path": f"{relative_path}/", "purpose": purpose})
            directory_lines.append(f"- `{relative_path}/`：{purpose}")
        (staging / "README.md").write_text(
            f"# {title}\n\n{description}\n\n## 目录\n\n"
            + "\n".join(directory_lines)
            + "\n\n只添加当前领域真实需要的 Workflow、Datasource、Helper、Guide 和测试；"
            "不要用占位实现冒充可运行能力。\n",
            encoding="utf-8",
        )
        staging.rename(target)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise

    return {"pack_directory": str(target), "entries": entries}


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="在 packs 目录下创建最小 KAT PACK 骨架。")
    parser.add_argument("--packs-dir", required=True, type=Path)
    parser.add_argument("--name", required=True)
    parser.add_argument("--title", required=True)
    parser.add_argument("--description", required=True)
    parser.add_argument("--owner", required=True)
    return parser


def main() -> int:
    arguments = _parser().parse_args()
    try:
        result = scaffold_pack(
            packs_directory=arguments.packs_dir,
            name=arguments.name,
            title=arguments.title,
            description=arguments.description,
            owner=arguments.owner,
        )
    except ScaffoldError as error:
        raise SystemExit(str(error)) from error
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
