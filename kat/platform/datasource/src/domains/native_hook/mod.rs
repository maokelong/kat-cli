//! native hook source-fact domain records.

mod event;
mod records {
    include!(concat!(env!("OUT_DIR"), "/native_hook_records.rs"));
}

pub(crate) use event::{NativeHookEvent, NativeHookEventContext};
pub(crate) use records::{NativeHookRecord, native_hook_record_from_event};
