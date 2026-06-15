use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use kat_rs_daemon::{
    api::{DatasourceSource, InputRole},
    error::ApiError,
    identity::{DatasourceIdentityKey, InputIdentity},
    registry::DatasourceRegistry,
};
use time::macros::datetime;
use tokio::{
    sync::{Barrier, oneshot},
    time::timeout,
};

fn test_identity(size_bytes: u64, modified_at: &str) -> DatasourceIdentityKey {
    DatasourceIdentityKey::new(
        DatasourceSource::Hitrace,
        vec![InputIdentity::new(
            InputRole::File,
            "fixtures/hitrace.jsonl",
            size_bytes,
            modified_at,
        )],
    )
}

#[tokio::test]
async fn same_identity_reuses_existing_entry() {
    let registry = DatasourceRegistry::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let identity = test_identity(1024, "2026-06-16T12:00:00Z");

    let calls_for_first = Arc::clone(&calls);
    let (first, first_created) = registry
        .get_or_insert_with(identity.clone(), move || async move {
            calls_for_first.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>("first".to_owned())
        })
        .await
        .expect("first load succeeds");

    let calls_for_second = Arc::clone(&calls);
    let (second, second_created) = registry
        .get_or_insert_with(identity, move || async move {
            calls_for_second.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>("second".to_owned())
        })
        .await
        .expect("existing entry is reused");

    assert!(first_created);
    assert!(!second_created);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.datasource.as_str(), "first");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_same_identity_runs_loader_once() {
    let registry = Arc::new(DatasourceRegistry::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(8));
    let identity = test_identity(2048, "2026-06-16T13:00:00Z");

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let registry = Arc::clone(&registry);
        let calls = Arc::clone(&calls);
        let start = Arc::clone(&start);
        let identity = identity.clone();

        tasks.push(tokio::spawn(async move {
            start.wait().await;
            registry
                .get_or_insert_with(identity, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok::<_, ApiError>(42usize)
                })
                .await
                .expect("load succeeds")
        }));
    }

    let mut entries = Vec::new();
    for task in tasks {
        entries.push(task.await.expect("task joins"));
    }

    let first_id = entries[0].0.id.clone();
    assert!(entries.iter().all(|(entry, _)| entry.id == first_id));
    assert!(entries.iter().all(|(entry, _)| *entry.datasource == 42));
    assert_eq!(
        entries.iter().filter(|(_, created)| *created).count(),
        1,
        "only the caller that ran the loader reports created"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn same_path_with_different_metadata_creates_distinct_entries() {
    let registry = DatasourceRegistry::default();
    let calls = Arc::new(AtomicUsize::new(0));

    let calls_for_first = Arc::clone(&calls);
    let (first, first_created) = registry
        .get_or_insert_with(
            test_identity(1024, "2026-06-16T12:00:00Z"),
            move || async move {
                calls_for_first.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ApiError>("first".to_owned())
            },
        )
        .await
        .expect("first load succeeds");

    let calls_for_different_size = Arc::clone(&calls);
    let (different_size, different_size_created) = registry
        .get_or_insert_with(
            test_identity(2048, "2026-06-16T12:00:00Z"),
            move || async move {
                calls_for_different_size.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ApiError>("different-size".to_owned())
            },
        )
        .await
        .expect("different size loads separately");

    let calls_for_different_mtime = Arc::clone(&calls);
    let (different_mtime, different_mtime_created) = registry
        .get_or_insert_with(
            test_identity(1024, "2026-06-16T13:00:00Z"),
            move || async move {
                calls_for_different_mtime.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ApiError>("different-mtime".to_owned())
            },
        )
        .await
        .expect("different mtime loads separately");

    assert!(first_created);
    assert!(different_size_created);
    assert!(different_mtime_created);
    assert_ne!(first.id, different_size.id);
    assert_ne!(first.id, different_mtime.id);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn canceled_creator_does_not_strand_inflight_load() {
    let registry = Arc::new(DatasourceRegistry::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let identity = test_identity(8192, "2026-06-16T15:00:00Z");
    let (started_tx, started_rx) = oneshot::channel();

    let creator = {
        let registry = Arc::clone(&registry);
        let calls = Arc::clone(&calls);
        let identity = identity.clone();

        tokio::spawn(async move {
            registry
                .get_or_insert_with(identity, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    started_tx.send(()).expect("test receiver should wait");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok::<_, ApiError>(99usize)
                })
                .await
        })
    };

    started_rx.await.expect("loader should start");
    creator.abort();
    assert!(
        creator
            .await
            .expect_err("creator task should be aborted")
            .is_cancelled()
    );

    let retry = {
        let calls = Arc::clone(&calls);
        timeout(
            Duration::from_millis(500),
            registry.get_or_insert_with(identity.clone(), move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ApiError>(100usize)
            }),
        )
        .await
        .expect("inflight load should not remain stuck")
        .expect("background load should succeed")
    };

    assert!(!retry.1);
    assert_eq!(*retry.0.datasource, 99);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let calls_for_cached = Arc::clone(&calls);
    let cached = registry
        .get_or_insert_with(identity, move || async move {
            calls_for_cached.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>(101usize)
        })
        .await
        .expect("cached entry should be available");

    assert!(!cached.1);
    assert!(Arc::ptr_eq(&retry.0, &cached.0));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn remove_returns_bool_and_list_paginates_in_stable_order() {
    let registry = DatasourceRegistry::default();

    let (first, _) = registry
        .get_or_insert_with(test_identity(1, "2026-06-16T16:00:00Z"), || async {
            Ok::<_, ApiError>("first".to_owned())
        })
        .await
        .expect("first load succeeds");

    tokio::time::sleep(Duration::from_millis(1)).await;
    let (second, _) = registry
        .get_or_insert_with(test_identity(2, "2026-06-16T16:00:00Z"), || async {
            Ok::<_, ApiError>("second".to_owned())
        })
        .await
        .expect("second load succeeds");

    tokio::time::sleep(Duration::from_millis(1)).await;
    let (third, _) = registry
        .get_or_insert_with(test_identity(3, "2026-06-16T16:00:00Z"), || async {
            Ok::<_, ApiError>("third".to_owned())
        })
        .await
        .expect("third load succeeds");

    let (all, total) = registry.list(10, 0).await;
    assert_eq!(total, 3);
    assert_eq!(
        all.iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        vec![third.id.as_str(), second.id.as_str(), first.id.as_str()]
    );

    let (page, total) = registry.list(1, 1).await;
    assert_eq!(total, 3);
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, second.id);

    assert!(registry.remove(&first.id).await);
    assert!(!registry.remove(&first.id).await);
    assert!(registry.get(&first.id).await.is_none());

    let (_, total_after_remove) = registry.list(10, 0).await;
    assert_eq!(total_after_remove, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_failed_same_identity_notifies_waiters_without_caching() {
    let registry = Arc::new(DatasourceRegistry::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(4));
    let identity = test_identity(4096, "2026-06-16T14:00:00Z");

    let mut tasks = Vec::new();
    for _ in 0..4 {
        let registry = Arc::clone(&registry);
        let calls = Arc::clone(&calls);
        let start = Arc::clone(&start);
        let identity = identity.clone();

        tasks.push(tokio::spawn(async move {
            start.wait().await;
            registry
                .get_or_insert_with(identity, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Err::<usize, _>(ApiError::validation("load failed"))
                })
                .await
        }));
    }

    for task in tasks {
        let error = task
            .await
            .expect("task joins")
            .expect_err("load should fail");
        assert_eq!(error.message, "load failed");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(registry.list(10, 0).await.1, 0);

    let calls_for_retry = Arc::clone(&calls);
    let (entry, created) = registry
        .get_or_insert_with(identity, move || async move {
            calls_for_retry.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ApiError>(7usize)
        })
        .await
        .expect("retry after failure should load");

    assert!(created);
    assert_eq!(*entry.datasource, 7);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn to_dto_includes_identity_and_updated_last_accessed_at() {
    let registry = DatasourceRegistry::default();
    let identity = test_identity(12345, "2026-06-16T17:00:00Z");
    let (entry, _) = registry
        .get_or_insert_with(identity, || async {
            Ok::<_, ApiError>("datasource".to_owned())
        })
        .await
        .expect("load succeeds");

    let updated_at = datetime!(2026-06-16 18:30:45 UTC);
    *entry.last_accessed_at.write().await = updated_at;

    let dto = entry.to_dto().await;
    assert_eq!(dto.id, entry.id);
    assert_eq!(dto.source, DatasourceSource::Hitrace);
    assert_eq!(dto.state, "READY");
    assert!(dto.created_at.contains('T'));
    assert!(dto.created_at.ends_with('Z'));
    assert_eq!(dto.last_accessed_at, "2026-06-16T18:30:45Z");
    assert_eq!(dto.inputs.len(), 1);
    assert_eq!(dto.inputs[0].role, InputRole::File);
    assert_eq!(dto.inputs[0].path, "fixtures/hitrace.jsonl");
    assert_eq!(dto.inputs[0].size_bytes, 12345);
    assert_eq!(dto.inputs[0].modified_at, "2026-06-16T17:00:00Z");
}
