from __future__ import annotations

import logging
from pathlib import Path


_LOGGER = logging.getLogger(__name__)


class WorkflowOperation:
    """Keep path capabilities valid only during one Workflow call."""

    def __init__(
        self,
        datasource_root: Path,
        scratch_root: Path,
    ) -> None:
        self._datasource_root = datasource_root
        self._scratch_root = scratch_root
        self._active = True

    def require_active(self) -> None:
        if not self._active:
            raise RuntimeError("Workflow execution lease is no longer active")

    @property
    def datasource_root(self) -> Path:
        return self._prepare_root(self._datasource_root, "Datasource", create=True)

    @property
    def scratch_root(self) -> Path:
        return self._prepare_root(self._scratch_root, "Scratch")

    def _prepare_root(self, root: Path, label: str, *, create: bool = False) -> Path:
        self.require_active()
        try:
            if root.is_symlink() or _is_junction(root):
                raise OSError(f"{label} root must not be a link")
            if create:
                root.mkdir(parents=True, exist_ok=True)
            resolved = root.resolve(strict=True)
            parent = root.parent.resolve(strict=True)
        except (OSError, RuntimeError):
            _LOGGER.exception("failed to prepare the private %s root", label)
            raise RuntimeError(f"{label} root could not be prepared") from None
        if resolved != root or resolved.parent != parent or not resolved.is_dir():
            raise RuntimeError(f"{label} root is not a canonical directory")
        return resolved

    def expire(self) -> None:
        self._active = False


def _is_junction(path: Path) -> bool:
    checker = getattr(path, "is_junction", None)
    return bool(checker()) if checker is not None else False
