from pathlib import Path

import kat

from ..decoders.smaps import mappings_reader, snapshots_reader


@kat.source(name="raw_smaps")
def raw_smaps(files: tuple[Path, ...]):
    """把调用方明确提供的已采集 SMAPS 文件解释为快照与映射事实。"""

    captured_files = tuple(files)
    return kat.schema_from_readers(
        {
            "mappings": lambda: mappings_reader(captured_files),
            "snapshots": lambda: snapshots_reader(captured_files),
        }
    )
