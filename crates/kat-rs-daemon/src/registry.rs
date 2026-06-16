use std::{
    collections::HashMap,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::{Mutex, RwLock, watch};
use uuid::Uuid;

use crate::{api::DatasourceDto, error::ApiError, identity::DatasourceIdentityKey};

type LoadResult<T> = Result<Arc<DatasourceEntry<T>>, ApiError>;

#[derive(Debug)]
pub struct DatasourceEntry<T> {
    pub id: String,
    pub identity: DatasourceIdentityKey,
    pub datasource: Arc<T>,
    pub created_at: OffsetDateTime,
    pub last_accessed_at: RwLock<OffsetDateTime>,
}

impl<T> DatasourceEntry<T> {
    pub fn new(identity: DatasourceIdentityKey, value: T) -> Self {
        let now = OffsetDateTime::now_utc();

        Self {
            id: Uuid::now_v7().to_string(),
            identity,
            datasource: Arc::new(value),
            created_at: now,
            last_accessed_at: RwLock::new(now),
        }
    }

    pub async fn to_dto(&self) -> DatasourceDto {
        let last_accessed_at = *self.last_accessed_at.read().await;

        DatasourceDto {
            id: self.id.clone(),
            source: self.identity.source,
            inputs: self
                .identity
                .inputs
                .iter()
                .map(crate::identity::InputIdentity::to_dto)
                .collect(),
            state: "READY",
            created_at: format_timestamp(self.created_at),
            last_accessed_at: format_timestamp(last_accessed_at),
        }
    }
}

#[derive(Debug)]
pub struct DatasourceRegistry<T> {
    inner: Arc<Mutex<RegistryInner<T>>>,
}

impl<T> DatasourceRegistry<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RegistryInner::default())),
        }
    }

    pub async fn get_or_insert_with<F, Fut>(
        &self,
        identity: DatasourceIdentityKey,
        loader: F,
    ) -> Result<(Arc<DatasourceEntry<T>>, bool), ApiError>
    where
        T: Send + Sync + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
    {
        let slot = {
            let mut inner = self.inner.lock().await;

            if let Some(id) = inner.by_identity.get(&identity)
                && let Some(entry) = inner.entries.get(id)
            {
                return Ok((Arc::clone(entry), false));
            }

            if let Some(slot) = inner.inflight.get(&identity) {
                LoadSlotRef::Wait(Arc::clone(slot))
            } else {
                let slot = Arc::new(LoadSlot::new());
                inner.inflight.insert(identity.clone(), Arc::clone(&slot));
                LoadSlotRef::Load(slot)
            }
        };

        match slot {
            LoadSlotRef::Wait(slot) => slot.wait().await.map(|entry| (entry, false)),
            LoadSlotRef::Load(slot) => {
                let inner = Arc::clone(&self.inner);
                let waiter_slot = Arc::clone(&slot);
                let _load_task = tokio::spawn(async move {
                    run_load(inner, identity, slot, loader).await;
                });

                waiter_slot.wait().await.map(|entry| (entry, true))
            }
        }
    }

    pub async fn get(&self, id: &str) -> Option<Arc<DatasourceEntry<T>>> {
        let inner = self.inner.lock().await;
        inner.entries.get(id).cloned()
    }

    pub async fn remove(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().await;
        let Some(entry) = inner.entries.remove(id) else {
            return false;
        };
        inner.by_identity.remove(&entry.identity);
        true
    }

    pub async fn list(&self, limit: usize, offset: usize) -> (Vec<Arc<DatasourceEntry<T>>>, usize) {
        let inner = self.inner.lock().await;
        let mut entries = inner.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });

        let total_items = entries.len();
        let entries = entries.into_iter().skip(offset).take(limit).collect();

        (entries, total_items)
    }
}

impl<T> Default for DatasourceRegistry<T> {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_load<T, F, Fut>(
    inner: Arc<Mutex<RegistryInner<T>>>,
    identity: DatasourceIdentityKey,
    slot: Arc<LoadSlot<T>>,
    loader: F,
) where
    T: Send + Sync + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
{
    let result = match catch_unwind(AssertUnwindSafe(loader)) {
        Ok(load) => match tokio::spawn(load).await {
            Ok(result) => {
                result.map(|value| Arc::new(DatasourceEntry::new(identity.clone(), value)))
            }
            Err(error) if error.is_panic() => Err(ApiError::internal("datasource loader panicked")),
            Err(_) => Err(ApiError::internal("datasource loader cancelled")),
        },
        Err(_) => Err(ApiError::internal("datasource loader panicked")),
    };

    let mut inner = inner.lock().await;
    if let Ok(entry) = &result {
        inner.by_identity.insert(identity.clone(), entry.id.clone());
        inner.entries.insert(entry.id.clone(), Arc::clone(entry));
    }
    slot.complete(result.clone());
    inner.inflight.remove(&identity);
}

#[derive(Debug)]
struct RegistryInner<T> {
    entries: HashMap<String, Arc<DatasourceEntry<T>>>,
    by_identity: HashMap<DatasourceIdentityKey, String>,
    inflight: HashMap<DatasourceIdentityKey, Arc<LoadSlot<T>>>,
}

impl<T> Default for RegistryInner<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            by_identity: HashMap::new(),
            inflight: HashMap::new(),
        }
    }
}

#[derive(Debug)]
enum LoadSlotRef<T> {
    Load(Arc<LoadSlot<T>>),
    Wait(Arc<LoadSlot<T>>),
}

#[derive(Debug)]
struct LoadSlot<T> {
    result: watch::Sender<Option<LoadResult<T>>>,
}

impl<T> LoadSlot<T> {
    fn new() -> Self {
        let (result, _receiver) = watch::channel(None);

        Self { result }
    }

    async fn wait(&self) -> LoadResult<T> {
        let mut result = self.result.subscribe();

        loop {
            if let Some(load_result) = result.borrow_and_update().clone() {
                return load_result;
            }

            result
                .changed()
                .await
                .expect("load slot sender is retained while waiters exist");
        }
    }

    fn complete(&self, result: LoadResult<T>) {
        self.result.send_replace(Some(result));
    }
}

fn format_timestamp(timestamp: OffsetDateTime) -> String {
    timestamp
        .format(&Rfc3339)
        .expect("UTC timestamp must format as RFC3339")
}
