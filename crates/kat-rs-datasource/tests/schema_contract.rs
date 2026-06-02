use kat_rs_datasource::sched_slice_contract;
use trace_model::schema::sched_slice_schema;

#[test]
fn sched_slice_schema_matches_datasource_contract() {
    let schema = sched_slice_schema();
    let contract = sched_slice_contract();

    assert_eq!(schema.fields().len(), contract.columns.len());
    for (field, expected) in schema.fields().iter().zip(contract.columns.iter()) {
        assert_eq!(field.name(), &expected.name);
        assert_eq!(format!("{:?}", field.data_type()), expected.data_type);
        assert_eq!(field.is_nullable(), expected.nullable);
    }
}
