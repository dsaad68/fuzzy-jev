//! The decisions endpoint's request and reply, as TypeSafe documents them
//! (https://docs.typesafe.ai/primitives). Instructions and every criterion are JSON values: a
//! string usually, but an object, an array or `null` works too (the docs' "Advanced: structure").

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::error::{Error, Result};

/// One request: a state, and questions about it under ids of the caller's choosing. Every question
/// sees the same state and is answered independently of the others.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionRequest {
    pub model: String,
    pub state: Value,
    pub questions: BTreeMap<String, Question>,
}

/// A question. The id it is sent under is never shown to the model, so `instructions` should say
/// everything. It reads from JSON too, in the endpoint's own shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Which one of these options? `criteria` maps each option to its description.
    Choice { instructions: Value, criteria: Options },
    /// Which level, from the lowest to the highest? Two to ten levels.
    Score { instructions: Value, criteria: Vec<Value> },
    /// Is this true? The answer is the probability of yes.
    Noul {
        instructions: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
}

impl Question {
    /// A Choice between `options`: (name, description) pairs, kept in the order given.
    pub fn choice<K: Into<String>, V: Into<Value>>(instructions: impl Into<Value>, options: impl IntoIterator<Item = (K, V)>) -> Question {
        let options = options.into_iter().map(|(name, description)| (name.into(), description.into())).collect();
        Question::Choice { instructions: instructions.into(), criteria: Options(options) }
    }

    /// A Score over `levels`, from the lowest (level 0) to the highest.
    pub fn score<V: Into<Value>>(instructions: impl Into<Value>, levels: impl IntoIterator<Item = V>) -> Question {
        Question::Score { instructions: instructions.into(), criteria: levels.into_iter().map(Into::into).collect() }
    }

    /// A Noul: the yes/no question alone.
    pub fn noul(instructions: impl Into<Value>) -> Question {
        Question::Noul { instructions: instructions.into(), criteria: None }
    }

    /// A Noul with what a yes and a no mean, for when the boundary is subtle.
    pub fn noul_with_criteria(instructions: impl Into<Value>, yes: impl Into<Value>, no: impl Into<Value>) -> Question {
        Question::Noul { instructions: instructions.into(), criteria: Some(NoulCriteria { yes: yes.into(), no: no.into() }) }
    }
}

impl Question {
    /// Whether the endpoint's documented limits hold, so a request that cannot be answered fails
    /// here rather than after a round trip. The endpoint is the authority on the rest.
    pub fn check(&self) -> std::result::Result<(), String> {
        match self {
            Question::Choice { criteria: Options(options), .. } => {
                if options.is_empty() {
                    return Err("a choice needs options to pick from".to_owned());
                }
                // The options are sent as a JSON object, where a repeated name would be one key.
                for (at, (name, _)) in options.iter().enumerate() {
                    if options[..at].iter().any(|(earlier, _)| earlier == name) {
                        return Err(format!("option `{name}` is there twice"));
                    }
                }
                if options.len() > CHOICE_OPTIONS {
                    return Err(format!("a choice takes up to {CHOICE_OPTIONS} options, not {}", options.len()));
                }
            }
            Question::Score { criteria: levels, .. } => {
                if levels.is_empty() {
                    return Err("a score needs levels to place the state on".to_owned());
                }
                if levels.len() > SCORE_LEVELS {
                    return Err(format!("a score takes up to {SCORE_LEVELS} levels, not {}", levels.len()));
                }
            }
            Question::Noul { .. } => {}
        }
        Ok(())
    }
}

/// How many options a Choice takes, and how many levels a Score takes, as the docs give them. No
/// minimum of two levels is enforced: the docs ask for two, but the endpoint answers a single one.
const CHOICE_OPTIONS: usize = 255;
const SCORE_LEVELS: usize = 10;

/// A Choice's options, sent as a JSON object in the order they were given.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options(pub Vec<(String, Value)>);

impl Serialize for Options {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, description) in &self.0 {
            map.serialize_entry(name, description)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Options {
    /// Keeps the options in the order the JSON has them.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Options, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Options;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object of options and their descriptions")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<Options, A::Error> {
                let mut options = Vec::new();
                while let Some(option) = map.next_entry()? {
                    options.push(option);
                }
                Ok(Options(options))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

/// What a Noul's yes and no mean.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: Value,
    #[serde(rename = "false")]
    pub no: Value,
}

/// The reply: one answer per question, under the ids from the request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    /// The model that answered, with its date, such as `typesafe/jev-1.13-20260917`.
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    pub usage: Usage,
    /// OpenRouter's id for the call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl DecisionResponse {
    /// The answer to question `id`.
    pub fn answer(&self, id: &str) -> Result<&Answer> {
        self.answers.get(id).ok_or_else(|| Error::MissingAnswer(id.to_owned()))
    }

    /// The Choice answer to question `id`.
    pub fn choice(&self, id: &str) -> Result<&ChoiceAnswer> {
        match self.answer(id)? {
            Answer::Choice(answer) => Ok(answer),
            other => Err(wrong_type(id, "choice", other)),
        }
    }

    /// The Score answer to question `id`.
    pub fn score(&self, id: &str) -> Result<&ScoreAnswer> {
        match self.answer(id)? {
            Answer::Score(answer) => Ok(answer),
            other => Err(wrong_type(id, "score", other)),
        }
    }

    /// The probability of yes for Noul question `id`.
    pub fn noul(&self, id: &str) -> Result<f64> {
        match self.answer(id)? {
            Answer::Noul(answer) => Ok(answer.noul),
            other => Err(wrong_type(id, "noul", other)),
        }
    }
}

fn wrong_type(id: &str, expected: &'static str, found: &Answer) -> Error {
    Error::WrongType { id: id.to_owned(), expected, found: found.kind().to_owned() }
}

/// One question's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
    Noul(NoulAnswer),
    /// An answer of a type this crate doesn't know yet, kept as it arrived so that nothing the
    /// endpoint sent is lost on the way back out.
    Other(Value),
}

impl Answer {
    /// The question type, as the endpoint names it.
    pub fn kind(&self) -> &str {
        match self {
            Answer::Choice(_) => "choice",
            Answer::Score(_) => "score",
            Answer::Noul(_) => "noul",
            Answer::Other(answer) => answer.get("type").and_then(Value::as_str).unwrap_or("an answer of another type"),
        }
    }
}

/// The three types this crate knows, tagged as the endpoint tags them. [`Answer`] hands anything
/// else to `Value`, which no derived enum can do: `#[serde(other)]` takes no payload.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Known<'a> {
    Choice(Cow<'a, ChoiceAnswer>),
    Score(Cow<'a, ScoreAnswer>),
    Noul(Cow<'a, NoulAnswer>),
}

impl Serialize for Answer {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Answer::Choice(answer) => Known::Choice(Cow::Borrowed(answer)).serialize(serializer),
            Answer::Score(answer) => Known::Score(Cow::Borrowed(answer)).serialize(serializer),
            Answer::Noul(answer) => Known::Noul(Cow::Borrowed(answer)).serialize(serializer),
            Answer::Other(answer) => answer.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Answer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Answer, D::Error> {
        let answer = Value::deserialize(deserializer)?;
        let known = match answer.get("type").and_then(Value::as_str) {
            Some("choice" | "score" | "noul") => serde_json::from_value::<Known>(answer.clone()).map_err(D::Error::custom)?,
            _ => return Ok(Answer::Other(answer)),
        };
        Ok(match known {
            Known::Choice(answer) => Answer::Choice(answer.into_owned()),
            Known::Score(answer) => Answer::Score(answer.into_owned()),
            Known::Noul(answer) => Answer::Noul(answer.into_owned()),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    /// The most probable option.
    pub choice: String,
    /// From 0 to 1: how peaked `probabilities` is.
    pub confidence: f64,
    /// Every option's probability; they add up to 1.
    pub probabilities: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    /// The expected level, from 0 to the top level; it can fall between two levels.
    pub score: f64,
    /// From 0 to 1: how peaked `probabilities` is.
    pub confidence: f64,
    /// Each level's probability, by level number; they add up to 1.
    #[serde(deserialize_with = "by_level")]
    pub probabilities: BTreeMap<u8, f64>,
    /// Each level's description, by level number.
    #[serde(default, deserialize_with = "by_level")]
    pub legend: BTreeMap<u8, Value>,
}

/// A map keyed by level numbers, which arrive as strings (`"0"`, `"1"`, …). serde_json reads those
/// into numbers on its own, but not inside a tagged enum such as [`Answer`], which buffers them first.
fn by_level<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> std::result::Result<BTreeMap<u8, T>, D::Error> {
    BTreeMap::<String, T>::deserialize(deserializer)?
        .into_iter()
        .map(|(level, value)| {
            level.parse().map(|level| (level, value)).map_err(|_| D::Error::custom(format!("level {level:?} is not a number")))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// The probability that the answer is yes.
    pub noul: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// What the call cost, in US dollars, when OpenRouter says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}
