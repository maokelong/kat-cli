from kat.dataprovider.ftrace import FtraceProvider as PublicFtraceProvider

import kat


@kat.provider(
    name="ftrace-text",
    description="将 tracefs 文本解码为可重复查询的类型化关系。",
    guide="providers/ftrace.md",
)
class FtraceProvider(PublicFtraceProvider):
    """为 mem-pack 声明公共文本 Ftrace Provider 及领域知识。"""
