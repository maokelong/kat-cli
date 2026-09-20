"""演示无外部依赖的公共函数。"""

__all__ = ["build_greeting"]


def build_greeting(name: str) -> str:
    """生成去除姓名首尾空白后的中文问候语。

    Args:
        name: 非空姓名，允许中文及其他 Unicode 字符。

    Returns:
        格式为“你好，姓名！”的字符串。

    Raises:
        TypeError: name 不是字符串。
        ValueError: name 去除首尾空白后为空。

    Examples:
        >>> build_greeting(" KAT ")
        '你好，KAT！'
    """
    if not isinstance(name, str):
        raise TypeError("name must be a string")
    normalized = name.strip()
    if not normalized:
        raise ValueError("name must not be blank")
    return f"你好，{normalized}！"
