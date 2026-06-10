use kat_rs_arrow_derive::ArrowRow;

#[derive(ArrowRow)]
struct UnsupportedRow {
    values: Vec<String>,
}

fn main() {}
