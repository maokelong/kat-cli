from __future__ import annotations

from configparser import ConfigParser, Error
from dataclasses import dataclass
import os
from pathlib import Path

import pytest


@dataclass(frozen=True)
class PostgreSQLTestConfig:
    readonly_profile: str
    writer_profile: str
    telemetry_database: str
    control_database: str
    secret: str


def _required_environment(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"{name} must name the external PostgreSQL test fixture")
    return value


def _required_file(name: str) -> Path:
    path = Path(_required_environment(name))
    if not path.is_file():
        raise RuntimeError(f"{name} must identify a readable test fixture file")
    return path


def _validate_service_file(
    path: Path,
    *,
    profiles: tuple[str, str],
) -> None:
    services = ConfigParser(interpolation=None)
    services.read(path, encoding="utf-8")
    for profile in profiles:
        if not services.has_section(profile):
            raise RuntimeError(
                "PGSERVICEFILE must define both PostgreSQL test profiles"
            )
        try:
            timeout = int(services.get(profile, "connect_timeout"))
        except (ValueError, Error):
            raise RuntimeError(
                "each PostgreSQL test service must set an integer connect_timeout"
            ) from None
        if timeout <= 0:
            raise RuntimeError(
                "each PostgreSQL test service must set a positive connect_timeout"
            )


@pytest.fixture(scope="session")
def postgresql_config() -> PostgreSQLTestConfig:
    readonly_profile = _required_environment(
        "KAT_TEST_POSTGRES_READONLY_PROFILE"
    )
    writer_profile = _required_environment("KAT_TEST_POSTGRES_WRITER_PROFILE")
    telemetry_database = _required_environment(
        "KAT_TEST_POSTGRES_TELEMETRY_DATABASE"
    )
    control_database = _required_environment(
        "KAT_TEST_POSTGRES_CONTROL_DATABASE"
    )
    if telemetry_database == control_database:
        raise RuntimeError(
            "the PostgreSQL fusion fixture requires two distinct Databases"
        )
    service_file = _required_file("PGSERVICEFILE")
    password_file = _required_file("PGPASSFILE")
    secret = _required_environment("KAT_TEST_POSTGRES_SECRET_SENTINEL")
    _validate_service_file(
        service_file,
        profiles=(readonly_profile, writer_profile),
    )
    if secret not in password_file.read_text(encoding="utf-8"):
        raise RuntimeError(
            "KAT_TEST_POSTGRES_SECRET_SENTINEL must match the test password file"
        )
    return PostgreSQLTestConfig(
        readonly_profile=readonly_profile,
        writer_profile=writer_profile,
        telemetry_database=telemetry_database,
        control_database=control_database,
        secret=secret,
    )
