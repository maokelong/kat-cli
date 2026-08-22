SELECT
    'pack-sql-file'::text AS query_source,
    current_database() AS database_name,
    current_user AS user_name;
