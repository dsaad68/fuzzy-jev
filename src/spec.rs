//! Questions written on the command line: `ID=INSTRUCTIONS`, then `|`-separated criteria.
//!
//! - `--noul 'is_urgent=Does this message convey urgency?'`, or with what a yes and a no mean:
//!   `--noul 'is_urgent=Is it urgent?|Explicitly time-sensitive|No urgency expressed'`
//! - `--choice 'department=Which team should handle this?|billing:Payments, refunds|technical:Bugs|sales'`
//!   (each option is `NAME` or `NAME:DESCRIPTION`)
//! - `--score 'frustration=How frustrated is the customer?|Calm|Frustrated|Very angry'` (lowest first)
//!
//! Text with a `|` in it, or structured criteria, go in a `--questions` file instead.

use serde::de::{Error as _, MapAccess};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::Question;

/// Which flag a question came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Noul,
    Choice,
    Score,
}

impl Kind {
    /// The flag's name, which is also clap's id for it.
    pub fn flag(self) -> &'static str {
        match self {
            Kind::Noul => "noul",
            Kind::Choice => "choice",
            Kind::Score => "score",
        }
    }
}

/// The question `spec` writes, under its id. The error says what's wrong with it.
pub fn parse(kind: Kind, spec: &str) -> Result<(String, Question), String> {
    let wrong = |why: &str| format!("--{} `{spec}`: {why}", kind.flag());
    let (id, rest) = spec.split_once('=').ok_or_else(|| wrong("expected ID=INSTRUCTIONS"))?;
    let id = id.trim();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return Err(wrong("the id before `=` must be one word"));
    }
    let mut parts = rest.split('|').map(str::trim);
    let instructions = parts.next().unwrap_or_default();
    if instructions.is_empty() {
        return Err(wrong("no instructions after `=`"));
    }
    let criteria: Vec<&str> = parts.collect();
    if criteria.iter().any(|part| part.is_empty()) {
        return Err(wrong("an empty criterion between `|`s"));
    }

    let question = match kind {
        Kind::Noul => match criteria[..] {
            [] => Question::noul(instructions),
            [yes, no] => Question::noul_with_criteria(instructions, yes, no),
            _ => return Err(wrong("a noul takes no criteria, or two: |WHAT YES MEANS|WHAT NO MEANS")),
        },
        Kind::Choice => {
            if criteria.len() < 2 {
                return Err(wrong("a choice needs at least two options: |NAME[:DESCRIPTION]|…"));
            }
            let mut options: Vec<(String, Value)> = Vec::new();
            for option in criteria {
                let (name, description) = match option.split_once(':') {
                    Some((name, description)) => (name.trim(), Value::from(description.trim())),
                    None => (option, Value::Null),
                };
                if name.is_empty() {
                    return Err(wrong(&format!("option `{option}` has no name before `:`")));
                }
                if options.iter().any(|(other, _)| other == name) {
                    return Err(wrong(&format!("option `{name}` is there twice")));
                }
                options.push((name.to_owned(), description));
            }
            Question::choice(instructions, options)
        }
        Kind::Score => {
            if !(2..=10).contains(&criteria.len()) {
                return Err(wrong("a score takes two to ten levels, lowest first: |LEVEL|LEVEL|…"));
            }
            Question::score(instructions, criteria)
        }
    };
    Ok((id.to_owned(), question))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn question(kind: Kind, spec: &str) -> Value {
        let (id, question) = parse(kind, spec).unwrap();
        json!({ id: question })
    }

    #[test]
    fn reads_a_noul() {
        assert_eq!(
            question(Kind::Noul, "is_urgent=Does this message convey urgency?"),
            json!({"is_urgent": {"type": "noul", "instructions": "Does this message convey urgency?"}})
        );
        assert_eq!(
            question(Kind::Noul, "is_urgent = Is it urgent? | Explicitly time-sensitive | No urgency expressed"),
            json!({"is_urgent": {
                "type": "noul",
                "instructions": "Is it urgent?",
                "criteria": {"true": "Explicitly time-sensitive", "false": "No urgency expressed"}
            }})
        );
    }

    #[test]
    fn reads_a_choice_with_and_without_descriptions() {
        assert_eq!(
            serde_json::to_string(&parse(Kind::Choice, "department=Which team?|sales|billing:Payments: cards, refunds").unwrap().1)
                .unwrap(),
            r#"{"type":"choice","instructions":"Which team?","criteria":{"sales":null,"billing":"Payments: cards, refunds"}}"#
        );
    }

    #[test]
    fn reads_a_score() {
        assert_eq!(
            question(Kind::Score, "frustration=How frustrated?|Calm|Frustrated|Very angry"),
            json!({"frustration": {"type": "score", "instructions": "How frustrated?", "criteria": ["Calm", "Frustrated", "Very angry"]}})
        );
    }

    #[test]
    fn says_what_is_wrong() {
        let error = |kind, spec| parse(kind, spec).unwrap_err();
        assert!(error(Kind::Noul, "Is it urgent?").contains("expected ID=INSTRUCTIONS"));
        assert!(error(Kind::Noul, "is urgent=Is it?").contains("one word"));
        assert!(error(Kind::Noul, "a=").contains("no instructions"));
        assert!(error(Kind::Noul, "a=Is it?|yes").contains("two"));
        assert!(error(Kind::Choice, "a=Which?|only").contains("at least two options"));
        assert!(error(Kind::Choice, "a=Which?|x|x:again").contains("twice"));
        assert!(error(Kind::Choice, "a=Which?|x|:no name").contains("no name"));
        assert!(error(Kind::Score, "a=How much?|low").contains("two to ten"));
        assert!(error(Kind::Score, "a=How much?|low||high").contains("empty criterion"));
    }
}

/// A `-q` file's questions, in the order the file has them. The text is the file's; where it came
/// from — the terminal's disk or the shell's files — is the caller's business.
pub fn questions_file(text: &str) -> Result<Vec<(String, Question)>, String> {
    serde_json::from_str::<Ordered>(text)
        .map(|Ordered(questions)| questions)
        .map_err(|error| format!("expected a JSON object of question ids to questions: {error}"))
}

/// A JSON object's entries in the order it has them. Read straight into `Question`s rather than
/// through a `serde_json::Value`, whose objects sort their keys, so each Choice keeps its options'
/// order too.
struct Ordered(Vec<(String, Question)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Ordered, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Ordered;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object of question ids to questions")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordered, A::Error> {
                let mut questions = Vec::new();
                while let Some(id) = map.next_key::<String>()? {
                    let question = map.next_value().map_err(|e| A::Error::custom(format!("question `{id}`: {e}")))?;
                    questions.push((id, question));
                }
                Ok(Ordered(questions))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(test)]
mod file_tests {
    use serde_json::json;

    use super::questions_file;
    use serde_json::Value;

    use crate::Question;

    #[test]
    fn reads_a_questions_file_in_order() {
        let questions = questions_file(
            r#"{
                "zeta": {"type": "noul", "instructions": "Is it?", "criteria": {"true": {"what": "yes"}, "false": "no"}},
                "alpha": {"type": "choice", "instructions": "Which?", "criteria": {"b": null, "a": "first"}}
            }"#,
        )
        .unwrap();
        assert_eq!(questions[0].0, "zeta");
        assert_eq!(questions[0].1, Question::noul_with_criteria("Is it?", json!({"what": "yes"}), "no"));
        assert_eq!(questions[1].1, Question::choice("Which?", [("b", Value::Null), ("a", Value::from("first"))]));
    }

    #[test]
    fn says_what_is_wrong_with_a_questions_file() {
        assert!(questions_file("[]").unwrap_err().contains("expected a JSON object"));
        let error = questions_file(r#"{"q": {"type": "ranking", "instructions": "?"}}"#).unwrap_err();
        assert!(error.contains("question `q`"), "{error}");
    }
}
