use trace_model::{
    is_trace_table, trace_table_contract, trace_table_contracts, trace_table_names,
    trace_table_schema,
};

#[test]
fn exposes_minimal_trace_bounds_contract() {
    let contracts = trace_table_contracts();
    assert_eq!(contracts.len(), 1);
    assert_eq!(trace_table_names(), vec!["trace_bounds"]);
    assert!(is_trace_table("trace_bounds"));
    assert!(!is_trace_table("process"));
}

#[test]
fn trace_bounds_schema_is_generated_from_contract() {
    let contract = trace_table_contract("trace_bounds").expect("trace_bounds contract exists");
    let schema = trace_table_schema("trace_bounds").expect("trace_bounds schema exists");

    assert_eq!(schema.fields().len(), contract.columns.len());

    for (field, column) in schema.fields().iter().zip(&contract.columns) {
        assert_eq!(field.name(), &column.name);
        assert_eq!(
            field.data_type().to_string(),
            column.data_type.as_contract_str()
        );
        assert_eq!(field.is_nullable(), column.nullable);
    }
}

#[test]
fn rejects_unregistered_tables() {
    for table_name in ["process", "thread", "sched_slice", "raw_event"] {
        assert!(trace_table_contract(table_name).is_none());
        assert!(trace_table_schema(table_name).is_none());
    }
}
