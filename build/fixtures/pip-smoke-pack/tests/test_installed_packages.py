from pathlib import Path
import sys
import sysconfig


def test_installed_packages_run_in_the_bundled_environment(kat_run):
    rows = kat_run(workflow="installed-packages")["main"].to_pylist()
    if len(rows) != 1:
        raise AssertionError(f"Expected one installed-library result, got {rows!r}")
    row = rows[0]
    expected = {
        "value": "KAT library install",
        "six_version": "1.17.0",
        "packaging_version": "25.0",
        "python_executable": str(Path(sys.executable).resolve()),
    }
    for key, value in expected.items():
        if row[key] != value:
            raise AssertionError(f"{key}: expected {value!r}, got {row[key]!r}")
    site_packages = Path(sysconfig.get_path("purelib")).resolve()
    for key in ("six_file", "packaging_file"):
        if not Path(row[key]).resolve().is_relative_to(site_packages):
            raise AssertionError(f"{key} is outside bundled site-packages: {row[key]}")
