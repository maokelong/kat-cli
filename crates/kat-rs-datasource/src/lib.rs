mod config;
mod hitrace;
mod json;
mod mmap;
mod query;

pub use config::{DataSourceConfig, DataSourceType};
pub use query::TraceDatasource;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/kat.hitrace.rs"));
}
