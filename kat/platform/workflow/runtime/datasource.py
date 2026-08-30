from __future__ import annotations

import logging
from pathlib import Path


_LOGGER = logging.getLogger(__name__)


class WorkflowOperation:
    """Own the path capabilities and lease for one Workflow run."""

    def __init__(
        self,
        datasource_root: Path,
    ) -> None:
        self._datasource_root = datasource_root
        self._active = True

    def require_active(self) -> None:
        if not self._active:
            raise RuntimeError("Workflow execution lease is no longer active")

    @property
    def datasource_root(self) -> Path:
        self.require_active()
        root = self._datasource_root
        try:
            if root.is_symlink() or _is_junction(root):
                raise OSError("Datasource root must not be a link")
            root.mkdir(parents=True, exist_ok=True)
            resolved = root.resolve(strict=True)
            parent = root.parent.resolve(strict=True)
        except (OSError, RuntimeError):
            _LOGGER.exception("failed to prepare the private Datasource root")
            raise RuntimeError("Datasource root could not be prepared") from None
        if resolved != root or resolved.parent != parent or not resolved.is_dir():
            raise RuntimeError("Datasource root is not a canonical directory")
        return resolved

    def expire(self) -> None:
        self._active = False


def _is_junction(path: Path) -> bool:
    checker = getattr(path, "is_junction", None)
    return bool(checker()) if checker is not None else False
