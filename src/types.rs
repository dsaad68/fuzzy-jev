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
///
/// Reading one is strict: a field it doesn't know (`critera` for `criteria`) is an error, not
/// dropped, since dropping it would ask a different question without a word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

    /// Whether this reply answers `questions`: every question has an answer of its own type, well
    /// formed ([`Answer::check`]), naming only options and levels the question has, and a Score's
    /// expected level within its scale. Answers to questions not asked are left alone.
    /// [`crate::Client::send`] checks every reply this way before returning it.
    pub fn check_against<'a>(&self, questions: impl IntoIterator<Item = (&'a String, &'a Question)>) -> Result<()> {
        for (id, question) in questions {
            let bad = |why: String| Error::BadAnswer { id: id.clone(), why };
            let answer = self.answer(id)?;
            match (question, answer) {
                (Question::Noul { .. }, Answer::Noul(_)) => {}
                (Question::Choice { criteria: Options(options), .. }, Answer::Choice(choice)) => {
                    let known = |name: &String| options.iter().any(|(option, _)| option == name);
                    if let Some(unknown) = choice.probabilities.keys().find(|name| !known(name)) {
                        return Err(bad(format!("it gives a probability for `{unknown}`, which isn't one of the question's options")));
                    }
                    if !known(&choice.choice) {
                        return Err(bad(format!("it chose `{}`, which isn't one of the question's options", choice.choice)));
                    }
                }
                (Question::Score { criteria: levels, .. }, Answer::Score(score)) => {
                    let top = levels.len().saturating_sub(1);
                    if let Some(level) = score.probabilities.keys().find(|level| usize::from(**level) > top) {
                        return Err(bad(format!("it gives a probability for level {level}, and the question's levels are 0 to {top}")));
                    }
                }
                (question, answer) => {
                    let expected = match question {
                        Question::Choice { .. } => "choice",
                        Question::Score { .. } => "score",
                        Question::Noul { .. } => "noul",
                    };
                    return Err(wrong_type(id, expected, answer));
                }
            }
            answer.check().map_err(bad)?;
        }
        Ok(())
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

    /// Whether it is a well-formed answer on its own: every probability finite and from 0 to 1; a
    /// Choice's or a Score's a distribution some real one could round to ([`ROUNDING`]); a
    /// `confidence` from 0 to 1; a Choice's `choice` its most probable option; and a Score's `score`
    /// an expectation its probabilities allow. An answer of another type has nothing to check. The
    /// error says what is wrong. [`DecisionResponse::check_against`] checks it against its question
    /// as well.
    pub fn check(&self) -> std::result::Result<(), String> {
        match self {
            Answer::Noul(answer) => probability("the probability of yes", answer.noul),
            Answer::Choice(answer) => {
                let values: Vec<(String, f64)> =
                    answer.probabilities.iter().map(|(option, value)| (format!("`{option}`'s probability"), *value)).collect();
                distribution(&values)?;
                probability("the confidence", answer.confidence)?;
                let chosen = answer
                    .probabilities
                    .get(&answer.choice)
                    .ok_or_else(|| format!("it chose `{}`, which has no probability of its own", answer.choice))?;
                let top = answer.probabilities.values().copied().fold(0.0, f64::max);
                // The most probable, give or take a tie that rounding made or broke.
                match *chosen >= top - 2.0 * ROUNDING - 1e-9 {
                    true => Ok(()),
                    false => Err(format!("it chose `{}` at {chosen}, while another option has {top}", answer.choice)),
                }
            }
            Answer::Score(answer) => {
                let values: Vec<(String, f64)> =
                    answer.probabilities.iter().map(|(level, value)| (format!("level {level}'s probability"), *value)).collect();
                distribution(&values)?;
                probability("the confidence", answer.confidence)?;
                let levels: Vec<(f64, f64)> = answer.probabilities.iter().map(|(level, value)| (f64::from(*level), *value)).collect();
                let (low, high) = expectation_bounds(&levels);
                match answer.score.is_finite() && answer.score >= low - ROUNDING - 1e-9 && answer.score <= high + ROUNDING + 1e-9 {
                    true => Ok(()),
                    false => Err(format!(
                        "its expected level is {}, which its probabilities can't give: they allow {low:.2} to {high:.2}",
                        answer.score
                    )),
                }
            }
            Answer::Other(_) => Ok(()),
        }
    }
}

/// How far a number the endpoint sends may be from the value behind it: it arrives rounded to two
/// decimal places, so by up to half a hundredth. Checks allow this much, and no more.
pub const ROUNDING: f64 = 0.005;

fn probability(what: &str, value: f64) -> std::result::Result<(), String> {
    match value.is_finite() && (0.0..=1.0).contains(&value) {
        true => Ok(()),
        false => Err(format!("{what} is {value}, which isn't a probability from 0 to 1")),
    }
}

/// Whether `values` could be a distribution rounded to [`ROUNDING`]: each could be as low as
/// `value − ROUNDING` or as high as `value + ROUNDING`, within 0 to 1, so the lowest they could
/// add up to must be at most 1 and the highest at least 1. `[0.51, 0.51, 0, 0]` fails: the two
/// 0.51s were at least 0.505 each.
fn distribution(values: &[(String, f64)]) -> std::result::Result<(), String> {
    for (what, value) in values {
        probability(what, *value)?;
    }
    let (low, high) =
        values.iter().fold((0.0, 0.0), |(low, high), (_, value)| (low + (value - ROUNDING).max(0.0), high + (value + ROUNDING).min(1.0)));
    let sum: f64 = values.iter().map(|(_, value)| value).sum();
    match low <= 1.0 + 1e-9 && high >= 1.0 - 1e-9 {
        true => Ok(()),
        false => Err(format!("its probabilities add up to {sum:.3}, which no distribution rounds to")),
    }
}

/// The lowest and highest expected level that `levels` (level, rounded probability) allow: each
/// probability anywhere within [`ROUNDING`] of its value, and all of them adding up to 1. The
/// lowest puts the mass left over after each level's minimum on the lowest levels first; the
/// highest, on the highest.
fn expectation_bounds(levels: &[(f64, f64)]) -> (f64, f64) {
    let bounds: Vec<(f64, f64, f64)> =
        levels.iter().map(|(level, value)| (*level, (value - ROUNDING).max(0.0), (value + ROUNDING).min(1.0))).collect();
    let floor: f64 = bounds.iter().map(|(level, low, _)| level * low).sum();
    let spare = (1.0 - bounds.iter().map(|(_, low, _)| low).sum::<f64>()).max(0.0);
    let fill = |order: &mut dyn Iterator<Item = &(f64, f64, f64)>| {
        let (mut left, mut total) = (spare, floor);
        for (level, low, high) in order {
            let add = (high - low).min(left);
            total += level * add;
            left -= add;
        }
        total
    };
    (fill(&mut bounds.iter()), fill(&mut bounds.iter().rev()))
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
///
/// Only a level written plainly (`"0"`, `"12"`) is read: `"00"` or `"+0"` would be level 0 as well,
/// and one would quietly overwrite the other's probability.
fn by_level<'de, D: Deserializer<'de>, T: Deserialize<'de>>(deserializer: D) -> std::result::Result<BTreeMap<u8, T>, D::Error> {
    let mut levels = BTreeMap::new();
    for (key, value) in BTreeMap::<String, T>::deserialize(deserializer)? {
        let level: u8 = key.parse().map_err(|_| D::Error::custom(format!("level {key:?} is not a number")))?;
        if level.to_string() != key {
            return Err(D::Error::custom(format!("level {key:?} isn't written as the number {level} is")));
        }
        if levels.insert(level, value).is_some() {
            return Err(D::Error::custom(format!("level {level} is there twice")));
        }
    }
    Ok(levels)
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn choice(probabilities: &[(&str, f64)], choice: &str) -> Answer {
        Answer::Choice(ChoiceAnswer {
            choice: choice.to_owned(),
            confidence: 0.5,
            probabilities: probabilities.iter().map(|(option, p)| ((*option).to_owned(), *p)).collect(),
        })
    }

    #[test]
    fn a_distribution_must_be_one_that_rounds_to_these_numbers() {
        assert!(choice(&[("a", 0.33), ("b", 0.33), ("c", 0.33)], "a").check().is_ok());
        // Each 0.51 was at least 0.505, and the zeros can't be less than 0: over 1 either way.
        let over = choice(&[("a", 0.51), ("b", 0.51), ("c", 0.0), ("d", 0.0)], "a").check().unwrap_err();
        assert!(over.contains("no distribution rounds to"), "{over}");
        // Two certainties among many zeros, which a flat allowance per option let through.
        let mut many: Vec<(String, f64)> = (0..200).map(|at| (format!("o{at}"), 0.0)).collect();
        many[0].1 = 1.0;
        many[1].1 = 1.0;
        let many: Vec<(&str, f64)> = many.iter().map(|(option, p)| (option.as_str(), *p)).collect();
        assert!(choice(&many, "o0").check().is_err());
    }

    #[test]
    fn the_other_fields_must_agree_with_the_probabilities() {
        let picked_less = choice(&[("a", 0.2), ("b", 0.8)], "a").check().unwrap_err();
        assert!(picked_less.contains("it chose `a` at 0.2"), "{picked_less}");
        assert!(choice(&[("a", 0.5), ("b", 0.5)], "b").check().is_ok());
        let Answer::Choice(mut sure) = choice(&[("a", 1.0)], "a") else { unreachable!() };
        sure.confidence = 1.5;
        assert!(Answer::Choice(sure).check().unwrap_err().contains("the confidence is 1.5"));
        let score = |score: f64| {
            Answer::Score(ScoreAnswer {
                score,
                confidence: 0.9,
                probabilities: [(0, 1.0), (1, 0.0), (2, 0.0)].into(),
                legend: BTreeMap::new(),
            })
        };
        assert!(score(0.0).check().is_ok());
        // All of it on level 0 can't expect level 2.
        assert!(score(2.0).check().unwrap_err().contains("its expected level is 2"));
    }

    #[test]
    fn a_reply_is_checked_against_its_questions() {
        let reply: DecisionResponse = serde_json::from_str(include_str!("../tests/fixtures/decision.json")).unwrap();
        let asked: BTreeMap<String, Question> = [
            ("is_urgent".to_owned(), Question::noul("?")),
            ("department".to_owned(), Question::choice("?", [("sales", ""), ("billing", ""), ("technical", "")])),
            ("frustration".to_owned(), Question::score("?", ["Calm", "Frustrated", "Very angry"])),
        ]
        .into();
        assert_eq!(reply.check_against(&asked), Ok(()));

        let mut fewer = asked.clone();
        fewer.insert("department".to_owned(), Question::choice("?", [("billing", ""), ("technical", "")]));
        let error = reply.check_against(&fewer).unwrap_err().to_string();
        assert!(error.contains("`sales`, which isn't one of the question's options"), "{error}");

        let mut shorter = asked.clone();
        shorter.insert("frustration".to_owned(), Question::score("?", ["Calm", "Frustrated"]));
        assert!(reply.check_against(&shorter).unwrap_err().to_string().contains("level 2, and the question's levels are 0 to 1"));

        let mut other = asked.clone();
        other.insert("is_urgent".to_owned(), Question::score("?", ["No", "Yes"]));
        assert_eq!(
            reply.check_against(&other),
            Err(Error::WrongType { id: "is_urgent".to_owned(), expected: "score", found: "noul".to_owned() })
        );

        let mut more = asked;
        more.insert("unasked".to_owned(), Question::noul("?"));
        assert_eq!(reply.check_against(&more), Err(Error::MissingAnswer("unasked".to_owned())));
    }

    #[test]
    fn a_level_is_written_one_way() {
        let read = |probabilities: serde_json::Value| {
            serde_json::from_value::<Answer>(json!({"type": "score", "score": 0.8, "confidence": 0.5, "probabilities": probabilities}))
        };
        assert!(read(json!({"0": 0.2, "1": 0.8})).is_ok());
        // "00" is level 0 too, and would quietly have replaced "0"'s probability.
        let error = read(json!({"0": 0.7, "00": 0.2, "1": 0.8})).unwrap_err().to_string();
        assert!(error.contains(r#"level "00" isn't written as the number 0 is"#), "{error}");
    }
}
