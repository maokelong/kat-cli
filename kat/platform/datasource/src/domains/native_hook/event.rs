#[derive(Clone, Debug)]
pub(crate) struct NativeHookEventContext {
    pub(crate) tv_sec: u64,
    pub(crate) tv_nsec: u64,
}

impl NativeHookEventContext {
    pub(crate) fn new(tv_sec: u64, tv_nsec: u64) -> Self {
        Self { tv_sec, tv_nsec }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NativeHookEvent<T> {
    pub(crate) context: NativeHookEventContext,
    pub(crate) event: T,
}

impl<T> NativeHookEvent<T> {
    pub(crate) fn new(context: NativeHookEventContext, event: T) -> Self {
        Self { context, event }
    }
}
