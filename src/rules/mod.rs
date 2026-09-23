//! Fuzzy rules over a reply: what `-r rules.toml` reads. A policy written over Jev's answers: each
//! term reads a probability (a Noul's probability of yes, one Score level's or one Choice option's)
//! and uses it as the degree to which the term holds. That is a modelling choice, not a fact about
//! the numbers: a probability of 0.8 that it is hot is not "hot to degree 0.8". The rules combine
//! the degrees with AND, OR, NOT and hedges, by fuzzy logic, into a support score per outcome, which
//! counts as a yes at or over a threshold. A support score is not a probability, and isn't
//! calibrated as one: the reply keeps the probabilities.
//!
//! ```toml
//! [logic]                    # optional
//! and = "min"                # min | product | lukasiewicz
//! or  = "max"                # max | probsum | bounded
//!
//! [decide]                   # optional
//! threshold = 0.5
//!
//! [terms]                    # the names rules use for the answers
//! hot     = "temp.Hot"       # a Score's level, by its text
//! stormy  = "sky.storm"      # a Choice's option
//! raining = "raining"        # a Noul, by its id alone
//!
//! [[rule]]
//! if     = "raining AND NOT SOMEWHAT hot"
//! then   = "raincoat"
//! weight = 1.0               # optional; the rule's score is multiplied by it
//! ```
//!
//! Operators are uppercase (`AND`, `OR`, `NOT`, and the hedges `VERY`, `SOMEWHAT`, `EXTREMELY`,
//! `INDEED`) and terms are lowercase, so neither can be taken for the other. Hedges and `NOT` bind
//! tightest, then `AND`, then `OR`, and parentheses group. Rules with the same `then` are joined by
//! the file's OR. `OUTPUT IS SET` always concludes in a declared `[output.OUTPUT]`.
//!
//! Before evaluating, every answer the terms read is checked: a probability outside 0 to 1, a
//! distribution that doesn't add up to 1, or a level or option the question doesn't have is an
//! error, never clamped into something that looks like an answer.
//!
//! A rules file is checked against the questions before anything is asked ([`Rules::parse`]), so a
//! term naming a level the question doesn't have costs no call.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{DecisionResponse, Options, Question};

mod graph;
mod output;
mod svg;

use output::Output;

/// How AND joins two degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum And {
    /// The smaller: the weakest condition decides, and repeating one doesn't lower it.
    #[default]
    Min,
    /// Their product: every weak condition lowers the result. Read as a probability, it assumes
    /// the events are independent, which answers about one state seldom are.
    Product,
    /// `max(0, a + b − 1)`: strict, high only when both are.
    Lukasiewicz,
}

impl And {
    /// How it is worked out, for the graph.
    fn describe(self) -> &'static str {
        match self {
            And::Min => "min",
            And::Product => "a × b",
            And::Lukasiewicz => "max(0, a + b − 1)",
        }
    }

    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            And::Min => a.min(b),
            And::Product => a * b,
            And::Lukasiewicz => (a + b - 1.0).max(0.0),
        }
    }
}

/// How OR joins two degrees, in a rule and between rules with the same `then`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Or {
    /// The larger: the strongest reason decides, and repeating one doesn't raise it.
    #[default]
    Max,
    /// `a + b − ab`: reasons reinforce each other. A reason written twice counts twice (0.6 becomes
    /// 0.84), so it suits distinct pieces of evidence, not restatements of one.
    Probsum,
    /// `min(1, a + b)`: reasons add up, to at most 1. For levels or options of one question, which
    /// exclude each other, this is their probabilities' sum: the chance that one of them holds.
    Bounded,
}

impl Or {
    /// How it is worked out, for the graph.
    fn describe(self) -> &'static str {
        match self {
            Or::Max => "max",
            Or::Probsum => "a + b − ab",
            Or::Bounded => "min(1, a + b)",
        }
    }

    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            Or::Max => a.max(b),
            Or::Probsum => a + b - a * b,
            Or::Bounded => (a + b).min(1.0),
        }
    }
}

/// The file's `[logic]`: which AND and which OR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logic {
    #[serde(default)]
    pub and: And,
    #[serde(default)]
    pub or: Or,
}

/// A word that reshapes a degree. It moves where the threshold falls (`VERY a` passes 0.5 only
/// when `a` is at least 0.71), and nothing more: it doesn't make "angry" mean "very angry", or add
/// evidence. A stronger meaning needs its own level or question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hedge {
    /// `a²`: only a strong answer stays strong.
    Very,
    /// `√a`: a weak answer still counts.
    Somewhat,
    /// `a³`.
    Extremely,
    /// Pushes a degree away from 0.5, toward 0 or 1. It makes no answer more certain.
    Indeed,
}

impl Hedge {
    fn named(word: &str) -> Option<Hedge> {
        Some(match word {
            "VERY" => Hedge::Very,
            "SOMEWHAT" => Hedge::Somewhat,
            "EXTREMELY" => Hedge::Extremely,
            "INDEED" => Hedge::Indeed,
            _ => return None,
        })
    }

    /// Its name and how it is worked out, for the graph.
    fn describe(self) -> (&'static str, &'static str) {
        match self {
            Hedge::Very => ("VERY", "x²"),
            Hedge::Somewhat => ("SOMEWHAT", "√x"),
            Hedge::Extremely => ("EXTREMELY", "x³"),
            Hedge::Indeed => ("INDEED", "toward 0 or 1"),
        }
    }

    fn apply(self, a: f64) -> f64 {
        match self {
            Hedge::Very => a * a,
            Hedge::Somewhat => a.sqrt(),
            Hedge::Extremely => a * a * a,
            Hedge::Indeed if a <= 0.5 => 2.0 * a * a,
            Hedge::Indeed => 1.0 - 2.0 * (1.0 - a) * (1.0 - a),
        }
    }
}

/// Every operator, for the messages that name them.
const OPERATORS: &str = "AND, OR, NOT, VERY, SOMEWHAT, EXTREMELY, INDEED";

/// A rule's `if`, parsed.
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Term(String),
    Not(Box<Expr>),
    Hedge(Hedge, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// The degree to which it holds, with each term's from `degree`.
    fn eval(&self, logic: Logic, degree: &mut impl FnMut(&str) -> crate::Result<f64>) -> crate::Result<f64> {
        Ok(match self {
            Expr::Term(term) => degree(term)?,
            Expr::Not(inner) => 1.0 - inner.eval(logic, degree)?,
            Expr::Hedge(hedge, inner) => hedge.apply(inner.eval(logic, degree)?),
            Expr::And(a, b) => logic.and.apply(a.eval(logic, degree)?, b.eval(logic, degree)?),
            Expr::Or(a, b) => logic.or.apply(a.eval(logic, degree)?, b.eval(logic, degree)?),
        })
    }

    /// The terms it uses, in the order written.
    fn terms<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::Term(term) => out.push(term),
            Expr::Not(inner) | Expr::Hedge(_, inner) => inner.terms(out),
            Expr::And(a, b) | Expr::Or(a, b) => {
                a.terms(out);
                b.terms(out);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Open,
    Close,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    for character in text.chars() {
        if character.is_whitespace() || character == '(' || character == ')' {
            if !word.is_empty() {
                tokens.push(Token::Word(std::mem::take(&mut word)));
            }
            match character {
                '(' => tokens.push(Token::Open),
                ')' => tokens.push(Token::Close),
                _ => {}
            }
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        tokens.push(Token::Word(word));
    }
    tokens
}

/// How many words and brackets one `if` may have.
const MOST_TOKENS: usize = 256;

/// A recursive descent over the tokens: OR of ANDs of hedged or negated terms and groups.
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn parse(text: &str) -> Result<Expr, String> {
        let mut parser = Parser { tokens: tokens(text), at: 0 };
        if parser.tokens.is_empty() {
            return Err("the `if` is empty".to_owned());
        }
        // Parsing and evaluating recurse once per level of the tree, and a tree is never deeper
        // than its words and brackets are many. Bounding them keeps a rule from a file or a model
        // from running the stack out, which would end the process rather than return an error.
        if parser.tokens.len() > MOST_TOKENS {
            return Err(format!(
                "it is {} words and brackets long, over the {MOST_TOKENS} a rule may have; split it into several rules with the same `then`",
                parser.tokens.len()
            ));
        }
        let expr = parser.or()?;
        match parser.tokens.get(parser.at) {
            None => Ok(expr),
            Some(Token::Close) => Err("a `)` with no `(` before it".to_owned()),
            Some(Token::Open) => Err("a `(` where AND or OR should be".to_owned()),
            Some(Token::Word(word)) if word.chars().any(|character| character.is_uppercase()) => {
                Err(format!("`{word}` isn't an operator: operators are {OPERATORS}"))
            }
            Some(Token::Word(word)) => Err(format!("`{word}` where AND or OR should be")),
        }
    }

    fn eat(&mut self, keyword: &str) -> bool {
        let found = matches!(self.tokens.get(self.at), Some(Token::Word(word)) if word == keyword);
        if found {
            self.at += 1;
        }
        found
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut left = self.and()?;
        while self.eat("OR") {
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        while self.eat("AND") {
            left = Expr::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("NOT") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if let Some(Token::Word(word)) = self.tokens.get(self.at) {
            if let Some(hedge) = Hedge::named(word) {
                self.at += 1;
                return Ok(Expr::Hedge(hedge, Box::new(self.unary()?)));
            }
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr, String> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        match token {
            None => Err("it ends where a term should be".to_owned()),
            Some(Token::Close) => Err("a `)` where a term should be".to_owned()),
            Some(Token::Open) => {
                let inner = self.or()?;
                if self.tokens.get(self.at) != Some(&Token::Close) {
                    return Err("a `(` that isn't closed".to_owned());
                }
                self.at += 1;
                Ok(inner)
            }
            Some(Token::Word(word)) if word == "AND" || word == "OR" => Err(format!("`{word}` where a term should be")),
            Some(Token::Word(word)) if is_term_name(&word) => Ok(Expr::Term(word)),
            Some(Token::Word(word)) => Err(format!(
                "`{word}` is neither an operator nor a term: operators are uppercase ({OPERATORS}), and terms are \
                 lowercase names from [terms]"
            )),
        }
    }
}

/// Whether `name` can be a term: a lowercase word of letters, digits and `_`, so that it can never
/// be taken for an operator.
fn is_term_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && characters.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_')
}

/// What a term reads from the reply: one level of a Score, one option of a Choice, or a Noul's yes.
/// It keeps every label of its question as well, which the graph draws.
#[derive(Debug, Clone, PartialEq)]
struct Target {
    id: String,
    kind: Kind,
    /// The Score's levels, the Choice's options, or `no` and `yes` for a Noul.
    labels: Vec<String>,
    /// Which of `labels` the term is.
    selected: usize,
}

/// Which type of question a term reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Noul,
    Score,
    Choice,
}

impl Target {
    /// Which question `target` names in `questions`, and what of it. `target` is the question id
    /// alone for a Noul, or the id, a `.`, and a level's text or an option's name.
    fn resolve(target: &str, questions: &BTreeMap<&str, &Question>) -> Result<Target, String> {
        let target = target.trim();
        if let Some(question) = questions.get(target) {
            return match question {
                Question::Noul { .. } => {
                    Ok(Target { id: target.to_owned(), kind: Kind::Noul, labels: vec!["no".to_owned(), "yes".to_owned()], selected: 1 })
                }
                Question::Score { criteria, .. } => {
                    Err(format!("`{target}` is a score: name one of its levels, as {}", listed(target, &level_names(criteria))))
                }
                Question::Choice { criteria, .. } => {
                    Err(format!("`{target}` is a choice: name one of its options, as {}", listed(target, &option_names(criteria))))
                }
            };
        }
        // An id may have a `.` of its own, so each `.` is tried until one ends a question's id: the
        // last first, so that with both `weather` and `weather.temp` asked, `weather.temp.Hot` is
        // the level of `weather.temp`.
        for (at, _) in target.rmatch_indices('.') {
            let (id, name) = (&target[..at], &target[at + 1..]);
            let Some(question) = questions.get(id) else { continue };
            return match question {
                Question::Noul { .. } => Err(format!("`{id}` is a noul, which has no levels or options: write `{id}` alone")),
                Question::Score { criteria, .. } => {
                    // Levels are numbered in a reply as u8s, so a level past 255 can't be read back.
                    match criteria.iter().position(|level| level.as_str() == Some(name)).filter(|at| u8::try_from(*at).is_ok()) {
                        Some(selected) => Ok(Target { id: id.to_owned(), kind: Kind::Score, labels: level_labels(criteria), selected }),
                        None => Err(format!("`{id}` has no level `{name}`; its levels are {}", listed(id, &level_names(criteria)))),
                    }
                }
                Question::Choice { criteria, .. } => {
                    let options = option_names(criteria);
                    match options.iter().position(|option| option == name) {
                        Some(selected) => Ok(Target { id: id.to_owned(), kind: Kind::Choice, labels: options, selected }),
                        None => Err(format!("`{id}` has no option `{name}`; its options are {}", listed(id, &options))),
                    }
                }
            };
        }
        let asked: Vec<&str> = questions.keys().copied().collect();
        Err(format!("no question `{}`; the questions are {}", target.split('.').next().unwrap_or(target), asked.join(", ")))
    }

    /// The degree the reply gives it. An answer missing from the reply, or of another type, is an
    /// error rather than a zero: a zero would read as a confident no.
    fn degree(&self, reply: &DecisionResponse) -> crate::Result<f64> {
        Ok(match self.kind {
            Kind::Noul => reply.noul(&self.id)?,
            Kind::Score => {
                let level = u8::try_from(self.selected).unwrap_or(u8::MAX);
                reply.score(&self.id)?.probabilities.get(&level).copied().ok_or_else(|| self.missing())?
            }
            Kind::Choice => {
                reply.choice(&self.id)?.probabilities.get(&self.labels[self.selected]).copied().ok_or_else(|| self.missing())?
            }
        })
    }
}

impl Target {
    /// Whether the reply's answer to this term's question is one it can read: of the right type,
    /// its numbers probabilities ([`crate::Answer::check`]), and no level or option the question
    /// doesn't have.
    fn check(&self, reply: &DecisionResponse) -> crate::Result<()> {
        let bad = |why: String| crate::Error::BadAnswer { id: self.id.clone(), why };
        match self.kind {
            Kind::Noul => {
                reply.noul(&self.id)?;
            }
            Kind::Score => {
                if let Some(level) = reply.score(&self.id)?.probabilities.keys().find(|level| usize::from(**level) >= self.labels.len()) {
                    return Err(bad(format!(
                        "it gives a probability for level {level}, and the question's levels are 0 to {}",
                        self.labels.len() - 1
                    )));
                }
            }
            Kind::Choice => {
                if let Some(option) = reply.choice(&self.id)?.probabilities.keys().find(|option| !self.labels.contains(option)) {
                    return Err(bad(format!("it gives a probability for `{option}`, which isn't one of the question's options")));
                }
            }
        }
        reply.answer(&self.id)?.check().map_err(bad)
    }

    /// The error for a reply whose answer gives no probability for this term's level or option.
    fn missing(&self) -> crate::Error {
        crate::Error::MissingProbability { id: self.id.clone(), label: self.labels[self.selected].clone() }
    }
}

/// A Score's levels as the graph labels them: the text, or the number of a structured level.
fn level_labels(levels: &[Value]) -> Vec<String> {
    levels
        .iter()
        .enumerate()
        .map(|(at, level)| match level {
            Value::String(text) => text.clone(),
            _ => format!("level {at}"),
        })
        .collect()
}

/// A Score's levels, as a term names them. A structured level has no text to be named by.
fn level_names(levels: &[Value]) -> Vec<String> {
    levels
        .iter()
        .enumerate()
        .map(|(at, level)| match level {
            Value::String(text) => text.clone(),
            _ => format!("(level {at} is structured, so no term can name it)"),
        })
        .collect()
}

fn option_names(Options(options): &Options) -> Vec<String> {
    options.iter().map(|(name, _)| name.clone()).collect()
}

/// `id.a`, `id.b`, … for a message.
fn listed(id: &str, names: &[String]) -> String {
    names.iter().map(|name| if name.starts_with('(') { name.clone() } else { format!("`{id}.{name}`") }).collect::<Vec<_>>().join(", ")
}

/// A rules file, as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    logic: Logic,
    #[serde(default)]
    decide: Decide,
    #[serde(default)]
    terms: BTreeMap<String, String>,
    #[serde(default, rename = "output")]
    outputs: BTreeMap<String, output::OutputFile>,
    #[serde(default, rename = "rule")]
    rules: Vec<RuleFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decide {
    #[serde(default = "Decide::half")]
    threshold: f64,
}

impl Decide {
    fn half() -> f64 {
        0.5
    }
}

impl Default for Decide {
    fn default() -> Decide {
        Decide { threshold: Decide::half() }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    #[serde(rename = "if")]
    when: String,
    then: String,
    #[serde(default = "RuleFile::full")]
    weight: f64,
}

impl RuleFile {
    fn full() -> f64 {
        1.0
    }
}

/// One rule, checked.
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    /// The `if` as written, which the outcome shows.
    text: String,
    when: Expr,
    then: Then,
    weight: f64,
}

/// What a rule concludes: an item, scored and then decided at the threshold, or one fuzzy set of an
/// output, clipped at the rule's score and merged into a crisp value.
#[derive(Debug, Clone, PartialEq)]
enum Then {
    Item(String),
    /// `OUTPUT IS SET`, by position in [`Rules::outputs`] and in that output's sets.
    Output {
        output: usize,
        set: usize,
    },
}

/// A rules file, parsed and checked against the questions it will read.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    logic: Logic,
    threshold: f64,
    terms: BTreeMap<String, Target>,
    outputs: Vec<Output>,
    rules: Vec<Rule>,
}

impl Rules {
    /// The rules in `text` (TOML), for a reply to `questions`. Every term is checked against the
    /// questions here, before anything is asked; the error says what is wrong and where.
    pub fn parse<'a>(text: &str, questions: impl IntoIterator<Item = (&'a str, &'a Question)>) -> Result<Rules, String> {
        let file: File = toml::from_str(text).map_err(|error| error.to_string().trim_end().to_owned())?;
        if !(0.0..=1.0).contains(&file.decide.threshold) {
            return Err(format!("[decide] threshold is {}; it goes from 0 to 1", file.decide.threshold));
        }
        let questions: BTreeMap<&str, &Question> = questions.into_iter().collect();
        let mut terms = BTreeMap::new();
        for (name, target) in &file.terms {
            if !is_term_name(name) {
                return Err(format!(
                    "[terms] `{name}`: a term is a lowercase word of letters, digits and `_`, so it can't be taken for an \
                     operator ({OPERATORS})"
                ));
            }
            terms.insert(name.clone(), Target::resolve(target, &questions).map_err(|why| format!("[terms] {name}: {why}"))?);
        }
        let mut outputs = Vec::with_capacity(file.outputs.len());
        for (name, output) in file.outputs {
            outputs.push(Output::parse(name, output)?);
        }
        if file.rules.is_empty() {
            return Err("no rules: add a [[rule]] with an `if` and a `then`".to_owned());
        }
        let mut rules = Vec::with_capacity(file.rules.len());
        for (at, rule) in file.rules.into_iter().enumerate() {
            let text = rule.when.trim().to_owned();
            let wrong = |why: &str| format!("rule {} (`{text}`): {why}", at + 1);
            // An operator written in lowercase reads as a term, and the parser would then trip over
            // whatever follows it; saying what it is helps more than where it tripped.
            for token in tokens(&text) {
                let Token::Word(word) = token else { continue };
                let upper = word.to_ascii_uppercase();
                let what = match upper.as_str() {
                    "AND" | "OR" | "NOT" => "operator",
                    _ if Hedge::named(&upper).is_some() => "hedge",
                    _ => continue,
                };
                if word != upper && !terms.contains_key(&word) {
                    return Err(wrong(&format!("`{word}` isn't in [terms]; the {what} is written {upper}")));
                }
            }
            let when = Parser::parse(&text).map_err(|why| wrong(&why))?;
            let mut used = Vec::new();
            when.terms(&mut used);
            if let Some(missing) = used.iter().find(|term| !terms.contains_key(**term)) {
                let named = match terms.is_empty() {
                    true => "none".to_owned(),
                    false => terms.keys().cloned().collect::<Vec<_>>().join(", "),
                };
                return Err(wrong(&format!("`{missing}` isn't in [terms], which names {named}")));
            }
            let then = Then::parse(&rule.then, &outputs).map_err(|why| wrong(&why))?;
            if !(0.0..=1.0).contains(&rule.weight) {
                return Err(wrong(&format!("the weight is {}; it goes from 0 to 1", rule.weight)));
            }
            rules.push(Rule { text, when, then, weight: rule.weight });
        }
        Ok(Rules { logic: file.logic, threshold: file.decide.threshold, terms, outputs, rules })
    }

    /// The outcome `reply` gives: every `then`, in the order the file first names it, with its
    /// score and the rules behind it.
    pub fn evaluate(&self, reply: &DecisionResponse) -> crate::Result<Outcome> {
        self.check(reply)?;
        let scores = self.scores(reply)?;
        let mut items: Vec<Item> = Vec::new();
        let mut outputs: Vec<OutputValue> = self
            .outputs
            .iter()
            .map(|output| OutputValue {
                output: output.name.clone(),
                value: None,
                sets: output.sets.iter().map(|set| SetScore { set: set.name.clone(), score: 0.0, rules: Vec::new() }).collect(),
            })
            .collect();
        for (rule, &score) in self.rules.iter().zip(&scores) {
            let fired = Fired { when: rule.text.clone(), weight: rule.weight, score };
            match &rule.then {
                Then::Item(name) => match items.iter_mut().find(|item| &item.item == name) {
                    Some(item) => {
                        item.score = self.logic.or.apply(item.score, score);
                        item.rules.push(fired);
                    }
                    None => items.push(Item { item: name.clone(), score, yes: false, rules: vec![fired] }),
                },
                Then::Output { output, set } => {
                    let set = &mut outputs[*output].sets[*set];
                    set.score = self.logic.or.apply(set.score, score);
                    set.rules.push(fired);
                }
            }
        }
        for item in &mut items {
            item.yes = item.score >= self.threshold;
        }
        for (at, value) in outputs.iter_mut().enumerate() {
            value.value = self.outputs[at].centroid(&self.set_scores(at, &scores), self.logic.or);
        }
        Ok(Outcome { threshold: self.threshold, items, outputs })
    }

    /// Every answer the terms read, checked once per question.
    fn check(&self, reply: &DecisionResponse) -> crate::Result<()> {
        let mut checked: Vec<&str> = Vec::new();
        for target in self.terms.values() {
            if !checked.contains(&target.id.as_str()) {
                target.check(reply)?;
                checked.push(&target.id);
            }
        }
        Ok(())
    }

    /// Each rule's score for `reply`: what its `if` came to, times its weight.
    fn scores(&self, reply: &DecisionResponse) -> crate::Result<Vec<f64>> {
        let mut degree = |term: &str| self.terms[term].degree(reply);
        self.rules.iter().map(|rule| Ok(rule.when.eval(self.logic, &mut degree)? * rule.weight)).collect()
    }

    /// Every set of output `at`, with the score it is clipped at: its rules' scores joined by the
    /// file's OR, and 0 for a set no rule concludes. Each set is clipped once, at this score, so the
    /// score an outcome reports, the shape drawn and the value taken from it are the same thing.
    fn set_scores(&self, at: usize, scores: &[f64]) -> Vec<f64> {
        let mut sets = vec![0.0; self.outputs[at].sets.len()];
        for (rule, &score) in self.rules.iter().zip(scores) {
            if let Then::Output { output, set } = rule.then {
                if output == at {
                    sets[set] = self.logic.or.apply(sets[set], score);
                }
            }
        }
        sets
    }
}

/// What the rules made of a reply.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    /// The score at or over which an item is a yes.
    pub threshold: f64,
    /// Every item a `then` names, in the order the file first names it.
    pub items: Vec<Item>,
    /// Every `[output]`, with the crisp value its sets' clipped shapes merge into.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<OutputValue>,
}

/// An output's crisp value: the centroid of its sets, each clipped at the score its rules give it.
/// The value says where the support lies, not how much there is: a set clipped at 0.01 alone gives
/// the same value as one clipped at 1, so read the sets' scores before acting on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutputValue {
    pub output: String,
    /// `None` when no rule concluding it scored above zero, so there is no shape to take a centre of.
    pub value: Option<f64>,
    /// Every set, in order along the output's range.
    pub sets: Vec<SetScore>,
}

/// One set of an output: the score its rules clip it at (joined by the file's OR), and those rules.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetScore {
    pub set: String,
    pub score: f64,
    pub rules: Vec<Fired>,
}

/// One `then`: its score, whether that reaches the threshold, and the rules that gave it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub item: String,
    pub score: f64,
    pub yes: bool,
    pub rules: Vec<Fired>,
}

/// One rule's part in an item's score.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fired {
    /// The rule's `if`, as written.
    #[serde(rename = "if")]
    pub when: String,
    pub weight: f64,
    /// What the `if` came to, times the weight.
    pub score: f64,
}

impl Outcome {
    /// One line per item, `name  score  yes`, then the threshold, then a line per output with its
    /// value and each set's score. Ends with a newline.
    pub fn text(&self) -> String {
        let names = self.items.iter().map(|item| &item.item).chain(self.outputs.iter().map(|output| &output.output));
        let width = names.map(|name| name.chars().count()).max().unwrap_or(0);
        let mut out = String::new();
        for item in &self.items {
            let yes = if item.yes { "  yes" } else { "" };
            let _ = writeln!(out, "{:width$}  {:.2}{yes}", item.item, item.score);
        }
        if !self.items.is_empty() {
            let _ = writeln!(out, "threshold {:.2}", self.threshold);
        }
        for output in &self.outputs {
            let _ = writeln!(out, "{:width$}  {}  ({})", output.output, output.value_text(), output.sets_text());
        }
        out
    }
}

impl OutputValue {
    /// The value to two places, or `-` when no rule fired.
    pub fn value_text(&self) -> String {
        self.value.map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"))
    }

    /// `short 0.00, medium 0.80, long 0.10`.
    pub fn sets_text(&self) -> String {
        self.sets.iter().map(|set| format!("{} {:.2}", set.set, set.score)).collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn questions() -> Vec<(String, Question)> {
        vec![
            ("temp".to_owned(), Question::score("How warm is it?", ["Cold", "Mild", "Hot"])),
            ("humidity".to_owned(), Question::score("How humid is it?", ["Dry", "Normal", "Humid"])),
            ("raining".to_owned(), Question::noul("Is it raining?")),
            ("sky".to_owned(), Question::choice("What is the sky like?", [("clear", ""), ("cloudy", ""), ("storm", "")])),
        ]
    }

    /// The weather from the examples: mostly mild, fairly humid, probably raining.
    fn reply() -> DecisionResponse {
        serde_json::from_value(json!({
            "model": "typesafe/jev-1.13-20260917",
            "answers": {
                "temp": {"type": "score", "score": 1.1, "confidence": 0.7,
                         "probabilities": {"0": 0.1, "1": 0.7, "2": 0.2}, "legend": {"0": "Cold", "1": "Mild", "2": "Hot"}},
                "humidity": {"type": "score", "score": 1.5, "confidence": 0.6,
                             "probabilities": {"0": 0.1, "1": 0.3, "2": 0.6}, "legend": {"0": "Dry", "1": "Normal", "2": "Humid"}},
                "raining": {"type": "noul", "noul": 0.8},
                "sky": {"type": "choice", "choice": "cloudy", "confidence": 0.5,
                        "probabilities": {"clear": 0.1, "cloudy": 0.6, "storm": 0.3}},
            },
            "usage": {"input_tokens": 400, "output_tokens": 70},
        }))
        .unwrap()
    }

    const TERMS: &str = r#"
        [terms]
        cold = "temp.Cold"
        mild = "temp.Mild"
        hot = "temp.Hot"
        humid = "humidity.Humid"
        raining = "raining"
        storm = "sky.storm"
    "#;

    fn rules(rules: &str) -> Result<Rules, String> {
        let questions = questions();
        Rules::parse(&format!("{TERMS}\n{rules}"), questions.iter().map(|(id, question)| (id.as_str(), question)))
    }

    /// The score `if` gives, under `logic`.
    fn score(logic: &str, when: &str) -> f64 {
        let rules = rules(&format!("{logic}\n[[rule]]\nif = \"{when}\"\nthen = \"x\"")).unwrap();
        rules.evaluate(&reply()).unwrap().items[0].score
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn reads_each_kind_of_term() {
        assert!(close(score("", "mild"), 0.7));
        assert!(close(score("", "raining"), 0.8));
        assert!(close(score("", "storm"), 0.3));
    }

    #[test]
    fn combines_with_min_max_and_one_minus() {
        assert!(close(score("", "raining AND humid"), 0.6));
        assert!(close(score("", "raining OR humid"), 0.8));
        assert!(close(score("", "NOT raining"), 0.2));
        assert!(close(score("", "raining AND NOT hot"), 0.8));
    }

    #[test]
    fn takes_the_files_and_and_or() {
        assert!(close(score("[logic]\nand = \"product\"", "raining AND humid"), 0.48));
        assert!(close(score("[logic]\nand = \"lukasiewicz\"", "raining AND humid"), 0.4));
        assert!(close(score("[logic]\nor = \"probsum\"", "raining OR humid"), 0.92));
        assert!(close(score("[logic]\nor = \"bounded\"", "raining OR humid"), 1.0));
    }

    #[test]
    fn applies_hedges() {
        assert!(close(score("", "VERY mild"), 0.49));
        assert!(close(score("", "EXTREMELY mild"), 0.343));
        assert!(close(score("", "SOMEWHAT mild"), 0.7f64.sqrt()));
        assert!(close(score("", "INDEED mild"), 0.82));
        assert!(close(score("", "INDEED cold"), 0.02));
        // A hedge or NOT binds to the term after it, not to the rest of the rule.
        assert!(close(score("", "NOT VERY mild"), 0.51));
        assert!(close(score("", "VERY mild AND raining"), 0.49));
    }

    #[test]
    fn and_binds_tighter_than_or_and_parentheses_group() {
        // cold OR (raining AND humid) = max(0.1, 0.6)
        assert!(close(score("", "cold OR raining AND humid"), 0.6));
        // (cold OR raining) AND humid = min(0.8, 0.6), and (cold OR hot) AND raining = min(0.2, 0.8)
        assert!(close(score("", "(cold OR raining) AND humid"), 0.6));
        assert!(close(score("", "(cold OR hot) AND raining"), 0.2));
        assert!(close(score("", "VERY (raining AND humid)"), 0.36));
    }

    #[test]
    fn weights_rules_and_joins_the_same_then_by_or() {
        let rules = rules(
            r#"
            [[rule]]
            if = "cold"
            then = "coat"

            [[rule]]
            if = "raining AND NOT hot"
            then = "raincoat"

            [[rule]]
            if = "raining AND humid"
            then = "coat"
            weight = 0.5
            "#,
        )
        .unwrap();
        let outcome = rules.evaluate(&reply()).unwrap();
        // In the order the file first names them.
        let items: Vec<(&str, f64, bool)> = outcome.items.iter().map(|item| (item.item.as_str(), item.score, item.yes)).collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "coat");
        // max(0.1, 0.6 × 0.5)
        assert!(close(items[0].1, 0.3));
        assert!(!items[0].2);
        assert_eq!(items[1].0, "raincoat");
        assert!(close(items[1].1, 0.8) && items[1].2);
        assert_eq!(outcome.items[0].rules.len(), 2);
        assert_eq!(outcome.items[0].rules[1].when, "raining AND humid");
        assert_eq!(outcome.text(), "coat      0.30\nraincoat  0.80  yes\nthreshold 0.50\n");
    }

    #[test]
    fn decides_at_the_files_threshold() {
        let outcome =
            rules("[decide]\nthreshold = 0.25\n[[rule]]\nif = \"storm\"\nthen = \"stay in\"").unwrap().evaluate(&reply()).unwrap();
        assert!(outcome.items[0].yes);
        assert_eq!(outcome.threshold, 0.25);
    }

    #[test]
    fn prints_the_outcome_as_json() {
        let outcome = rules("[[rule]]\nif = \"raining\"\nthen = \"umbrella\"").unwrap().evaluate(&reply()).unwrap();
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            json!({"threshold": 0.5, "items": [
                {"item": "umbrella", "score": 0.8, "yes": true, "rules": [{"if": "raining", "weight": 1.0, "score": 0.8}]}
            ]})
        );
    }

    fn error(rules_text: &str) -> String {
        rules(rules_text).unwrap_err()
    }

    #[test]
    fn says_what_is_wrong_with_a_rule() {
        let rule = |when: &str| error(&format!("[[rule]]\nif = \"{when}\"\nthen = \"x\""));
        assert!(rule("").contains("empty"), "{}", rule(""));
        assert!(rule("raining AND").contains("ends where a term should be"), "{}", rule("raining AND"));
        assert!(rule("(raining AND hot").contains("isn't closed"));
        assert!(rule("raining)").contains("`)` with no `(`"));
        assert!(rule("raining hot").contains("`hot` where AND or OR should be"));
        assert!(rule("raining XOR hot").contains("`XOR` isn't an operator: operators are AND, OR"));
        assert!(rule("Hot").contains("`Hot` is neither"));
        assert!(rule("AND hot").contains("`AND` where a term should be"));
        // A term the file doesn't define, and the likeliest reason for one.
        let wet = rule("wet");
        assert!(wet.contains("rule 1 (`wet`): `wet` isn't in [terms]") && wet.contains("raining"), "{wet}");
        assert!(rule("raining and hot").contains("the operator is written AND"));
        assert!(rule("very hot").contains("the hedge is written VERY"));
    }

    #[test]
    fn says_what_is_wrong_with_the_file() {
        assert!(error("").contains("no rules"));
        assert!(error("[[rule]]\nif = \"hot\"\nthen = \" \"").contains("`then` is empty"));
        assert!(error("[[rule]]\nif = \"hot\"\nthen = \"x\"\nweight = 2.0").contains("weight is 2"));
        assert!(error("[decide]\nthreshold = 1.5\n[[rule]]\nif = \"hot\"\nthen = \"x\"").contains("threshold is 1.5"));
        assert!(error("[logic]\nand = \"average\"\n[[rule]]\nif = \"hot\"\nthen = \"x\"").contains("average"));
        assert!(error("[[rule]]\nif = \"hot\"\nthen = \"x\"\nelse = \"y\"").contains("else"));
    }

    #[test]
    fn checks_every_term_against_the_questions() {
        let questions = questions();
        let parse = |terms: &str| {
            Rules::parse(&format!("[terms]\n{terms}\n[[rule]]\nif = \"t\"\nthen = \"x\""), questions.iter().map(|(id, q)| (id.as_str(), q)))
        };
        assert!(parse("t = \"temp.Hot\"").is_ok());
        // Exact: the level's own text.
        let hot = parse("t = \"temp.hot\"").unwrap_err();
        assert!(hot.contains("[terms] t: `temp` has no level `hot`; its levels are `temp.Cold`, `temp.Mild`, `temp.Hot`"), "{hot}");
        assert!(parse("t = \"sky.Storm\"").unwrap_err().contains("its options are `sky.clear`, `sky.cloudy`, `sky.storm`"));
        assert!(parse("t = \"temp\"").unwrap_err().contains("`temp` is a score: name one of its levels"));
        assert!(parse("t = \"sky\"").unwrap_err().contains("`sky` is a choice"));
        assert!(parse("t = \"raining.yes\"").unwrap_err().contains("write `raining` alone"));
        assert!(parse("t = \"wind.Strong\"").unwrap_err().contains("no question `wind`; the questions are"));
        // Checked even when no rule uses it: a typo there is still a typo.
        let unused = Rules::parse(
            "[terms]\nt = \"temp.Hot\"\nu = \"temp.Warm\"\n[[rule]]\nif = \"t\"\nthen = \"x\"",
            questions.iter().map(|(id, q)| (id.as_str(), q)),
        );
        assert!(unused.unwrap_err().contains("[terms] u"));
        assert!(parse("T = \"temp.Hot\"").unwrap_err().contains("[terms] `T`: a term is a lowercase word"));
    }

    #[test]
    fn a_question_id_may_have_a_dot() {
        let questions = [("weather.temp".to_owned(), Question::score("How warm?", ["Cold", "Hot"]))];
        let parsed = Rules::parse(
            "[terms]\nhot = \"weather.temp.Hot\"\n[[rule]]\nif = \"hot\"\nthen = \"x\"",
            questions.iter().map(|(id, q)| (id.as_str(), q)),
        );
        let hot = &parsed.unwrap().terms["hot"];
        assert_eq!((hot.id.as_str(), hot.kind, hot.selected), ("weather.temp", Kind::Score, 1));
    }

    #[test]
    fn a_longer_question_id_wins_over_its_prefix() {
        let questions = [
            ("weather".to_owned(), Question::score("How is it?", ["Fine", "Bad"])),
            ("weather.temp".to_owned(), Question::score("How warm?", ["Cold", "Hot"])),
        ];
        let parsed = Rules::parse(
            "[terms]\nhot = \"weather.temp.Hot\"\nbad = \"weather.Bad\"\n[[rule]]\nif = \"hot OR bad\"\nthen = \"x\"",
            questions.iter().map(|(id, q)| (id.as_str(), q)),
        )
        .unwrap();
        let (hot, bad) = (&parsed.terms["hot"], &parsed.terms["bad"]);
        assert_eq!((hot.id.as_str(), hot.selected), ("weather.temp", 1));
        assert_eq!((bad.id.as_str(), bad.selected), ("weather", 1));
    }

    #[test]
    fn refuses_a_rule_too_deep_to_evaluate() {
        // Deep enough to overflow the stack if it were parsed: it must be refused before that.
        let deep = format!("{}raining{}", "NOT (".repeat(100_000), ")".repeat(100_000));
        let long = rule_error(&deep);
        assert!(long.contains("over the 256 a rule may have"), "{long}");
        let chain = vec!["raining"; 200].join(" AND ");
        assert!(rule_error(&chain).contains("over the 256"));
        // And a rule of a sensible length is fine.
        assert!(rules(&format!("[[rule]]\nif = \"{}\"\nthen = \"x\"", vec!["raining"; 100].join(" AND "))).is_ok());
    }

    fn rule_error(when: &str) -> String {
        error(&format!("[[rule]]\nif = \"{when}\"\nthen = \"x\""))
    }

    #[test]
    fn a_missing_probability_is_an_error_not_a_zero() {
        let rules = rules("[[rule]]\nif = \"hot OR storm\"\nthen = \"x\"").unwrap();
        let mut level_gone = reply();
        let crate::Answer::Score(temp) = level_gone.answers.get_mut("temp").unwrap() else { unreachable!() };
        // Its share moved to another level, so that the rest still adds up to 1.
        temp.probabilities.remove(&2);
        temp.probabilities.insert(1, 0.9);
        assert_eq!(rules.evaluate(&level_gone), Err(crate::Error::MissingProbability { id: "temp".to_owned(), label: "Hot".to_owned() }));
        let mut option_gone = reply();
        let crate::Answer::Choice(sky) = option_gone.answers.get_mut("sky").unwrap() else { unreachable!() };
        sky.probabilities.remove("storm");
        sky.probabilities.insert("cloudy".to_owned(), 0.9);
        assert_eq!(rules.evaluate(&option_gone).unwrap_err().to_string(), "question `sky` gives no probability for `storm`");
    }

    #[test]
    fn an_answer_that_isnt_probabilities_is_an_error_not_clamped() {
        let rules = rules("[[rule]]\nif = \"NOT raining OR hot OR storm\"\nthen = \"x\"").unwrap();
        let why = |change: &dyn Fn(&mut DecisionResponse)| {
            let mut reply = reply();
            change(&mut reply);
            match rules.evaluate(&reply) {
                Err(error @ crate::Error::BadAnswer { .. }) => error.to_string(),
                other => panic!("{other:?}"),
            }
        };
        let noul = |value: f64| {
            move |reply: &mut DecisionResponse| {
                reply.answers.insert("raining".to_owned(), crate::Answer::Noul(crate::NoulAnswer { noul: value })).map(drop).unwrap_or(())
            }
        };
        // 1.2 would have read as a yes of 1.2, and NOT it as −0.2.
        assert_eq!(
            why(&noul(1.2)),
            "the answer to question `raining` can't be read: the probability of yes is 1.2, which isn't a probability from 0 to 1"
        );
        assert!(why(&noul(f64::NAN)).contains("NaN"));
        let temp = |levels: [(u8, f64); 3]| {
            move |reply: &mut DecisionResponse| {
                let crate::Answer::Score(temp) = reply.answers.get_mut("temp").unwrap() else { unreachable!() };
                temp.probabilities = levels.into();
            }
        };
        assert!(why(&temp([(0, 0.1), (1, 0.9), (2, 0.2)])).contains("its probabilities add up to 1.200, not 1"));
        assert!(why(&temp([(0, 0.9), (1, -0.1), (2, 0.2)])).contains("level 1's probability is -0.1"));
        assert!(why(&temp([(0, 0.1), (1, 0.9), (3, 0.0)])).contains("level 3, and the question's levels are 0 to 2"));
        assert!(why(&|reply: &mut DecisionResponse| {
            let crate::Answer::Choice(sky) = reply.answers.get_mut("sky").unwrap() else { unreachable!() };
            sky.probabilities.insert("hail".to_owned(), 0.0);
        })
        .contains("`hail`, which isn't one of the question's options"));
        // Two places of rounding is not a mistake: 0.33 three times is 0.99.
        let mut rounded = reply();
        let crate::Answer::Choice(sky) = rounded.answers.get_mut("sky").unwrap() else { unreachable!() };
        sky.probabilities = [("clear", 0.33), ("cloudy", 0.33), ("storm", 0.33)].map(|(option, p)| (option.to_owned(), p)).into();
        assert!(rules.evaluate(&rounded).is_ok());
        // The drawings check the same way.
        let mut bad = reply();
        noul(1.2)(&mut bad);
        assert!(rules.graph_text(Some(&bad)).is_err() && rules.graph_svg(Some(&bad)).is_err());
    }

    #[test]
    fn a_missing_answer_is_an_error_not_a_zero() {
        let rules = rules("[[rule]]\nif = \"raining\"\nthen = \"umbrella\"").unwrap();
        let mut reply = reply();
        reply.answers.remove("raining");
        assert_eq!(rules.evaluate(&reply), Err(crate::Error::MissingAnswer("raining".to_owned())));
    }
}
