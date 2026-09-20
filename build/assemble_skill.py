"""把 KAT Skills 集合及三个黑盒输入装配到全新 deployment view。"""

from __future__ import annotations

import argparse
import shutil
import tempfile
from pathlib import Path


SKILL_NAMES = ("kat", "kat-analyze", "kat-author", "kat-review", "kat-dev-sdk")


class AssemblyError(ValueError):
    """Skill deployment view 无法由给定黑盒输入唯一组成。"""


def _directory(path: Path, label: str) -> Path:
    if not path.is_dir():
        raise AssemblyError(f"{label} directory is missing: {path}")
    if path.is_symlink():
        raise AssemblyError(f"{label} directory must not be a symbolic link: {path}")
    return path.resolve()


def _overlap(left: Path, right: Path) -> bool:
    return left == right or left.is_relative_to(right) or right.is_relative_to(left)


def _validate_symlink_closure(root: Path, label: str) -> None:
    for path in root.rglob("*"):
        if not path.is_symlink():
            continue
        if path.readlink().is_absolute():
            raise AssemblyError(f"{label} contains an absolute symbolic link: {path}")
        try:
            target = path.resolve(strict=True)
        except FileNotFoundError as error:
            raise AssemblyError(
                f"{label} contains a dangling symbolic link: {path}"
            ) from error
        if not target.is_relative_to(root):
            raise AssemblyError(
                f"{label} contains a symbolic link that escapes its input directory: {path}"
            )


def _validate_skills_source(root: Path) -> None:
    # 顶层只接受本次成套发布的五个入口，避免把旁路文件意外装入归档。
    unexpected = {path.name for path in root.iterdir()} - set(SKILL_NAMES)
    if unexpected:
        raise AssemblyError(f"unexpected entries in Skills source: {sorted(unexpected)}")
    for name in SKILL_NAMES:
        skill = _directory(root / name, f"Skill {name}")
        entry = skill / "SKILL.md"
        if not entry.is_file():
            raise AssemblyError(f"Skill {name} SKILL.md file is missing: {entry}")
        try:
            content = entry.read_text(encoding="utf-8")
        except UnicodeDecodeError as error:
            raise AssemblyError(f"Skill {name} SKILL.md must be UTF-8: {entry}") from error
        if not content.strip():
            raise AssemblyError(f"Skill {name} SKILL.md is empty: {entry}")


def _validated_inputs(
    skills_source: Path,
    packs: Path,
    linux_payload: Path,
    windows_payload: Path,
    output: Path,
) -> tuple[Path, Path, Path, Path, Path]:
    skills_source = _directory(skills_source, "Skills source")
    packs = _directory(packs, "Bundled PACK source")
    linux_payload = _directory(linux_payload, "Linux Platform Payload")
    windows_payload = _directory(windows_payload, "Windows Platform Payload")
    if output.exists() or output.is_symlink():
        raise AssemblyError(
            f"output already exists; refusing to merge deployment views: {output}"
        )
    output = output.resolve()

    sources = (
        ("Skills source", skills_source),
        ("Bundled PACK source", packs),
        ("Linux Platform Payload", linux_payload),
        ("Windows Platform Payload", windows_payload),
    )
    for label, source in sources:
        _validate_symlink_closure(source, label)
    for index, (left_label, left) in enumerate(sources):
        if _overlap(left, output):
            raise AssemblyError(f"{left_label} overlaps output: {left} and {output}")
        for right_label, right in sources[index + 1 :]:
            if _overlap(left, right):
                raise AssemblyError(
                    f"assembly inputs overlap: {left_label} {left} and {right_label} {right}"
                )

    _validate_skills_source(skills_source)
    return skills_source, packs, linux_payload, windows_payload, output


def assemble_skill(
    *,
    skills_source: Path,
    packs: Path,
    linux_payload: Path,
    windows_payload: Path,
    output: Path,
) -> Path:
    """把五个同级 Skill 发布到集合根；调用方须独占缺席的 output 路径。"""
    skills_source, packs, linux_payload, windows_payload, output = _validated_inputs(
        skills_source,
        packs,
        linux_payload,
        windows_payload,
        output,
    )

    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(prefix=f".{output.name}-assembly-", dir=output.parent)
    )
    try:
        shutil.copytree(skills_source, staging, dirs_exist_ok=True, symlinks=True)
        shared = staging / "kat"
        shutil.copytree(packs, shared / "assets" / "packs", symlinks=True)
        shutil.copytree(
            linux_payload,
            shared / "scripts" / "targets" / "linux-x86_64",
            symlinks=True,
        )
        shutil.copytree(
            windows_payload,
            shared / "scripts" / "targets" / "windows-x86_64",
            symlinks=True,
        )
        # 同目录 rename 是单写入者流程的提交点；调用方负责在装配期间独占 output。
        staging.rename(output)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise

    return output


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Assemble five KAT Skills, bundled PACKs, and both platform payloads."
    )
    parser.add_argument("--skills-source", required=True, type=Path)
    parser.add_argument("--packs", required=True, type=Path)
    parser.add_argument("--linux-payload", required=True, type=Path)
    parser.add_argument("--windows-payload", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser


def main() -> int:
    arguments = _parser().parse_args()
    assemble_skill(
        skills_source=arguments.skills_source,
        packs=arguments.packs,
        linux_payload=arguments.linux_payload,
        windows_payload=arguments.windows_payload,
        output=arguments.output,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
