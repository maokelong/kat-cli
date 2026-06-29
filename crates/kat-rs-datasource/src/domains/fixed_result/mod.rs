//! fixed result profiler plugin domain decoding.

mod records {
    include!(concat!(env!("OUT_DIR"), "/fixed_result_records.rs"));
}

pub(crate) use records::{FIXED_RESULT_PLUGIN_DECODERS, FixedResultRecord};
