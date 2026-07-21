//! native hook plugin domain decoding.

mod event;
mod packet;
mod records {
    include!(concat!(env!("OUT_DIR"), "/native_hook_records.rs"));
}

pub(crate) use event::{NativeHookEvent, NativeHookEventContext};
pub(crate) use packet::{HOOK_DAEMON_PLUGIN_DECODER, NATIVE_HOOK_PLUGIN_DECODER};
pub(crate) use records::{NativeHookRecord, native_hook_record_from_event};
