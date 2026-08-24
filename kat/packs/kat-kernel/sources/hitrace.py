from pathlib import Path

import kat


@kat.source(name="hitrace")
def hitrace(trace: Path):
    """把调用方明确提供的一份 Hitrace capture 解释为时钟与调度事实。"""

    from _kat_hitrace import HitraceSchemaProvider

    return HitraceSchemaProvider(trace)
