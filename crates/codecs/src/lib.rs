#![deny(clippy::unwrap_used)]
//! Explicit, conservative adapter for a subset of Chronexa's school JSON.
//! Unsupported rule-bearing data is rejected rather than silently discarded.
use alina_doc::{Class, Lesson, Room, Subject, Teacher, TimetableDoc};
use alina_foundation::{Availability, CardId, ClassId, LessonId, RoomId, SubjectId, TeacherId};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct ImportError(pub String);
impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for ImportError {}
#[derive(Debug)]
pub struct ChronexaImport {
    pub document: TimetableDoc,
    pub requests: Vec<ImportedCard>,
    /// Maps source lesson IDs to stable generated card IDs, in occurrence order.
    pub cards: BTreeMap<String, Vec<CardId>>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ImportedCard {
    pub card: CardId,
    pub lesson: LessonId,
    pub rooms: Vec<RoomId>,
}
fn err(message: impl Into<String>) -> ImportError {
    ImportError(message.into())
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, ImportError> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| err(format!("{key} must be an array")))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, ImportError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err(format!("missing or invalid {key}")))
}
fn number(v: &Value, key: &str, default: u64) -> Result<u64, ImportError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(x) => x.as_u64().ok_or_else(|| err(format!("invalid {key}"))),
    }
}
fn nonempty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        _ => true,
    }
}
fn reject_fields(v: &Value, keys: &[&str], context: &str) -> Result<(), ImportError> {
    for key in keys {
        if nonempty(v.get(*key)) {
            return Err(err(format!("unsupported {context}.{key}")));
        }
    }
    Ok(())
}
fn allowed_fields(v: &Value, keys: &[&str], context: &str) -> Result<(), ImportError> {
    let object = v
        .as_object()
        .ok_or_else(|| err(format!("{context} must be an object")))?;
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(err(format!("unsupported {context}.{key}")));
        }
    }
    Ok(())
}
fn ids(items: &[Value], kind: &str) -> Result<BTreeMap<String, u64>, ImportError> {
    let mut names = BTreeSet::new();
    for item in items {
        let id = string(item, "id")?;
        if !names.insert(id.to_owned()) {
            return Err(err(format!("duplicate {kind} ID {id}")));
        }
    }
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(i, id)| (id, i as u64 + 1))
        .collect())
}
fn references(
    v: &Value,
    field: &str,
    ids: &BTreeMap<String, u64>,
) -> Result<Vec<u64>, ImportError> {
    let Some(value) = v.get(field) else {
        return Ok(Vec::new());
    };
    let a = value
        .as_array()
        .ok_or_else(|| err(format!("{field} must be an array")))?;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for item in a {
        let id = item
            .as_str()
            .ok_or_else(|| err(format!("invalid {field} ID")))?;
        let n = *ids
            .get(id)
            .ok_or_else(|| err(format!("unknown {field} ID {id}")))?;
        if !seen.insert(n) {
            return Err(err(format!("duplicate {field} ID {id}")));
        }
        out.push(n);
    }
    Ok(out)
}
fn availability(v: Option<&Value>, days: u8, periods: u8) -> Result<Availability, ImportError> {
    let mut result = Availability::default();
    let Some(v) = v else { return Ok(result) };
    let mut set = |d: usize, p: usize, state: &Value| -> Result<(), ImportError> {
        let status = match state {
            Value::Null => 0,
            Value::Number(n) => n
                .as_u64()
                .filter(|n| *n <= 2)
                .ok_or_else(|| err("invalid timeOff state"))?,
            Value::String(s) if s == "unavailable" || s == "blocked" => 2,
            Value::String(s) if s == "conditional" || s == "preferred" => 1,
            _ => return Err(err("invalid timeOff state")),
        };
        if d >= usize::from(days) || p >= usize::from(periods) {
            return Err(err("timeOff outside week"));
        }
        let bit = 1u64 << (d * usize::from(periods) + p);
        match status {
            1 => result.undesirable |= bit,
            2 => result.blocked |= bit,
            _ => {}
        }
        Ok(())
    };
    if let Some(rows) = v.as_array() {
        for (d, row) in rows.iter().enumerate() {
            let row = row.as_array().ok_or_else(|| err("invalid timeOff row"))?;
            for (p, state) in row.iter().enumerate() {
                set(d, p, state)?;
            }
        }
    } else if let Some(entries) = v.as_object() {
        for (key, state) in entries {
            let (day, period) = key
                .split_once('_')
                .ok_or_else(|| err("invalid timeOff coordinate"))?;
            let d: usize = day.parse().map_err(|_| err("invalid timeOff day"))?;
            let p: usize = period.parse().map_err(|_| err("invalid timeOff period"))?;
            // Chronexa accepts 1-based periods for object keys and 0 as the first period.
            set(
                d,
                if p > 0 && p <= usize::from(periods) {
                    p - 1
                } else {
                    p
                },
                state,
            )?;
        }
    } else if !v.is_null() {
        return Err(err("invalid timeOff"));
    }
    if result.blocked & result.undesirable != 0 {
        return Err(err("overlapping timeOff coordinates"));
    }
    Ok(result)
}
/// Import the supported JSON subset. IDs are assigned by sorted source ID,
/// independent of source array order. Coordinates in Alina are zero-based.
/// This adapter deliberately refuses known constraints it cannot represent.
pub fn import_chronexa_json(input: &str) -> Result<ChronexaImport, ImportError> {
    let root: Value = serde_json::from_str(input).map_err(|e| err(e.to_string()))?;
    allowed_fields(
        &root,
        &[
            "schoolName",
            "daysPerWeek",
            "periodsPerDay",
            "teachers",
            "classes",
            "classrooms",
            "subjects",
            "lessons",
            "relations",
            "cards",
            "daysDefs",
            "groups",
            "divisions",
            "globals",
            "bell",
            "bells",
            "settings",
        ],
        "school",
    )?;
    reject_fields(
        &root,
        &[
            "relations",
            "cards",
            "daysDefs",
            "groups",
            "divisions",
            "globals",
        ],
        "school",
    )?;
    let days =
        u8::try_from(number(&root, "daysPerWeek", 5)?).map_err(|_| err("invalid daysPerWeek"))?;
    let periods = u8::try_from(number(&root, "periodsPerDay", 8)?)
        .map_err(|_| err("invalid periodsPerDay"))?;
    if days == 0 || periods == 0 || u16::from(days) * u16::from(periods) > 64 {
        return Err(err("week must contain 1..=64 slots"));
    }
    // Bells, per-class schedules, and settings must be explicitly translated
    // before they can safely participate in generation.
    if root.get("bell").is_some_and(Value::is_object)
        || root
            .get("bells")
            .and_then(Value::as_array)
            .is_some_and(|x| !x.is_empty())
    {
        return Err(err("unsupported school bell definitions"));
    }
    reject_fields(&root, &["settings"], "school")?;
    let teachers = array(&root, "teachers")?;
    let classes = array(&root, "classes")?;
    let rooms = array(&root, "classrooms")?;
    let subjects = array(&root, "subjects")?;
    let lessons = array(&root, "lessons")?;
    let ti = ids(teachers, "teacher")?;
    let ci = ids(classes, "class")?;
    let ri = ids(rooms, "room")?;
    let si = ids(subjects, "subject")?;
    let li = ids(lessons, "lesson")?;
    let mut doc = TimetableDoc {
        days,
        periods_per_day: periods,
        ..Default::default()
    };
    for t in teachers {
        allowed_fields(
            t,
            &["id", "name", "subjects", "timeOff", "constraints"],
            "teacher",
        )?;
        reject_fields(t, &["constraints"], "teacher")?;
        let id = TeacherId(ti[string(t, "id")?]);
        doc.teachers.insert(
            id,
            Teacher {
                id,
                name: string(t, "name")?.into(),
                availability: availability(t.get("timeOff"), days, periods)?,
            },
        );
    }
    for c in classes {
        allowed_fields(
            c,
            &["id", "name", "size", "bellId", "constraints", "timeOff"],
            "class",
        )?;
        reject_fields(c, &["bellId", "constraints", "timeOff"], "class")?;
        let id = ClassId(ci[string(c, "id")?]);
        doc.classes.insert(
            id,
            Class {
                id,
                name: string(c, "name")?.into(),
                size: u32::try_from(number(c, "size", 0)?)
                    .map_err(|_| err("invalid class size"))?,
            },
        );
    }
    for r in rooms {
        allowed_fields(
            r,
            &["id", "name", "capacity", "tags", "constraints", "timeOff"],
            "room",
        )?;
        reject_fields(r, &["constraints", "timeOff"], "room")?;
        let id = RoomId(ri[string(r, "id")?]);
        doc.rooms.insert(
            id,
            Room {
                id,
                name: string(r, "name")?.into(),
                capacity: u32::try_from(number(r, "capacity", 0)?)
                    .map_err(|_| err("invalid room capacity"))?,
            },
        );
    }
    for s in subjects {
        allowed_fields(s, &["id", "name", "color", "constraints"], "subject")?;
        reject_fields(s, &["constraints"], "subject")?;
        let id = SubjectId(si[string(s, "id")?]);
        doc.subjects.insert(
            id,
            Subject {
                id,
                name: string(s, "name")?.into(),
            },
        );
    }
    let mut cards = BTreeMap::new();
    let mut requests = Vec::new();
    let mut next = 1u64;
    let mut sorted = lessons.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|l| l.get("id").and_then(Value::as_str).unwrap_or(""));
    for l in sorted {
        allowed_fields(
            l,
            &[
                "id",
                "subjectId",
                "classIds",
                "teacherIds",
                "periodsPerWeek",
                "periodsPerCard",
                "isLabDouble",
                "classroomIdsExpanded",
                "_lessonRoomIds",
                "preferredRoomId",
                "constraints",
                "fixedDay",
                "fixedPeriod",
                "daysDefId",
                "groupIds",
                "conditional",
                "isOptional",
            ],
            "lesson",
        )?;
        reject_fields(
            l,
            &[
                "constraints",
                "fixedDay",
                "fixedPeriod",
                "daysDefId",
                "groupIds",
                "conditional",
                "isOptional",
            ],
            "lesson",
        )?;
        let source = string(l, "id")?;
        let subject = string(l, "subjectId")?;
        let subject = SubjectId(
            *si.get(subject)
                .ok_or_else(|| err(format!("unknown subject ID {subject}")))?,
        );
        let duration = u8::try_from(number(
            l,
            "periodsPerCard",
            if l.get("isLabDouble") == Some(&Value::Bool(true)) {
                2
            } else {
                1
            },
        )?)
        .map_err(|_| err("invalid lesson duration"))?;
        let weekly = number(l, "periodsPerWeek", 0)?;
        if duration == 0 || weekly == 0 || weekly % u64::from(duration) != 0 {
            return Err(err(format!(
                "lesson {source} has inconsistent weekly periods and duration"
            )));
        }
        let occurrences =
            u16::try_from(weekly / u64::from(duration)).map_err(|_| err("too many occurrences"))?;
        let lesson = LessonId(li[source]);
        let class_ids = references(l, "classIds", &ci)?
            .into_iter()
            .map(ClassId)
            .collect();
        let teacher_ids = references(l, "teacherIds", &ti)?
            .into_iter()
            .map(TeacherId)
            .collect();
        let room_field = if l
            .get("classroomIdsExpanded")
            .and_then(Value::as_array)
            .is_some_and(|x| !x.is_empty())
        {
            "classroomIdsExpanded"
        } else {
            "_lessonRoomIds"
        };
        let mut allowed = references(l, room_field, &ri)?
            .into_iter()
            .map(RoomId)
            .collect::<Vec<_>>();
        if allowed.is_empty() {
            if let Some(room) = l
                .get("preferredRoomId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                allowed.push(RoomId(
                    *ri.get(room)
                        .ok_or_else(|| err(format!("unknown room ID {room}")))?,
                ));
            }
        }
        doc.lessons.insert(
            lesson,
            Lesson {
                id: lesson,
                subject,
                teachers: teacher_ids,
                classes: class_ids,
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration,
                occurrences,
            },
        );
        let mut occurrence_ids = Vec::new();
        for _ in 0..occurrences {
            let card = CardId(next);
            next += 1;
            occurrence_ids.push(card);
            requests.push(ImportedCard {
                card,
                lesson,
                rooms: allowed.clone(),
            });
        }
        cards.insert(source.to_owned(), occurrence_ids);
    }
    Ok(ChronexaImport {
        document: doc,
        requests,
        cards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn input() -> Value {
        json!({"daysPerWeek": 2, "periodsPerDay": 4,
          "teachers": [{"id":"t", "name":"T"}],
          "classes": [{"id":"c", "name":"C", "size":20}],
          "classrooms": [{"id":"r", "name":"R", "capacity":25}],
          "subjects": [{"id":"s", "name":"S"}],
          "lessons": [{"id":"l", "subjectId":"s", "teacherIds":["t"], "classIds":["c"], "periodsPerWeek":4, "periodsPerCard":2, "preferredRoomId":"r"}]})
    }
    #[test]
    fn expands_double_lessons_and_room_choices() -> Result<(), ImportError> {
        let imported = import_chronexa_json(&input().to_string())?;
        assert_eq!(imported.cards["l"], vec![CardId(1), CardId(2)]);
        assert_eq!(imported.requests[0].rooms, vec![RoomId(1)]);
        assert_eq!(imported.document.lessons[&LessonId(1)].duration, 2);
        Ok(())
    }
    #[test]
    fn rejects_unknown_constraints_and_dangling_ids() {
        let mut v = input();
        v["lessons"][0]["periodsPerDay"] = json!(1);
        assert!(import_chronexa_json(&v.to_string()).is_err());
        let mut v = input();
        v["lessons"][0]["teacherIds"] = json!(["missing"]);
        assert!(import_chronexa_json(&v.to_string()).is_err());
        let mut v = input();
        v["lessons"][0]["periodsPerWeek"] = json!(3);
        assert!(import_chronexa_json(&v.to_string()).is_err());
    }
    #[test]
    fn identifiers_do_not_depend_on_input_order() -> Result<(), ImportError> {
        let mut v = input();
        v["lessons"]
            .as_array_mut()
            .ok_or_else(|| err("lessons"))?
            .push(json!({"id":"a","subjectId":"s","periodsPerWeek":1}));
        let a = import_chronexa_json(&v.to_string())?;
        v["lessons"]
            .as_array_mut()
            .ok_or_else(|| err("lessons"))?
            .reverse();
        let b = import_chronexa_json(&v.to_string())?;
        assert_eq!(a.cards, b.cards);
        assert_eq!(a.requests, b.requests);
        Ok(())
    }
    #[test]
    fn imports_both_time_off_encodings() -> Result<(), ImportError> {
        let mut v = input();
        v["teachers"][0]["timeOff"] = json!({"0_1":"blocked", "1_3":"conditional"});
        let a = import_chronexa_json(&v.to_string())?;
        let availability = a.document.teachers[&TeacherId(1)].availability;
        assert_eq!(availability.blocked, 1);
        assert_eq!(availability.undesirable, 1 << 6);
        v["teachers"][0]["timeOff"] = json!([[2, 0, 0, 0], [0, 0, 1, 0]]);
        let b = import_chronexa_json(&v.to_string())?;
        assert_eq!(
            availability,
            b.document.teachers[&TeacherId(1)].availability
        );
        Ok(())
    }
}
