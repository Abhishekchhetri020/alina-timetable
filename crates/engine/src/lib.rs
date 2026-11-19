#![deny(clippy::unwrap_used)]
//! Atomic command dispatch and snapshot-based undo/redo.
use alina_doc::{Placement, TimetableDoc};
use alina_foundation::{CardId, TimeSlot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub id: String,
    pub params: Value,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandError {
    pub message: String,
}
impl CommandError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
/// Commands mutate a private candidate. Session commits only valid candidates.
/// History stores the pre-execution document, so undo is per execution rather
/// than mutable shared state inside a command object.
pub trait Command: Send + Sync {
    fn id(&self) -> &'static str;
    fn execute(&self, doc: &mut TimetableDoc, params: Value) -> Result<Value, CommandError>;
}
#[derive(Default)]
pub struct CommandRegistry {
    commands: BTreeMap<String, Box<dyn Command>>,
}
impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, command: Box<dyn Command>) -> Result<(), CommandError> {
        let id = command.id().to_owned();
        if self.commands.contains_key(&id) {
            return Err(CommandError::new("duplicate command"));
        }
        self.commands.insert(id, command);
        Ok(())
    }
    pub fn builtins() -> Self {
        let mut registry = Self::new();
        for kind in [
            Builtin::Place,
            Builtin::Unplace,
            Builtin::Swap,
            Builtin::Lock,
        ] {
            registry
                .commands
                .insert(kind.id().to_owned(), Box::new(kind));
        }
        registry
    }
}
struct HistoryEntry {
    doc: TimetableDoc,
    command: CommandSpec,
}
/// Schedule state is exposed read-only; only registered commands can change it.
pub struct Session {
    doc: TimetableDoc,
    registry: CommandRegistry,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    history_limit: usize,
}
impl Session {
    pub fn new(doc: TimetableDoc) -> Self {
        Self::with_registry(doc, CommandRegistry::builtins(), 200)
    }
    pub fn with_registry(
        doc: TimetableDoc,
        registry: CommandRegistry,
        history_limit: usize,
    ) -> Self {
        Self {
            doc,
            registry,
            undo: Vec::new(),
            redo: Vec::new(),
            history_limit,
        }
    }
    pub fn document(&self) -> &TimetableDoc {
        &self.doc
    }
    pub fn dispatch(&mut self, spec: CommandSpec) -> Result<Value, CommandError> {
        let command = self
            .registry
            .commands
            .get(&spec.id)
            .ok_or_else(|| CommandError::new("unknown command"))?;
        let mut candidate = self.doc.clone();
        let result = command.execute(&mut candidate, spec.params.clone())?;
        if !alina_oracle::verify(&candidate).violations.is_empty() {
            return Err(CommandError::new(
                "command would produce an invalid timetable",
            ));
        }
        let previous = std::mem::replace(&mut self.doc, candidate);
        self.undo.push(HistoryEntry {
            doc: previous,
            command: spec,
        });
        if self.undo.len() > self.history_limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(result)
    }
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        let doc = std::mem::replace(&mut self.doc, entry.doc);
        self.redo.push(HistoryEntry {
            doc,
            command: entry.command,
        });
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        let doc = std::mem::replace(&mut self.doc, entry.doc);
        self.undo.push(HistoryEntry {
            doc,
            command: entry.command,
        });
        true
    }
}
#[derive(Clone, Copy)]
enum Builtin {
    Place,
    Unplace,
    Swap,
    Lock,
}
#[derive(Deserialize)]
struct CardParam {
    card_id: CardId,
}
#[derive(Deserialize)]
struct SwapParams {
    card_a: CardId,
    card_b: CardId,
}
#[derive(Deserialize)]
struct LockParams {
    slot: TimeSlot,
    locked: bool,
}
fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, CommandError> {
    serde_json::from_value(value).map_err(|e| CommandError::new(e.to_string()))
}
fn ensure_unlocked(doc: &TimetableDoc, placement: &Placement) -> Result<(), CommandError> {
    let lesson = doc
        .lessons
        .get(&placement.lesson)
        .ok_or_else(|| CommandError::new("unknown lesson"))?;
    let end = u16::from(placement.start.period) + u16::from(lesson.duration);
    if end > u16::from(doc.periods_per_day)
        || placement.start.day >= doc.days
        || lesson.duration == 0
    {
        return Err(CommandError::new("invalid placement"));
    }
    if (u16::from(placement.start.period)..end).any(|period| {
        doc.locked_slots.contains(&TimeSlot {
            day: placement.start.day,
            period: period as u8,
        })
    }) {
        return Err(CommandError::new("placement touches a locked slot"));
    }
    Ok(())
}
impl Command for Builtin {
    fn id(&self) -> &'static str {
        match self {
            Self::Place => "card.place",
            Self::Unplace => "card.unplace",
            Self::Swap => "card.swap",
            Self::Lock => "slot.lock",
        }
    }
    fn execute(&self, doc: &mut TimetableDoc, params: Value) -> Result<Value, CommandError> {
        match self {
            Self::Place => {
                let placement: Placement = parse(params)?;
                if let Some(old) = doc.placements.get(&placement.card) {
                    ensure_unlocked(doc, old)?;
                }
                ensure_unlocked(doc, &placement)?;
                Arc::make_mut(&mut doc.placements).insert(placement.card, placement);
            }
            Self::Unplace => {
                let p: CardParam = parse(params)?;
                let old = doc
                    .placements
                    .get(&p.card_id)
                    .ok_or_else(|| CommandError::new("card is not placed"))?;
                ensure_unlocked(doc, old)?;
                Arc::make_mut(&mut doc.placements).remove(&p.card_id);
            }
            Self::Swap => {
                let p: SwapParams = parse(params)?;
                if p.card_a == p.card_b {
                    return Err(CommandError::new("cannot swap a card with itself"));
                }
                let mut a = doc
                    .placements
                    .get(&p.card_a)
                    .cloned()
                    .ok_or_else(|| CommandError::new("first card is not placed"))?;
                let mut b = doc
                    .placements
                    .get(&p.card_b)
                    .cloned()
                    .ok_or_else(|| CommandError::new("second card is not placed"))?;
                ensure_unlocked(doc, &a)?;
                ensure_unlocked(doc, &b)?;
                std::mem::swap(&mut a.start, &mut b.start);
                ensure_unlocked(doc, &a)?;
                ensure_unlocked(doc, &b)?;
                let placements = Arc::make_mut(&mut doc.placements);
                placements.insert(a.card, a);
                placements.insert(b.card, b);
            }
            Self::Lock => {
                let p: LockParams = parse(params)?;
                if p.slot.day >= doc.days || p.slot.period >= doc.periods_per_day {
                    return Err(CommandError::new("invalid slot"));
                }
                if p.locked {
                    doc.locked_slots.insert(p.slot);
                } else {
                    doc.locked_slots.remove(&p.slot);
                }
            }
        }
        Ok(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alina_doc::{Lesson, Subject};
    use alina_foundation::{LessonId, SubjectId};
    use serde_json::json;
    fn fixture() -> Session {
        let mut doc = TimetableDoc::default();
        doc.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "Math".into(),
            },
        );
        doc.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 2,
            },
        );
        Session::new(doc)
    }
    fn place(id: u64, period: u8) -> CommandSpec {
        CommandSpec {
            id: "card.place".into(),
            params: json!({"card": id, "lesson": 1, "start": {"day":0,"period":period}, "room":null}),
        }
    }
    #[test]
    fn atomic_history_and_redo_invalidation() -> Result<(), CommandError> {
        let mut session = fixture();
        session.dispatch(place(1, 0))?;
        assert!(session.dispatch(place(2, 5)).is_err());
        assert_eq!(session.document().placements.len(), 1);
        assert!(session.undo());
        assert!(session.document().placements.is_empty());
        assert!(session.redo());
        assert_eq!(session.document().placements.len(), 1);
        assert!(session.undo());
        session.dispatch(place(2, 2))?;
        assert!(!session.redo());
        Ok(())
    }
    #[test]
    fn locked_second_period_blocks_moves_and_unplace() -> Result<(), CommandError> {
        let mut session = fixture();
        session.dispatch(place(1, 0))?;
        session.dispatch(CommandSpec {
            id: "slot.lock".into(),
            params: json!({"slot":{"day":0,"period":1},"locked":true}),
        })?;
        assert!(session
            .dispatch(CommandSpec {
                id: "card.unplace".into(),
                params: json!({"card_id":1})
            })
            .is_err());
        assert!(session.dispatch(place(2, 1)).is_err());
        assert_eq!(session.document().placements.len(), 1);
        assert!(session.undo());
        session.dispatch(CommandSpec {
            id: "card.unplace".into(),
            params: json!({"card_id":1}),
        })?;
        Ok(())
    }
}
