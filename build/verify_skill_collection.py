"""只读检查解压后的 KAT Skills 路径合同；真实运行由平台 smoke 验证。"""

from __future__ import annotations

import argparse
import re
from pathlib import Path
from urllib.parse import unquote, urlsplit


SKILLS = ("kat", "kat-analyze", "kat-author", "kat-review")


def _file(path: Path) -> None:
    if not path.is_file():
        raise ValueError(f"Required collection file is missing: {path}")


def _local_links(document: Path, root: Path) -> None:
    # 产品文档使用普通 inline link；这里只检查其部署路径，不实现 Markdown renderer。
    content = document.read_text(encoding="utf-8")
    for href in re.findall(r"\[[^\]\n]*\]\(([^\s)]+)\)", content):
        link = urlsplit(href)
        if link.scheme in ("http", "https") or not link.path:
            continue
        relative = Path(unquote(link.path))
        target = (document.parent / relative).resolve()
        if (
            link.scheme
            or link.netloc
            or relative.is_absolute()
            or not target.is_relative_to(root)
        ):
            raise ValueError(f"Local reference escapes collection: {document}: {href}")
        if not target.exists():
            raise ValueError(f"Local reference is missing: {document}: {href}")


def verify_skill_collection(root: Path) -> None:
    root = root.resolve()
    if not root.is_dir() or {path.name for path in root.iterdir()} != set(SKILLS):
        raise ValueError(
            f"Collection must contain exactly these top-level directories: {SKILLS}"
        )

    if (root / "kat/sdk").exists():
        raise ValueError("SDK must be installed in bundled Python, not kat/sdk")

    for name in SKILLS:
        entrypoint = root / name / "SKILL.md"
        _file(entrypoint)
        content = entrypoint.read_text(encoding="utf-8")
        header = content.split("---", 2)
        if len(header) != 3 or header[0].strip() or not re.search(
            rf"^name: {re.escape(name)}$", header[1], re.MULTILINE
        ):
            raise ValueError(f"Skill name does not match its directory: {entrypoint}")
        _local_links(entrypoint, root)
        for document in (root / name / "references").rglob("*.md"):
            _local_links(document, root)
        if name != "kat":
            reference = "../kat/references/command-reference.md"
            if f"]({reference})" not in content:
                raise ValueError(
                    "Task Skill must link the adjacent shared command contract: "
                    f"{entrypoint}"
                )
            for owned_path in ("scripts/targets", "assets/packs"):
                if (root / name / owned_path).exists():
                    raise ValueError(
                        "Task Skill must use the shared kat payload: "
                        f"{root / name / owned_path}"
                    )

    router = (root / "kat/SKILL.md").read_text(encoding="utf-8")
    for name in SKILLS[1:]:
        if f"](../{name}/SKILL.md)" not in router:
            raise ValueError(f"Router must link the {name} entrypoint")

    for relative in (
        "kat/references/command-reference.md",
        "kat/scripts/targets/linux-x86_64/kat",
        "kat/scripts/targets/linux-x86_64/python/bin/python3",
        "kat/scripts/targets/windows-x86_64/kat.exe",
        "kat/scripts/targets/windows-x86_64/python/python.exe",
        "kat-author/scripts/scaffold_pack.py",
        "kat-author/references/examples/dataprovider-pack/pack.toml",
    ):
        _file(root / relative)
    if not (root / "kat/assets/packs").is_dir():
        raise ValueError("Shared kat deployment is missing its Bundled PACK directory")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "skills", type=Path, help="Extracted KAT Skills collection directory"
    )
    arguments = parser.parse_args()
    verify_skill_collection(arguments.skills)
    print(
        "KAT Skills collection paths verified; "
        "runtime execution is checked by platform smoke."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
