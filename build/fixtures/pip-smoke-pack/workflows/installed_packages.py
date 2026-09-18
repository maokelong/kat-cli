from pathlib import Path
import sys

import kat
from kat import dataprovider as dp
import packaging
import pyarrow as pa
import six


@kat.workflow(
    name="installed-packages",
    description="Use an installed library and report the runtime that imported it.",
)
def installed_packages(ctx: kat.Context):
    return dp.Table.from_rows(
        [{
            "value": six.ensure_text(b"KAT library install"),
            "six_version": six.__version__,
            "packaging_version": packaging.__version__,
            "six_file": str(Path(six.__file__).resolve()),
            "packaging_file": str(Path(packaging.__file__).resolve()),
            "python_executable": str(Path(sys.executable).resolve()),
        }],
        schema=pa.schema([
            ("value", pa.string()),
            ("six_version", pa.string()),
            ("packaging_version", pa.string()),
            ("six_file", pa.string()),
            ("packaging_file", pa.string()),
            ("python_executable", pa.string()),
        ]),
    )
