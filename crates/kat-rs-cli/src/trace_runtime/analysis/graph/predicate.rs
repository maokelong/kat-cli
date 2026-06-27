use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Task 1 parser placeholder; later graph predicate work gives this semantics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PredicateSpec(pub Value);
