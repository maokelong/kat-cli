use std::path::Path;

use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn sqlite_dataset_materializes_openharmony_tables_with_instant_rowid() {
    let dir = tempdir().expect("tempdir is created");
    let sqlite_path = dir.path().join("input.db");
    create_sqlite_fixture(&sqlite_path);
    let dataset_path = dir.path().join("dataset");

    kat_rs_datasource::materialize_sqlite_dataset(&sqlite_path, &dataset_path)
        .await
        .expect("sqlite dataset is materialized");

    assert!(dataset_path.join("catalog.json").exists());

    let datasource = kat_rs_datasource::TraceDatasource::from_dataset(&dataset_path)
        .await
        .expect("dataset opens");
    let rows = datasource
        .query_json(
            "select \
               (select count(*) from process) as process_count, \
               (select count(*) from thread) as thread_count, \
               (select count(*) from callstack) as callstack_count, \
               (select count(*) from thread_state) as thread_state_count, \
               (select count(*) from instant) as instant_count, \
               (select rowid from instant where name = 'sched_wakeup') as wakeup_rowid",
        )
        .await
        .expect("dataset query succeeds");

    assert_eq!(
        rows,
        json!([{
            "process_count": 1,
            "thread_count": 1,
            "callstack_count": 1,
            "thread_state_count": 1,
            "instant_count": 1,
            "wakeup_rowid": 1
        }])
    );
}

fn create_sqlite_fixture(path: &Path) {
    let connection = Connection::open(path).expect("sqlite fixture opens");
    connection
        .execute_batch(
            r#"
            CREATE TABLE process (
                id INT, ipid INT, pid INT, name TEXT
            );
            CREATE TABLE thread (
                id INT, itid INT, tid INT, name TEXT, ipid INT, is_main_thread INT
            );
            CREATE TABLE callstack (
                id INT, ts INT, dur INT, callid INT, name TEXT, depth INT, parent_id INT
            );
            CREATE TABLE thread_state (
                id INT, ts INT, dur INT, itid INT, tid INT, state TEXT
            );
            CREATE TABLE instant (
                ts INT, name TEXT, ref INT, wakeup_from INT, ref_type TEXT
            );

            INSERT INTO process VALUES (89, 89, 15040, '.tencent.wechat');
            INSERT INTO thread VALUES (405, 405, 15040, '.tencent.wechat', 89, 1);
            INSERT INTO callstack VALUES (6387, 245720189000, 481901000, 405, 'HandleLaunchAbility', 0, 4294967295);
            INSERT INTO thread_state VALUES (1, 245720189000, 1000000, 405, 15040, 'Sleeping');
            INSERT INTO instant VALUES (245721000000, 'sched_wakeup', 405, 406, 'itid');
            "#,
        )
        .expect("sqlite fixture schema is created");
}
