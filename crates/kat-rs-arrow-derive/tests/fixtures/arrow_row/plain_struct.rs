use kat_rs_arrow_derive::ArrowRow;

#[derive(ArrowRow)]
struct PlainRow {
    name: String,
    payload: Vec<u8>,
    ok: bool,
    score: f64,
    count: u32,
    delta: i32,
}

fn main() {
    let _batch = PlainRow::record_batch_from(vec![PlainRow {
        name: "row".to_string(),
        payload: vec![1, 2, 3],
        ok: true,
        score: 1.5,
        count: 7,
        delta: -1,
    }])
    .expect("record batch");
}
