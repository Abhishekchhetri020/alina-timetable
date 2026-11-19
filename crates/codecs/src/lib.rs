#![deny(clippy::unwrap_used)]
//! Explicit, conservative adapter for a subset of Chronexa's school JSON.
//! Unsupported rule-bearing data is rejected rather than silently discarded.
use alina_doc::{Class, Lesson, Room, Subject, Teacher, TimetableDoc};
use alina_foundation::{
    Availability, CardId, ClassId, LessonId, RoomId, SubjectId, TeacherId, TimeSlot,
};
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

/// Return a period mask and whether the bell actually defined periods. An
/// explicitly all-break bell is distinct from an absent/empty definition.
fn bell_mask(bell: &Value, periods: u8) -> Result<(u64, bool), ImportError> {
    let Some(entries) = bell.get("periods") else {
        return Ok((0, false));
    };
    let entries = entries
        .as_array()
        .ok_or_else(|| err("bell.periods must be an array"))?;
    let mut mask = 0u64;
    let mut seen = BTreeSet::new();
    for item in entries {
        let index = number(item, "index", 0)?;
        if index == 0 || index > u64::from(periods) || !seen.insert(index) {
            return Err(err("invalid or duplicate bell period index"));
        }
        match item.get("isTeaching") {
            None | Some(Value::Bool(true)) => mask |= 1u64 << (index - 1),
            Some(Value::Bool(false)) => {}
            _ => return Err(err("invalid bell isTeaching")),
        }
    }
    Ok((mask, !entries.is_empty()))
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
        &["relations", "cards", "groups", "divisions", "globals"],
        "school",
    )?;
    let days =
        u8::try_from(number(&root, "daysPerWeek", 5)?).map_err(|_| err("invalid daysPerWeek"))?;
    let periods = u8::try_from(number(&root, "periodsPerDay", 8)?)
        .map_err(|_| err("invalid periodsPerDay"))?;
    if days == 0 || periods == 0 || u16::from(days) * u16::from(periods) > 64 {
        return Err(err("week must contain 1..=64 slots"));
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
    let mut day_definitions = BTreeMap::new();
    if let Some(value) = root.get("daysDefs") {
        let definitions = value
            .as_array()
            .ok_or_else(|| err("daysDefs must be an array"))?;
        for definition in definitions {
            let id = string(definition, "id")?;
            let bits = definition
                .get("bits")
                .or_else(|| definition.get("days"))
                .and_then(Value::as_str)
                .ok_or_else(|| err("day definition needs bits"))?;
            let mut mask = 0u64;
            for group in bits.split(',') {
                for (day, ch) in group.trim().chars().enumerate() {
                    match ch {
                        '1' if day < usize::from(days) => mask |= 1u64 << day,
                        '0' => {}
                        '1' => return Err(err("day definition outside week")),
                        _ => return Err(err("invalid day definition bits")),
                    }
                }
            }
            if mask == 0 {
                return Err(err("empty day definition"));
            }
            if day_definitions.insert(id.to_owned(), mask).is_some() {
                return Err(err("duplicate day definition"));
            }
        }
    }
    let mut bells = BTreeMap::new();
    if let Some(value) = root.get("bells") {
        let entries = value
            .as_array()
            .ok_or_else(|| err("bells must be an array"))?;
        for bell in entries {
            let id = string(bell, "id")?;
            if bells
                .insert(id.to_owned(), bell_mask(bell, periods)?)
                .is_some()
            {
                return Err(err(format!("duplicate bell ID {id}")));
            }
        }
    }
    let default_bell = match root.get("bell") {
        None | Some(Value::Null) => (0, false),
        Some(Value::Object(_)) => bell_mask(&root["bell"], periods)?,
        Some(Value::String(id)) => *bells
            .get(id)
            .ok_or_else(|| err(format!("unknown default bell {id}")))?,
        _ => return Err(err("invalid default bell")),
    };
    let all_periods = if periods == 64 {
        u64::MAX
    } else {
        (1u64 << periods) - 1
    };
    let default_mask = if default_bell.1 {
        default_bell.0
    } else {
        all_periods
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
        reject_fields(c, &["constraints", "timeOff"], "class")?;
        let id = ClassId(ci[string(c, "id")?]);
        let class_bell = match c.get("bellId") {
            None | Some(Value::Null) => default_bell,
            Some(Value::String(bell)) if bell.is_empty() => default_bell,
            Some(Value::String(bell)) => *bells
                .get(bell)
                .ok_or_else(|| err(format!("unknown class bell {bell}")))?,
            _ => return Err(err("invalid class bellId")),
        };
        let valid = if class_bell.1 {
            class_bell.0
        } else {
            default_mask
        };
        let mut blocked = 0;
        for day in 0..days {
            for period in 0..periods {
                if valid & (1u64 << period) == 0 {
                    blocked |= 1u64 << (u32::from(day) * u32::from(periods) + u32::from(period));
                }
            }
        }
        if blocked != 0 {
            doc.hard_limits.class_blocked_slots.insert(id, blocked);
        }
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
        allowed_fields(
            s,
            &["id", "name", "color", "constraints", "maxParallel"],
            "subject",
        )?;
        reject_fields(s, &["constraints"], "subject")?;
        let id = SubjectId(si[string(s, "id")?]);
        if let Some(value) = s.get("maxParallel") {
            if !value.is_null() {
                let limit = value
                    .as_u64()
                    .ok_or_else(|| err("invalid subject maxParallel"))?;
                // Chronexa treats zero as unrestricted.
                if limit > 0 {
                    doc.hard_limits.subject_max_parallel.insert(
                        id,
                        u16::try_from(limit).map_err(|_| err("subject maxParallel too large"))?,
                    );
                }
            }
        }
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
            &["constraints", "groupIds", "conditional", "isOptional"],
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
        if let Some(value) = l.get("daysDefId") {
            if !value.is_null() {
                let day_id = value.as_str().ok_or_else(|| err("invalid daysDefId"))?;
                if !day_id.is_empty() {
                    let mask = *day_definitions
                        .get(day_id)
                        .ok_or_else(|| err(format!("unknown day definition {day_id}")))?;
                    doc.hard_limits.lesson_allowed_days.insert(lesson, mask);
                }
            }
        }
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
        match (l.get("fixedDay"), l.get("fixedPeriod")) {
            (None | Some(Value::Null), None | Some(Value::Null)) => {}
            (Some(day), Some(period)) => {
                let day = day.as_u64().ok_or_else(|| err("invalid fixedDay"))?;
                let period = period.as_u64().ok_or_else(|| err("invalid fixedPeriod"))?;
                if day >= u64::from(days) || period == 0 || period > u64::from(periods) {
                    return Err(err("fixed placement outside week"));
                }
                let card = occurrence_ids
                    .first()
                    .copied()
                    .ok_or_else(|| err("no occurrence for fixed placement"))?;
                doc.hard_limits.card_fixed_start.insert(
                    card,
                    TimeSlot {
                        day: day as u8,
                        period: (period - 1) as u8,
                    },
                );
            }
            _ => return Err(err("fixedDay and fixedPeriod must be specified together")),
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
    #[test]
    fn bells_block_breaks_and_all_break_bells() -> Result<(), ImportError> {
        let mut v = input();
        v["bell"] =
            json!({"periods":[{"index":1},{"index":2,"isTeaching":false},{"index":3},{"index":4}]});
        let imported = import_chronexa_json(&v.to_string())?;
        assert_eq!(
            imported.document.hard_limits.class_blocked_slots[&ClassId(1)],
            (1 << 1) | (1 << 5)
        );
        v["bells"] = json!([{"id":"breaks","periods":[{"index":1,"isTeaching":false}]}]);
        v["classes"][0]["bellId"] = json!("breaks");
        let imported = import_chronexa_json(&v.to_string())?;
        assert_eq!(
            imported.document.hard_limits.class_blocked_slots[&ClassId(1)],
            255
        );
        Ok(())
    }
    #[test]
    fn day_patterns_are_unioned_and_checked() -> Result<(), ImportError> {
        let mut v = input();
        v["daysDefs"] = json!([{"id":"d", "bits":"10,01"}]);
        v["lessons"][0]["daysDefId"] = json!("d");
        let imported = import_chronexa_json(&v.to_string())?;
        assert_eq!(
            imported.document.hard_limits.lesson_allowed_days[&LessonId(1)],
            3
        );
        v["lessons"][0]["daysDefId"] = json!("missing");
        assert!(import_chronexa_json(&v.to_string()).is_err());
        Ok(())
    }
    #[test]
    fn lesson_anchor_pins_only_first_occurrence() -> Result<(), ImportError> {
        let mut v = input();
        v["lessons"][0]["fixedDay"] = json!(0);
        v["lessons"][0]["fixedPeriod"] = json!(2);
        let imported = import_chronexa_json(&v.to_string())?;
        assert_eq!(imported.document.hard_limits.card_fixed_start.len(), 1);
        assert_eq!(
            imported.document.hard_limits.card_fixed_start[&CardId(1)],
            TimeSlot { day: 0, period: 1 }
        );
        Ok(())
    }
}
