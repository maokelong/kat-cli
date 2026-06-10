use kat_rs_arrow_derive::ArrowRow;

#[derive(Clone, PartialEq, ::prost::Message, ArrowRow)]
struct ProstRow {
    #[prost(string, tag = "1")]
    name: String,
    #[prost(bytes = "vec", tag = "2")]
    payload: Vec<u8>,
    #[prost(uint64, tag = "3")]
    timestamp: u64,
}

fn main() {
    let _batch = ProstRow::record_batch_from(vec![ProstRow {
        name: "row".to_string(),
        payload: vec![1, 2, 3],
        timestamp: 10,
    }])
    .expect("record batch");
}
