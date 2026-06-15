// serde_arrow 契约测试锁定 prost 结构到 Arrow 表的字段展开方式。
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

#[test]
fn serde_arrow_writes_event_row_with_separately_combined_fields() {
    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct EventMeta {
        event_timestamp: u64,
        event_cpu: u32,
        event_tgid: i32,
        event_comm: String,
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct SchedSwitchFormat {
        prev_comm: String,
        prev_pid: i32,
        next_comm: String,
        next_pid: i32,
    }

    #[derive(Clone, Debug, Default, Serialize, Deserialize)]
    struct EventRow<M> {
        #[serde(flatten)]
        meta: EventMeta,
        #[serde(flatten)]
        message: M,
    }

    let mut fields =
        Vec::<arrow_schema::FieldRef>::from_type::<EventMeta>(TracingOptions::default())
            .expect("event meta schema is traced");
    fields.extend(
        Vec::<arrow_schema::FieldRef>::from_type::<SchedSwitchFormat>(TracingOptions::default())
            .expect("message schema is traced"),
    );
    let mut builder =
        serde_arrow::ArrayBuilder::from_arrow(&fields).expect("array builder is created");
    builder
        .push(EventRow {
            meta: EventMeta {
                event_timestamp: 10,
                event_cpu: 3,
                event_tgid: 500,
                event_comm: "source".to_string(),
            },
            message: SchedSwitchFormat {
                prev_comm: "render".to_string(),
                prev_pid: 42,
                next_comm: "main".to_string(),
                next_pid: 7,
            },
        })
        .expect("flattened event row is appended");

    let batch = builder
        .into_record_batch()
        .expect("record batch is created");
    let schema = batch.schema();

    assert_eq!(batch.num_rows(), 1);
    for field in [
        "event_timestamp",
        "event_cpu",
        "event_tgid",
        "event_comm",
        "prev_comm",
        "prev_pid",
        "next_comm",
        "next_pid",
    ] {
        assert!(
            schema.field_with_name(field).is_ok(),
            "{field} should be a top-level column"
        );
    }
    assert!(schema.field_with_name("meta").is_err());
    assert!(schema.field_with_name("message").is_err());
}
