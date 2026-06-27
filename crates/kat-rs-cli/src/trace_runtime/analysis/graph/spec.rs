use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use serde_json::{Map, Value};

use super::{binding::BindingExpr, predicate::PredicateSpec};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenericGraphWalkStepSpec {
    pub id: String,
    pub root: GenericGraphRootSpec,
    #[serde(default)]
    pub limits: GenericGraphWalkLimitsSpec,
    #[serde(default)]
    pub providers: Vec<GraphProviderSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenericGraphRootSpec {
    pub from_state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenericGraphWalkLimitsSpec {
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_max_edges_per_node")]
    pub max_edges_per_node: usize,
}

impl Default for GenericGraphWalkLimitsSpec {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            max_nodes: default_max_nodes(),
            max_edges_per_node: default_max_edges_per_node(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphProviderSpec {
    pub id: String,
    pub input: GraphProviderInputSpec,
    #[serde(rename = "match")]
    pub match_: PredicateSpec,
    pub expand: GraphExpandSpec,
    #[serde(default)]
    pub select: GraphSelectSpec,
    pub output: GraphOutputSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphProviderInputSpec {
    pub table: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphExpandSpec {
    pub node: GraphNodeExpandSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeExpandSpec {
    #[serde(default)]
    pub same_as: Option<BindingExpr>,
    #[serde(default)]
    pub fields: BTreeMap<String, GraphValueSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphSelectSpec {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub order_by: Vec<GraphOrderBySpec>,
    #[serde(default)]
    pub dedupe_by: Vec<BindingExpr>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphOrderBySpec {
    pub expr: BindingExpr,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphOutputSpec {
    pub relation: String,
    #[serde(default)]
    pub evidence: GraphEvidenceSpec,
    #[serde(default)]
    pub annotations: BTreeMap<String, GraphValueSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphEvidenceSpec {
    #[serde(default)]
    pub tables: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GraphValueSpec {
    Scaled { value: BindingExpr, scale: f64 },
    Value(BindingExpr),
}

impl Serialize for GraphValueSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Scaled { value, scale } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("value", value)?;
                map.serialize_entry("scale", scale)?;
                map.end()
            }
            Self::Value(BindingExpr::Literal(Value::Object(object)))
                if is_exact_scaled_value_shape(object) =>
            {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("literal", object)?;
                map.end()
            }
            Self::Value(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for GraphValueSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Value::deserialize(deserializer)?;
        if raw.as_object().is_some_and(is_exact_scaled_value_shape) {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct ScaledGraphValueSpec {
                value: BindingExpr,
                scale: f64,
            }

            let scaled =
                ScaledGraphValueSpec::deserialize(raw).map_err(serde::de::Error::custom)?;
            return Ok(Self::Scaled {
                value: scaled.value,
                scale: scaled.scale,
            });
        }

        let value = serde_json::from_value(raw).map_err(serde::de::Error::custom)?;
        Ok(Self::Value(value))
    }
}

fn is_exact_scaled_value_shape(object: &Map<String, Value>) -> bool {
    object.len() == 2 && object.contains_key("value") && object.contains_key("scale")
}

const fn default_max_depth() -> usize {
    3
}

const fn default_max_nodes() -> usize {
    50
}

const fn default_max_edges_per_node() -> usize {
    3
}
