from kat.pack.datasources.hitrace import HitraceProvider


def test_pack_owned_provider_is_importable() -> None:
    assert HitraceProvider.__module__ == "kat.pack.datasources.hitrace"
