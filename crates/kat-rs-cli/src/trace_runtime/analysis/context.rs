use anyhow::{Result, bail};
use serde_json::{Value, json};

pub struct AnalysisState {
    value: Value,
}

impl Default for AnalysisState {
    fn default() -> Self {
        Self {
            value: json!({
                "root": {},
                "frontier": { "nodes": [] },
                "visitedEdges": [],
                "decisions": [],
                "coverage": { "explainedIntervals": [] },
                "derived": {},
                "evidenceRefs": []
            }),
        }
    }
}

impl AnalysisState {
    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn set_path(&mut self, path: &str, value: Value) -> Result<()> {
        if path.is_empty() {
            bail!("analysis state path cannot be empty");
        }

        let mut current = &mut self.value;
        let mut parts = path.split('.').peekable();
        while let Some(part) = parts.next() {
            if part.is_empty() {
                bail!("analysis state path contains an empty segment: {path:?}");
            }

            if parts.peek().is_none() {
                let Some(object) = current.as_object_mut() else {
                    bail!("cannot set analysis state path through non-object at {part:?}");
                };
                object.insert(part.to_owned(), value);
                return Ok(());
            }

            let Some(object) = current.as_object_mut() else {
                bail!("cannot traverse analysis state path through non-object at {part:?}");
            };
            current = object.entry(part).or_insert_with(|| json!({}));
            if !current.is_object() {
                bail!("cannot traverse analysis state path through non-object at {part:?}");
            }
        }

        Ok(())
    }
}
