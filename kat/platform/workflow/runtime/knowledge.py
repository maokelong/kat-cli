from __future__ import annotations

from pathlib import Path
import stat


def read_guide(
    root: Path,
    reference: str,
    *,
    declaration: str,
    category: str,
) -> str:
    """Validate and read one raw Markdown guide below ``PACK/knowledge``."""
    relative = Path(reference)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"{declaration} guide must be relative to PACK knowledge/")
    if not relative.parts or relative.parts[0] != category:
        raise ValueError(
            f"{declaration} guide must be under PACK knowledge/{category}/"
        )
    if relative.suffix != ".md":
        raise ValueError(f"{declaration} guide must identify a .md file")
    knowledge = root / "knowledge"
    try:
        knowledge_metadata = knowledge.lstat()
    except OSError as error:
        raise ValueError("PACK knowledge/ must be an ordinary directory") from error
    if not stat.S_ISDIR(knowledge_metadata.st_mode):
        raise ValueError("PACK knowledge/ must be an ordinary directory")
    resolved_knowledge = knowledge.resolve(strict=True)
    if resolved_knowledge != knowledge or not resolved_knowledge.is_relative_to(root):
        raise ValueError("PACK knowledge/ must be an ordinary directory")
    target = knowledge / relative
    try:
        target_metadata = target.lstat()
        resolved_target = target.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{declaration} guide does not exist: {reference}") from error
    if (
        not stat.S_ISREG(target_metadata.st_mode)
        or not resolved_target.is_relative_to(resolved_knowledge)
    ):
        raise ValueError(
            f"{declaration} guide is not an ordinary knowledge file: {reference}"
        )
    try:
        with resolved_target.open("r", encoding="utf-8", newline="") as guide_file:
            contents = guide_file.read()
    except UnicodeError as error:
        raise ValueError(f"{declaration} guide must be UTF-8: {reference}") from error
    if not contents:
        raise ValueError(f"{declaration} guide must not be empty: {reference}")
    return contents
