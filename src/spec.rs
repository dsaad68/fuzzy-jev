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

/// What a `-q` file is written in: a JSON object of ids to questions in the endpoint's shape, or a
/// TOML document with a `[[question]]` table per question, in the order asked:
///
/// ```toml
/// [[question]]
/// name = "team"
/// type = "choice"
/// instruction = "Which team should handle this?"
/// criteria = { billing = "charges, refunds", support = "bugs, outages" }
/// ```
///
/// A Score's criteria are its levels, lowest first, and a Noul's `{ true = "…", false = "…" }`, as
/// in JSON. TOML has no `null`: a Choice's options without descriptions are a list of names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Toml,
}

impl Format {
    /// The format a file named `name` is in: by its extension when it has a known one (`.json`,
    /// `.toml`), and otherwise — standard input, or a name like `questions.txt` — by its text. A
    /// JSON questions file is an object, so it starts with `{`, which no TOML document can.
    pub fn guess(name: Option<&str>, text: &str) -> Format {
        let extension = name.and_then(|name| std::path::Path::new(name).extension()).and_then(|extension| extension.to_str());
        match extension.map(str::to_ascii_lowercase).as_deref() {
            Some("json") => Format::Json,
            Some("toml") => Format::Toml,
            _ if text.trim_start_matches('\u{feff}').trim_start().starts_with('{') => Format::Json,
            _ => Format::Toml,
        }
    }
}

/// A `-q` file's questions, in the order the file has them, from JSON. The text is the file's;
/// where it came from — the terminal's disk or the shell's files — is the caller's business.
pub fn questions_file(text: &str) -> Result<Vec<(String, Question)>, String> {
    questions_in(text, Format::Json)
}

/// A `-q` file's questions, in the order the file has them, from `text` in `format`.
pub fn questions_in(text: &str, format: Format) -> Result<Vec<(String, Question)>, String> {
    let questions = match format {
        Format::Json => {
            let Ordered(questions) =
                serde_json::from_str(text).map_err(|error| format!("expected a JSON object of question ids to questions: {error}"))?;
            questions
        }
        Format::Toml => {
            let TomlFile { question } = toml::from_str(text).map_err(|error| {
                format!("expected [[question]] tables, each with a name, a type and an instruction: {}", error.to_string().trim_end())
            })?;
            let questions: Vec<(String, Question)> = question.into_iter().map(TomlQuestion::into_question).collect();
            for (at, (name, _)) in questions.iter().enumerate() {
                if name.trim().is_empty() {
                    return Err(format!("question {} has an empty name", at + 1));
                }
                // Two tables can give one name, and asked as one map, one of them would be lost.
                // TOML has no other way to say it, so it is refused here, where it is written.
                if questions[..at].iter().any(|(earlier, _)| earlier == name) {
                    return Err(format!("question `{name}` is asked twice"));
                }
            }
            questions
        }
    };
    // The limits `Client::request` checks, so that a file's question that can't be answered fails
    // here, before a call, as a flag's does, and `--dry-run` finds it too.
    for (id, question) in &questions {
        question.check().map_err(|why| format!("question `{id}`: {why}"))?;
    }
    Ok(questions)
}

/// A TOML questions file: its `[[question]]` tables. None is an empty file, which asks nothing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlFile {
    #[serde(default)]
    question: Vec<TomlQuestion>,
}

/// One `[[question]]`: the endpoint's question, with its id as `name` and its instructions as
/// `instruction`. As strict as JSON's: a field it doesn't know is an error.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum TomlQuestion {
    Choice { name: String, instruction: Value, criteria: crate::Options },
    Score { name: String, instruction: Value, criteria: Vec<Value> },
    Noul { name: String, instruction: Value, criteria: Option<crate::NoulCriteria> },
}

impl TomlQuestion {
    fn into_question(self) -> (String, Question) {
        match self {
            TomlQuestion::Choice { name, instruction, criteria } => (name, Question::Choice { instructions: instruction, criteria }),
            TomlQuestion::Score { name, instruction, criteria } => (name, Question::Score { instructions: instruction, criteria }),
            TomlQuestion::Noul { name, instruction, criteria } => (name, Question::Noul { instructions: instruction, criteria }),
        }
    }
}

/// `questions` as a JSON object in their order, two-space indented: what a TOML file's questions
/// are as JSON, for what reads only JSON (the `--explore` page).
pub fn questions_json(questions: &[(String, Question)]) -> String {
    struct InOrder<'a>(&'a [(String, Question)]);
    impl serde::Serialize for InOrder<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_map(self.0.iter().map(|(id, question)| (id, question)))
        }
    }
    serde_json::to_string_pretty(&InOrder(questions)).expect("questions are JSON")
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
        // What the endpoint can't answer is refused here, as a flag's question is.
        let empty = questions_file(r#"{"s": {"type": "score", "instructions": "?", "criteria": []}}"#).unwrap_err();
        assert!(empty.contains("question `s`: a score needs levels"), "{empty}");
        let eleven =
            questions_file(&format!(r#"{{"s": {{"type": "score", "instructions": "?", "criteria": {:?}}}}}"#, ["l"; 11])).unwrap_err();
        assert!(eleven.contains("up to 10 levels"), "{eleven}");
        let twice = questions_file(r#"{"c": {"type": "choice", "instructions": "?", "criteria": {"a": "", "a": "again"}}}"#).unwrap_err();
        assert!(twice.contains("option `a` is there twice"), "{twice}");
        // A misspelt field is an error, not a question quietly asked without it.
        let typo = questions_file(r#"{"u": {"type": "noul", "instructions": "?", "critera": {"true": "a", "false": "b"}}}"#).unwrap_err();
        assert!(typo.contains("unknown field `critera`"), "{typo}");
        let inner = questions_file(r#"{"u": {"type": "noul", "instructions": "?", "criteria": {"ture": "a", "false": "b"}}}"#).unwrap_err();
        assert!(inner.contains("unknown field `ture`"), "{inner}");
    }

    #[test]
    fn a_choice_s_options_may_be_a_list_of_names() {
        let questions = questions_file(r#"{"c": {"type": "choice", "instructions": "Which?", "criteria": ["b", "a"]}}"#).unwrap();
        assert_eq!(questions[0].1, Question::choice("Which?", [("b", Value::Null), ("a", Value::Null)]));
        let twice = questions_file(r#"{"c": {"type": "choice", "instructions": "?", "criteria": ["a", "a"]}}"#).unwrap_err();
        assert!(twice.contains("option `a` is there twice"), "{twice}");
        let not_names = questions_file(r#"{"c": {"type": "choice", "instructions": "?", "criteria": [1]}}"#).unwrap_err();
        assert!(not_names.contains("question `c`"), "{not_names}");
    }
}

#[cfg(test)]
mod toml_tests {
    use serde_json::{json, Value};

    use super::{questions_file, questions_in, questions_json, Format};
    use crate::Question;

    fn toml(text: &str) -> Result<Vec<(String, Question)>, String> {
        questions_in(text, Format::Toml)
    }

    #[test]
    fn reads_each_kind_of_question_in_order() {
        let questions = toml(
            r#"
            [[question]]
            name = "zeta"
            type = "noul"
            instruction = "Is it?"
            criteria = { true = { what = "yes" }, false = "no" }

            [[question]]
            type = "choice"
            name = "alpha"
            instruction = "Which?"
            criteria = { b = "second", a = "first" }

            [[question]]
            name = "mid"
            type = "score"
            instruction = "How much?"
            criteria = ["Low", "High"]

            [[question]]
            name = "bare"
            type = "noul"
            instruction = "Is it, plainly?"
            "#,
        )
        .unwrap();
        let names: Vec<&str> = questions.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["zeta", "alpha", "mid", "bare"]);
        assert_eq!(questions[0].1, Question::noul_with_criteria("Is it?", json!({"what": "yes"}), "no"));
        assert_eq!(questions[1].1, Question::choice("Which?", [("b", "second"), ("a", "first")]));
        assert_eq!(questions[2].1, Question::score("How much?", ["Low", "High"]));
        assert_eq!(questions[3].1, Question::noul("Is it, plainly?"));
    }

    #[test]
    fn reads_criteria_as_a_table_of_its_own_and_options_without_descriptions() {
        // The criteria may be a [question.criteria] table, which keeps its order too, and the
        // instruction structured, as the endpoint takes it.
        let questions = toml(
            r#"
            [[question]]
            name = "team"
            type = "choice"

            [question.instruction]
            question = "Which team?"
            focus = "the primary request"

            [question.criteria]
            "z team" = "last letter"
            a = ""
            "#,
        )
        .unwrap();
        assert_eq!(
            questions[0].1,
            Question::choice(json!({"question": "Which team?", "focus": "the primary request"}), [("z team", "last letter"), ("a", "")])
        );
        // TOML has no null: options without descriptions are a list of names.
        let names =
            toml("[[question]]\nname = \"c\"\ntype = \"choice\"\ninstruction = \"?\"\ncriteria = [\"sales\", \"billing\"]\n").unwrap();
        assert_eq!(names[0].1, Question::choice("?", [("sales", Value::Null), ("billing", Value::Null)]));
    }

    #[test]
    fn the_triage_example_is_the_same_in_toml_and_json() {
        let from_json = questions_file(include_str!("../examples/rules/triage.json")).unwrap();
        let from_toml = toml(include_str!("../examples/rules/triage.questions.toml")).unwrap();
        assert_eq!(from_toml, from_json);
    }

    fn noul(fields: &str) -> String {
        toml(&format!("[[question]]\n{fields}\n")).unwrap_err()
    }

    #[test]
    fn says_what_is_wrong_with_a_toml_file() {
        let not_toml = toml("question = ").unwrap_err();
        assert!(not_toml.starts_with("expected [[question]] tables"), "{not_toml}");
        // The JSON file's shape, a table per id, isn't this one.
        let by_id = toml("[team]\ntype = \"noul\"\ninstructions = \"?\"\n").unwrap_err();
        assert!(by_id.contains("unknown field `team`"), "{by_id}");
        let one_table = toml("[question]\nname = \"n\"\ntype = \"noul\"\ninstruction = \"?\"\n").unwrap_err();
        assert!(one_table.starts_with("expected [[question]] tables"), "{one_table}");

        assert!(noul("type = \"noul\"\ninstruction = \"?\"").contains("missing field `name`"));
        assert!(noul("name = \"n\"\ninstruction = \"?\"").contains("type"));
        assert!(noul("name = \"n\"\ntype = \"noul\"").contains("missing field `instruction`"));
        assert!(noul("name = \"n\"\ntype = \"ranking\"\ninstruction = \"?\"").contains("ranking"));
        // The JSON spelling, and a typo, are errors, not a question asked without them.
        assert!(noul("name = \"n\"\ntype = \"noul\"\ninstruction = \"?\"\ninstructions = \"?\"").contains("unknown field `instructions`"));
        assert!(noul("name = \"n\"\ntype = \"noul\"\ninstruction = \"?\"\ncritera = []").contains("unknown field `critera`"));
        assert!(noul("name = \"n\"\ntype = \"noul\"\ninstruction = \"?\"\ncriteria = { ture = \"a\", false = \"b\" }")
            .contains("unknown field `ture`"));
        assert!(noul("name = \"c\"\ntype = \"choice\"\ninstruction = \"?\"").contains("missing field `criteria`"));
        assert!(noul("name = 3\ntype = \"noul\"\ninstruction = \"?\"").contains("name"));
    }

    #[test]
    fn refuses_what_the_endpoint_cant_answer() {
        let empty = noul("name = \"s\"\ntype = \"score\"\ninstruction = \"?\"\ncriteria = []");
        assert!(empty.contains("question `s`: a score needs levels"), "{empty}");
        let eleven = noul(&format!("name = \"s\"\ntype = \"score\"\ninstruction = \"?\"\ncriteria = {:?}", ["l"; 11]));
        assert!(eleven.contains("up to 10 levels"), "{eleven}");
        let none = noul("name = \"c\"\ntype = \"choice\"\ninstruction = \"?\"\ncriteria = {}");
        assert!(none.contains("question `c`: a choice needs options"), "{none}");
        let names_twice = noul("name = \"c\"\ntype = \"choice\"\ninstruction = \"?\"\ncriteria = [\"a\", \"a\"]");
        assert!(names_twice.contains("option `a` is there twice"), "{names_twice}");
        // TOML itself refuses a key written twice, which JSON would quietly keep one of.
        let option_twice = noul("name = \"c\"\ntype = \"choice\"\ninstruction = \"?\"\ncriteria = { a = \"\", a = \"again\" }");
        assert!(option_twice.contains("duplicate key"), "{option_twice}");
    }

    #[test]
    fn refuses_two_questions_with_one_name_or_none() {
        let twice = toml(
            "[[question]]\nname = \"q\"\ntype = \"noul\"\ninstruction = \"?\"\n\
             [[question]]\nname = \"q\"\ntype = \"noul\"\ninstruction = \"Again?\"\n",
        )
        .unwrap_err();
        assert_eq!(twice, "question `q` is asked twice");
        let blank = toml(
            "[[question]]\nname = \"a\"\ntype = \"noul\"\ninstruction = \"?\"\n\
             [[question]]\nname = \" \"\ntype = \"noul\"\ninstruction = \"?\"\n",
        )
        .unwrap_err();
        assert_eq!(blank, "question 2 has an empty name");
    }

    #[test]
    fn an_empty_toml_file_has_no_questions() {
        assert_eq!(toml("").unwrap(), []);
        assert_eq!(toml("# nothing yet\n").unwrap(), []);
        assert_eq!(toml("question = []\n").unwrap(), []);
    }

    #[test]
    fn guesses_the_format_from_the_name_then_the_text() {
        assert_eq!(Format::guess(Some("q.json"), "[[question]]"), Format::Json);
        assert_eq!(Format::guess(Some("q.toml"), "{}"), Format::Toml);
        assert_eq!(Format::guess(Some("dir/Q.TOML"), "{}"), Format::Toml);
        assert_eq!(Format::guess(Some("dir/Q.Json"), "[[question]]"), Format::Json);
        assert_eq!(Format::guess(Some("-"), "  \n{\"q\": {}}"), Format::Json);
        assert_eq!(Format::guess(None, "\u{feff}{}"), Format::Json);
        assert_eq!(Format::guess(Some("questions.txt"), "[[question]]\nname = \"n\""), Format::Toml);
        assert_eq!(Format::guess(None, "# a comment\n[[question]]"), Format::Toml);
        assert_eq!(Format::guess(None, ""), Format::Toml);
    }

    #[test]
    fn writes_questions_back_as_json_in_order() {
        let questions = toml(include_str!("../examples/rules/triage.questions.toml")).unwrap();
        let json = questions_json(&questions);
        assert!(json.starts_with("{\n  \"team\": {"), "{json}");
        assert_eq!(questions_file(&json).unwrap(), questions);
        assert_eq!(questions_json(&[]), "{}");
    }
}
