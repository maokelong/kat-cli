mod hitrace;
mod json;
mod mmap;
mod query;

pub use query::TraceDatasource;

#[allow(dead_code)]
pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
}
