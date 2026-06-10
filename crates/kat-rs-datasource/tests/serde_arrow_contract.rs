use arrow_schema::DataType;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_arrow::schema::{SchemaLike, TracingOptions};

#[test]
fn serde_arrow_builds_record_batch_from_plain_struct() {
    #[derive(Deserialize, Serialize)]
    struct PlainRow {
        name: String,
        pid: i32,
        timestamp: u64,
    }

    let rows = vec![PlainRow {
        name: "render".to_string(),
        pid: 42,
        timestamp: 100,
    }];
    let fields = Vec::<arrow_schema::FieldRef>::from_type::<PlainRow>(TracingOptions::default())
        .expect("schema is traced");
    let batch = serde_arrow::to_record_batch(&fields, &rows).expect("record batch is built");

    assert_eq!(batch.num_rows(), 1);
    assert_eq!(
        batch
            .schema()
            .field_with_name("name")
            .expect("name field")
            .data_type(),
        &DataType::LargeUtf8
    );
    assert_eq!(
        batch
            .schema()
            .field_with_name("pid")
            .expect("pid field")
            .data_type(),
        &DataType::Int32
    );
}

#[test]
fn serde_arrow_builds_record_batch_from_prost_struct() {
    #[derive(Clone, Deserialize, Message, PartialEq, Serialize)]
    struct ProstRow {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(bytes = "vec", tag = "2")]
        #[serde(with = "serde_bytes")]
        payload: Vec<u8>,
        #[prost(uint64, tag = "3")]
        timestamp: u64,
    }

    let rows = vec![ProstRow {
        name: "row".to_string(),
        payload: vec![1, 2, 3],
        timestamp: 10,
    }];
    let fields = Vec::<arrow_schema::FieldRef>::from_type::<ProstRow>(TracingOptions::default())
        .expect("schema is traced");
    let batch = serde_arrow::to_record_batch(&fields, &rows).expect("record batch is built");

    assert_eq!(batch.num_rows(), 1);
    assert_eq!(
        batch
            .schema()
            .field_with_name("payload")
            .expect("payload field")
            .data_type(),
        &DataType::LargeBinary
    );
}
