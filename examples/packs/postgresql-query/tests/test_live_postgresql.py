import pyarrow as pa


def test_live_postgresql_smoke(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            """
            SELECT
                current_database() AS database_name,
                current_user AS user_name,
                version() AS server_version,
                current_setting('transaction_read_only') AS transaction_read_only,
                COALESCE(
                    (
                        SELECT ssl::text
                        FROM pg_stat_ssl
                        WHERE pid = pg_backend_pid()
                    ),
                    'false'
                ) AS tls_enabled
            """,
        ],
    )

    table = outputs["postgresql_result"]
    assert table.num_rows == 1
    assert table.column_names == [
        "database_name",
        "user_name",
        "server_version",
        "transaction_read_only",
        "tls_enabled",
    ]
    row = table.to_pylist()[0]
    assert row["database_name"]
    assert row["user_name"]
    assert row["server_version"].startswith("PostgreSQL ")
    assert row["transaction_read_only"] in {"on", "off"}
    assert row["tls_enabled"] in {"true", "false"}


def test_live_postgresql_preserves_null(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            "SELECT 'present'::text AS present_value, NULL::text AS missing_value",
        ],
    )

    table = outputs["postgresql_result"]
    assert table.schema.types == [pa.string(), pa.string()]
    assert table.to_pylist() == [
        {"present_value": "present", "missing_value": None}
    ]


def test_live_postgresql_preserves_zero_row_schema(kat_run):
    outputs = kat_run(
        workflow="query-postgresql",
        arguments=[
            "--sql",
            "SELECT NULL::text AS empty_value WHERE FALSE",
        ],
    )

    table = outputs["postgresql_result"]
    assert table.num_rows == 0
    assert table.column_names == ["empty_value"]
    assert table.schema.field("empty_value").type == pa.string()
