SELECT
    current_database() AS database_name,
    current_user AS user_name,
    version() AS server_version,
    current_setting('transaction_read_only') AS transaction_read_only,
    COALESCE(
        (SELECT ssl::text FROM pg_stat_ssl WHERE pid = pg_backend_pid()),
        'false'
    ) AS tls_active,
    COALESCE(
        (SELECT version FROM pg_stat_ssl WHERE pid = pg_backend_pid()),
        ''
    ) AS tls_version;
