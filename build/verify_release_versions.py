#!/usr/bin/env python3
"""Verify that every component uses the KAT release version."""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path


VERSION_SOURCES = (
    ("release/kat/dist.toml", ("package", "version")),
    ("Cargo.toml", ("workspace", "package", "version")),
    ("kat/platform/workflow/pyproject.toml", ("project", "version")),
)
TAG_NAMESPACE = "kat"


def read_version(repository: Path, relative_path: str, keys: tuple[str, ...]) -> str:
    with (repository / relative_path).open("rb") as source:
        value: object = tomllib.load(source)
    for key in keys:
        if not isinstance(value, dict) or key not in value:
            raise ValueError(f"{relative_path} is missing {'.'.join(keys)}")
        value = value[key]
    if not isinstance(value, str) or not value:
        raise ValueError(f"{relative_path} {'.'.join(keys)} must be a non-empty string")
    return value


def verify_release_versions(repository: Path) -> str:
    versions = {
        path: read_version(repository, path, keys)
        for path, keys in VERSION_SOURCES
    }
    release_version = versions[VERSION_SOURCES[0][0]]
    if any(version != release_version for version in versions.values()):
        details = "\n".join(f"  {path}: {version}" for path, version in versions.items())
        raise ValueError(f"release versions do not match:\n{details}")
    return release_version


def verify_release_tag(tag: str, version: str) -> None:
    expected = f"{TAG_NAMESPACE}/{version}"
    if tag != expected:
        raise ValueError(f"release tag must be exactly {expected}, got {tag}")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--repository",
        type=Path,
        default=Path(__file__).resolve().parents[1],
    )
    parser.add_argument(
        "--tag",
        help="also require the exact formal release tag kat/<version>",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        arguments = parse_args(argv)
        version = verify_release_versions(arguments.repository.resolve())
        if arguments.tag is not None:
            verify_release_tag(arguments.tag, version)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"release version verification failed: {error}", file=sys.stderr)
        return 1
    if arguments.tag is None:
        print(f"release versions match: {version}")
    else:
        print(f"release versions and tag match: {arguments.tag}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
