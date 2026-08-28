from __future__ import annotations

from collections.abc import Mapping
import logging
from pathlib import Path
from types import MappingProxyType


_LOGGER = logging.getLogger(__name__)


class WorkflowOperation:
    """Own the path capabilities and Dataset grants for one Workflow run."""

    def __init__(
        self,
        candidate_path: Path,
        datasource_root: Path,
        dataset_tables: Mapping[str, Path],
    ) -> None:
        self._datasource_root = datasource_root
        self._output_root = candidate_path / "outputs"
        self._dataset_tables = MappingProxyType(dict(dataset_tables))
        self._active = True

    def require_active(self) -> None:
        if not self._active:
            raise RuntimeError("Workflow execution lease is no longer active")

    @property
    def dataset_tables(self) -> Mapping[str, Path]:
        self.require_active()
        return self._dataset_tables

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

    @property
    def output_root(self) -> Path:
        self.require_active()
        return self._output_root

    def expire(self) -> None:
        self._active = False


def _is_junction(path: Path) -> bool:
    checker = getattr(path, "is_junction", None)
    return bool(checker()) if checker is not None else False
