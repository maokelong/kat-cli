"""读取 knowledge 中随 SDK 发布的分层知识。"""
from importlib.resources import files

_TOPICS = {
    "index": "knowledge/index.md",
    "authoring": "knowledge/authoring/pack-authoring-flow.md",
    "ftrace": "knowledge/providers/ftrace/guide.md",
    "trace_streamer": "knowledge/providers/trace_streamer/guide.md",
}


def read(topic: str = "index") -> str:
    """返回指定主题的 UTF-8 Markdown；未知主题抛出 ValueError。"""
    if topic not in _TOPICS:
        raise ValueError(f"Unknown SDK knowledge topic: {topic!r}; choose from {tuple(_TOPICS)}")
    return files("kat").joinpath(_TOPICS[topic]).read_text(encoding="utf-8")
