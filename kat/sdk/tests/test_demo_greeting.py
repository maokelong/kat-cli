import pytest

from kat_sdk.helpers.demo.greeting import build_greeting


@pytest.mark.parametrize(
    ("name", "expected"),
    [("KAT", "你好，KAT！"), (" 小明 ", "你好，小明！"), ("A B", "你好，A B！")],
)
def test_build_greeting(name, expected):
    assert build_greeting(name) == expected


@pytest.mark.parametrize("name", ["", " \t\n"])
def test_blank_name_is_rejected(name):
    with pytest.raises(ValueError, match="blank"):
        build_greeting(name)


def test_non_string_name_is_rejected():
    with pytest.raises(TypeError, match="string"):
        build_greeting(None)
