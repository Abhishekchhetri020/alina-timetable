#![deny(clippy::unwrap_used)]
//! Command contracts. Registry and history are not implemented yet.
use alina_doc::TimetableDoc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub id: String,
    pub params: Value,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandError {
    pub message: String,
}
/// Execution must be atomic. Undo must be associated with one execution;
/// history will need a per-execution inverse or snapshot, not shared state.
pub trait Command: Send + Sync {
    fn id(&self) -> &'static str;
    fn execute(&self, doc: &mut TimetableDoc, params: Value) -> Result<Value, CommandError>;
    fn undo(&self, doc: &mut TimetableDoc) -> Result<(), CommandError>;
}

#[cfg(test)]
mod tests {
    use super::CommandSpec;
    #[test]
    fn command_spec_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let command = CommandSpec {
            id: "card.place".into(),
            params: serde_json::json!({"card_id": 42, "day": 1, "period": 3}),
        };
        let encoded = serde_json::to_string(&command)?;
        let decoded: CommandSpec = serde_json::from_str(&encoded)?;
        assert_eq!(command, decoded);
        Ok(())
    }
}
