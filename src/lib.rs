//! A client for TypeSafe's Jev, a System One model, on OpenRouter's decisions endpoint. Jev reads a
//! state (a string, a JSON object or an array) and answers typed questions about it: a Choice
//! between options, a Score along levels, or a Noul, the probability that something is true.
//!
//! ```no_run
//! # async fn run() -> jev::Result<()> {
//! use jev::{Client, Question};
//!
//! let client = Client::new(&std::env::var("OPENROUTER_API_KEY").unwrap_or_default());
//! let reply = client
//!     .decide(
//!         "Help! My payouts have been failing for 3 days.",
//!         [
//!             ("is_urgent", Question::noul("Does this message convey urgency?")),
//!             ("department", Question::choice("Which team should handle this?", [("billing", "Payments"), ("technical", "Bugs")])),
//!             ("frustration", Question::score("How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"])),
//!         ],
//!     )
//!     .await?;
//! if reply.noul("is_urgent")? > 0.8 && reply.choice("department")?.confidence > 0.6 {
//!     println!("page {}", reply.choice("department")?.choice);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Works natively and on wasm32, where requests go through the host's `fetch`.

mod error;
#[cfg(feature = "command")]
pub mod print;
#[cfg(feature = "command")]
pub mod rules;
#[cfg(feature = "command")]
pub mod skill;
#[cfg(feature = "command")]
pub mod spec;
mod types;

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

pub use error::{Error, Result};
pub use types::{Answer, ChoiceAnswer, DecisionRequest, DecisionResponse, NoulAnswer, NoulCriteria, Options, Question, ScoreAnswer, Usage};

/// The model requests go to unless the client says otherwise.
pub const DEFAULT_MODEL: &str = "typesafe/jev-1.13";

/// OpenRouter's decisions endpoint.
pub const DECISIONS_URL: &str = "https://openrouter.ai/api/alpha/decisions";

/// How long a request may take, from sending it to reading the whole reply, unless the client says
/// otherwise ([`Client::with_timeout`]). A reply usually takes a second or two.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Sends decision requests. `Debug` isn't derived, to keep the key out of logs.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    key: String,
    url: String,
    model: String,
    timeout: Duration,
}

impl Client {
    /// A client for OpenRouter with an API key. An empty key sends no `Authorization` header, for an
    /// endpoint (see [`Client::with_url`]) that adds the key itself.
    pub fn new(key: &str) -> Client {
        Client {
            http: reqwest::Client::new(),
            key: key.trim().to_owned(),
            url: DECISIONS_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Gives up on a request that takes longer than `timeout` in all, instead of
    /// [`DEFAULT_TIMEOUT`]. A request that timed out is not sent again: it may have been answered,
    /// and billed, all the same.
    pub fn with_timeout(mut self, timeout: Duration) -> Client {
        self.timeout = timeout;
        self
    }

    /// Sends requests through `http`, for a proxy, other TLS roots or a shared connection pool.
    /// The timeout is still this client's own ([`Client::with_timeout`]).
    pub fn with_http(mut self, http: reqwest::Client) -> Client {
        self.http = http;
        self
    }

    /// Sends requests to `url` instead of OpenRouter's decisions endpoint.
    pub fn with_url(mut self, url: &str) -> Client {
        url.clone_into(&mut self.url);
        self
    }

    /// Asks `model` instead of [`DEFAULT_MODEL`].
    pub fn with_model(mut self, model: &str) -> Client {
        model.clone_into(&mut self.model);
        self
    }

    /// Asks `questions`, under their ids, about `state`.
    pub async fn decide<K: Into<String>>(
        &self,
        state: impl Serialize,
        questions: impl IntoIterator<Item = (K, Question)>,
    ) -> Result<DecisionResponse> {
        self.send(&self.request(state, questions)?).await
    }

    /// The request [`Client::decide`] sends.
    pub fn request<K: Into<String>>(
        &self,
        state: impl Serialize,
        questions: impl IntoIterator<Item = (K, Question)>,
    ) -> Result<DecisionRequest> {
        let mut asked = BTreeMap::new();
        for (id, question) in questions {
            let id = id.into();
            question.check().map_err(|why| Error::Invalid { id: id.clone(), why })?;
            // Collecting into the map would keep the last of two questions under one id and drop
            // the other, leaving the caller an answer short with nothing to say why.
            if asked.insert(id.clone(), question).is_some() {
                return Err(Error::DuplicateQuestion(id));
            }
        }
        Ok(DecisionRequest {
            model: self.model.clone(),
            state: serde_json::to_value(state).map_err(|e| Error::Decode(format!("state: {e}")))?,
            questions: asked,
        })
    }

    /// Sends `request` as it is, whatever model it names.
    pub async fn send(&self, request: &DecisionRequest) -> Result<DecisionResponse> {
        let body = serde_json::to_vec(request).map_err(|e| Error::Decode(e.to_string()))?;
        let mut post = self.http.post(&self.url).header("content-type", "application/json").timeout(self.timeout).body(body);
        if !self.key.is_empty() {
            post = post.bearer_auth(&self.key);
        }
        // reqwest's errors hold JavaScript values on wasm32, so they're turned into messages.
        let reply = post.send().await.map_err(|e| Error::Http(e.to_string()))?;
        let status = reply.status();
        let body = reply.bytes().await.map_err(|e| Error::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(Error::Status { status: status.as_u16(), message: error_message(&body) });
        }
        serde_json::from_slice(&body).map_err(|e| Error::Decode(e.to_string()))
    }
}

/// What an error reply says: OpenRouter's `{"error": {"message": …}}`, or the start of the body.
fn error_message(body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct Reply {
        error: Message,
    }
    #[derive(serde::Deserialize)]
    struct Message {
        message: String,
    }
    match serde_json::from_slice::<Reply>(body) {
        Ok(reply) => reply.error.message,
        Err(_) => String::from_utf8_lossy(&body[..body.len().min(300)]).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    fn triage() -> Vec<(&'static str, Question)> {
        vec![
            (
                "is_urgent",
                Question::noul_with_criteria("Does this message convey urgency?", "Explicitly time-sensitive", "No urgency expressed"),
            ),
            (
                "department",
                Question::choice(
                    "Which team should handle this?",
                    [
                        ("billing", "Payments, invoicing, refunds"),
                        ("technical", "Bugs, outages, integrations"),
                        ("sales", "Pricing, upgrades, new accounts"),
                    ],
                ),
            ),
            ("frustration", Question::score("How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"])),
        ]
    }

    #[test]
    fn builds_the_documented_request() {
        let request = Client::new("key").request("Help! My payouts have been failing for 3 days.", triage()).unwrap();
        let expected = json!({
            "model": "typesafe/jev-1.13",
            "state": "Help! My payouts have been failing for 3 days.",
            "questions": {
                "is_urgent": {
                    "type": "noul",
                    "instructions": "Does this message convey urgency?",
                    "criteria": {"true": "Explicitly time-sensitive", "false": "No urgency expressed"}
                },
                "department": {
                    "type": "choice",
                    "instructions": "Which team should handle this?",
                    "criteria": {
                        "billing": "Payments, invoicing, refunds",
                        "technical": "Bugs, outages, integrations",
                        "sales": "Pricing, upgrades, new accounts"
                    }
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated is the customer?",
                    "criteria": ["Calm", "Frustrated", "Very angry"]
                }
            }
        });
        assert_eq!(serde_json::to_value(&request).unwrap(), expected);
    }

    #[test]
    fn keeps_choice_options_in_order() {
        let question = Question::choice("?", [("zeta", "last letter"), ("alpha", "first letter")]);
        assert_eq!(
            serde_json::to_string(&question).unwrap(),
            r#"{"type":"choice","instructions":"?","criteria":{"zeta":"last letter","alpha":"first letter"}}"#
        );
    }

    #[test]
    fn leaves_out_missing_noul_criteria() {
        assert_eq!(serde_json::to_value(Question::noul("Is it?")).unwrap(), json!({"type": "noul", "instructions": "Is it?"}));
    }

    #[test]
    fn takes_structured_state_and_criteria() {
        let question = Question::choice(json!({"question": "Which?", "focus": "the primary request"}), [("a", Value::Null)]);
        let request = Client::new("").with_model("typesafe/jev-latest").request(json!({"message": "hi"}), [("q", question)]).unwrap();
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "model": "typesafe/jev-latest",
                "state": {"message": "hi"},
                "questions": {"q": {"type": "choice", "instructions": {"question": "Which?", "focus": "the primary request"}, "criteria": {"a": null}}}
            })
        );
    }

    #[test]
    fn reads_a_reply() {
        let reply: DecisionResponse = serde_json::from_str(include_str!("../tests/fixtures/decision.json")).unwrap();
        assert_eq!(reply.model, "typesafe/jev-1.13-20260917");
        assert_eq!(reply.noul("is_urgent").unwrap(), 0.95);

        let department = reply.choice("department").unwrap();
        assert_eq!(department.choice, "billing");
        assert_eq!(department.confidence, 0.82);
        assert_eq!(department.probabilities["technical"], 0.12);

        let frustration = reply.score("frustration").unwrap();
        assert_eq!(frustration.score, 1.04);
        assert_eq!(frustration.probabilities[&1], 0.96);
        assert_eq!(frustration.legend[&2], json!("Very angry"));

        assert_eq!(reply.usage.input_tokens, 427);
        assert_eq!(reply.usage.cost, Some(0.000017934));
        assert_eq!(reply.provider.as_deref(), Some("TypeSafe"));
    }

    #[test]
    fn says_which_answer_is_missing_or_of_another_type() {
        let reply: DecisionResponse = serde_json::from_str(include_str!("../tests/fixtures/decision.json")).unwrap();
        assert_eq!(reply.noul("nope"), Err(Error::MissingAnswer("nope".into())));
        assert_eq!(reply.score("department").unwrap_err().to_string(), "question `department` is a choice, not a score");
    }

    #[test]
    fn reads_an_unknown_answer_type() {
        let reply: DecisionResponse = serde_json::from_value(json!({
            "model": "m",
            "answers": {"q": {"type": "ranking", "order": [1, 2]}},
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }))
        .unwrap();
        // Nothing the endpoint sent is dropped: --json hands back what arrived.
        assert_eq!(reply.answers["q"], Answer::Other(json!({"type": "ranking", "order": [1, 2]})));
        assert_eq!(serde_json::to_value(&reply.answers["q"]).unwrap(), json!({"type": "ranking", "order": [1, 2]}));
        assert_eq!(reply.answers["q"].kind(), "ranking");
        assert_eq!(reply.usage.cost, None);
    }

    #[test]
    fn refuses_a_question_asked_twice() {
        let client = Client::new("key");
        let twice = [("a", Question::noul("Is it?")), ("a", Question::noul("Is it really?"))];
        assert_eq!(client.request("state", twice), Err(Error::DuplicateQuestion("a".into())));
    }

    #[test]
    fn refuses_a_question_the_endpoint_cannot_answer() {
        let client = Client::new("key");
        let invalid = |question| client.request("state", [("q", question)]).unwrap_err().to_string();
        let options: Vec<(String, &str)> = (0..256).map(|option| (format!("option{option}"), "")).collect();
        assert!(invalid(Question::choice("Which?", options)).contains("up to 255 options"));
        assert!(invalid(Question::choice("Which?", [("a", ""), ("a", "")])).contains("`a` is there twice"));
        assert!(invalid(Question::choice::<String, &str>("Which?", [])).contains("needs options"));
        assert!(invalid(Question::score("How much?", ["level"; 11])).contains("up to 10 levels"));
        assert!(invalid(Question::score::<&str>("How much?", [])).contains("needs levels"));
        // The docs ask for two levels, but the endpoint answers a single one, so the client allows it.
        assert!(client.request("state", [("q", Question::score("How much?", ["the only level"]))]).is_ok());
    }

    #[test]
    fn reads_error_replies() {
        assert_eq!(error_message(br#"{"error":{"message":"Missing Authentication header","code":401}}"#), "Missing Authentication header");
        assert_eq!(error_message(b"Bad Gateway"), "Bad Gateway");
    }
}
