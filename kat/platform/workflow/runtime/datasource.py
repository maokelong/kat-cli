from __future__ import annotations

import logging
from pathlib import Path
import shutil


_LOGGER = logging.getLogger(__name__)


class WorkflowOperation:
    """Own the path capabilities and lease for one Workflow run."""

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
        return self._prepare_root(self._datasource_root, "Datasource")

    @property
    def scratch_root(self) -> Path:
        return self._prepare_root(self._scratch_root, "Scratch")

    def _prepare_root(self, root: Path, label: str) -> Path:
        self.require_active()
        try:
            if root.is_symlink() or _is_junction(root):
                raise OSError(f"{label} root must not be a link")
            root.mkdir(parents=True, exist_ok=True)
            resolved = root.resolve(strict=True)
            parent = root.parent.resolve(strict=True)
        except (OSError, RuntimeError):
            _LOGGER.exception("failed to prepare the private %s root", label)
            raise RuntimeError(f"{label} root could not be prepared") from None
        if resolved != root or resolved.parent != parent or not resolved.is_dir():
            raise RuntimeError(f"{label} root is not a canonical directory")
        return resolved

    def cleanup_scratch(self) -> None:
        root = self._scratch_root
        try:
            if root.is_symlink() or _is_junction(root):
                raise OSError("Scratch root must remain an ordinary directory")
            if not root.exists():
                return
            if not root.is_dir():
                raise OSError("Scratch root must remain an ordinary directory")
            resolved = root.resolve(strict=True)
            parent = root.parent.resolve(strict=True)
            if resolved != root or resolved.parent != parent:
                raise OSError("Scratch root must remain canonical")
            shutil.rmtree(root)
            if root.exists() or root.is_symlink():
                raise OSError("Scratch root still exists after cleanup")
        except FileNotFoundError:
            return
        except (OSError, RuntimeError):
            _LOGGER.exception("failed to clean the private Scratch root")
            raise RuntimeError("Scratch root could not be cleaned") from None

    def expire(self) -> None:
        self._active = False


def _is_junction(path: Path) -> bool:
    checker = getattr(path, "is_junction", None)
    return bool(checker()) if checker is not None else False
