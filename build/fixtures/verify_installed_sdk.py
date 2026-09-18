"""用发布包自带 Python 验证 SDK 是三个模块的唯一安装所有者。"""
from importlib.metadata import distribution, PackageNotFoundError
from pathlib import Path
from importlib.util import find_spec

import kat
import kat._runtime
import kat.providers.decoding
from kat import Context, knowledge, provider, workflow

for old in ("kat_sdk", "_kat_runtime", "kat_datasource", "kat.api", "kat.runtime", "kat.datasource"):
    assert find_spec(old) is None, old

sdk = distribution("kat-sdk")
files = {str(path) for path in sdk.files or ()}
for module, name in ((kat, "kat"), (kat.dataprovider, "kat.dataprovider"),
                     (knowledge, "kat.knowledge"), (kat._runtime, "kat._runtime"),
                     (kat.providers.decoding, "kat.providers.decoding")):
    entry = name.replace(".", "/") + "/__init__.py"
    assert entry in files
    assert sdk.locate_file(entry).resolve() == Path(module.__file__).resolve()
for name in ("kat-workflow", "kat-datasource", "kat-cli"):
    try:
        distribution(name)
    except PackageNotFoundError:
        pass
    else:
        raise AssertionError(f"split distribution still installed: {name}")
for topic in ("index", "authoring", "ftrace", "trace_streamer"):
    assert knowledge.read(topic).strip()
assert callable(workflow) and callable(provider)
assert Context.__module__ == "kat._declarations.workflow"
print(f"Bundled Python uses one kat-sdk {sdk.version} for API, Runtime and Datasource.")
