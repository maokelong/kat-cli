mod binding;

pub use binding::{
    ColumnInspection, DatasetBindingKind, DatasetInspection, DatasetInspectionError,
    DatasetMutationError, DatasetTargetInspection, MaterializedSourcePublication, ResolvedDataset,
    ResolvedSource, ResolvedTable, SourceInspection, TableInspection, inspect_dataset,
    inspect_dataset_target, publish_materialized_source, resolve_dataset, write_external_binding,
};
