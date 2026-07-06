use std::collections::HashMap;

use serde_json::{Value, json};

use crate::error::ApiError;

#[derive(Clone, Debug)]
pub enum ContextValue {
    Scalar(Value),
    Interval { start: i64, end: i64 },
}

#[derive(Clone, Debug)]
pub struct ContextPublication {
    pub slot: String,
    pub carrier: String,
    pub value: Value,
    pub producing_step: String,
}

#[derive(Clone, Debug, Default)]
pub struct ContextStore {
    values: HashMap<String, ContextValue>,
    publications: Vec<ContextPublication>,
}

impl ContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish_scalar(
        &mut self,
        slot: &str,
        value: Value,
        producing_step: &str,
    ) -> Result<(), ApiError> {
        self.values
            .insert(slot.to_owned(), ContextValue::Scalar(value.clone()));
        self.publications.push(ContextPublication {
            slot: slot.to_owned(),
            carrier: "scalar".to_owned(),
            value,
            producing_step: producing_step.to_owned(),
        });
        Ok(())
    }

    pub fn publish_interval(
        &mut self,
        slot: &str,
        start: i64,
        end: i64,
        producing_step: &str,
    ) -> Result<(), ApiError> {
        self.values
            .insert(slot.to_owned(), ContextValue::Interval { start, end });
        self.publications.push(ContextPublication {
            slot: slot.to_owned(),
            carrier: "interval".to_owned(),
            value: json!({ "start": start, "end": end }),
            producing_step: producing_step.to_owned(),
        });
        Ok(())
    }

    pub fn value(&self, slot: &str) -> Result<&ContextValue, ApiError> {
        self.values
            .get(slot)
            .ok_or_else(|| ApiError::validation(format!("context slot is not published: {slot}")))
    }

    pub fn publications(&self) -> &[ContextPublication] {
        &self.publications
    }
}
