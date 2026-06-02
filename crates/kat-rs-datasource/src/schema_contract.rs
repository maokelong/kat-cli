use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnContract {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableContract {
    pub table: String,
    pub columns: Vec<ColumnContract>,
}

pub fn sched_slice_contract() -> TableContract {
    TableContract {
        table: "sched_slice".to_string(),
        columns: vec![
            ColumnContract {
                name: "cpu".to_string(),
                data_type: "UInt32".to_string(),
                nullable: false,
                unit: None,
            },
            ColumnContract {
                name: "utid".to_string(),
                data_type: "UInt32".to_string(),
                nullable: false,
                unit: None,
            },
            ColumnContract {
                name: "ts".to_string(),
                data_type: "Int64".to_string(),
                nullable: false,
                unit: Some("ns".to_string()),
            },
            ColumnContract {
                name: "dur".to_string(),
                data_type: "Int64".to_string(),
                nullable: true,
                unit: Some("ns".to_string()),
            },
            ColumnContract {
                name: "priority".to_string(),
                data_type: "Int32".to_string(),
                nullable: true,
                unit: None,
            },
            ColumnContract {
                name: "end_state".to_string(),
                data_type: "Utf8".to_string(),
                nullable: true,
                unit: None,
            },
        ],
    }
}
