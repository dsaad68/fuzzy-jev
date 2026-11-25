//! A client for OpenRouter's decision models, TypeSafe's Jev by default, on its decisions endpoint.
//! A model reads a state (a string, a JSON object or an array, which can hold images: [`image_part`])
//! and answers typed questions about it: a Choice between options, a Score along levels, or a Noul, the
//! probability that something is true.
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
//!
//! Ollama serves decision models too, such as Liquid's `d1`, on its own System One endpoint
//! ([`OLLAMA_URL`]); a client sent there speaks Ollama's shape ([`Client::with_ollama`]).

mod error;
mod image;
pub mod models;
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
pub use image::{has_images, image_part, text_part};
pub use models::ModelInfo;
pub use types::{
    Answer, ChoiceAnswer, DecisionRequest, DecisionResponse, NoulAnswer, NoulCriteria, Options, Question, ScoreAnswer, Usage, ROUNDING,
};

/// The model requests go to unless the client says otherwise: TypeSafe's Jev, whichever version is
/// the latest. [`Client::list_models`] lists the others on offer; pin one, such as
/// `typesafe/jev-1.13`, for answers that don't change when a new version comes out.
pub const DEFAULT_MODEL: &str = "~typesafe/jev-latest";

/// OpenRouter's decisions endpoint.
pub const DECISIONS_URL: &str = "https://openrouter.ai/api/alpha/decisions";

/// Ollama's System One endpoint, on the machine it runs on. It needs no key. A request to it, or
/// to any other URL whose path is `/v1/systemone` (such as `https://ollama.com/v1/systemone`), is
/// sent in Ollama's shape: see [`Client::with_ollama`].
pub const OLLAMA_URL: &str = "http://localhost:11434/v1/systemone";

/// OpenRouter's list of the models on that endpoint, with each one's price and context window.
pub const MODELS_URL: &str = "https://openrouter.ai/api/v1/models?output_modalities=decisions";

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
    ollama: bool,
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
            ollama: false,
        }
    }

    /// Gives up on a request that takes longer than `timeout` in all, instead of
    /// [`DEFAULT_TIMEOUT`]. This crate never sends a request again by itself, after a timeout or any
    /// other failure: one that timed out may have been answered, and billed, all the same. (The HTTP
    /// client underneath, or one given to [`Client::with_http`], has retry rules of its own for
    /// connection failures.) On wasm32 the host's timer takes at most about 24 days, and a longer
    /// timeout is cut to that.
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

    /// Sends requests to `url` instead of OpenRouter's decisions endpoint. A URL whose path is
    /// `/v1/systemone`, Ollama's ([`OLLAMA_URL`]), turns on Ollama's shape, and any other turns it
    /// off; [`Client::with_ollama`] after this says otherwise.
    pub fn with_url(mut self, url: &str) -> Client {
        url.clone_into(&mut self.url);
        self.ollama = is_ollama(url);
        self
    }

    /// Whether to send requests in Ollama's shape. Ollama reads a state as text and refuses
    /// images inside it, so a state's images ([`image_part`]) go in a top-level `images` array
    /// instead, as bare base64, and its text parts are joined into a string. A request without
    /// images is the same either way. [`Client::with_url`] sets this from the URL.
    pub fn with_ollama(mut self, ollama: bool) -> Client {
        self.ollama = ollama;
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

    /// The request [`Client::decide`] sends. A question its model can't answer is refused here, as
    /// in [`Client::send`].
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
        let request = DecisionRequest {
            model: self.model.clone(),
            state: serde_json::to_value(state).map_err(|e| Error::Decode(format!("state: {e}")))?,
            questions: asked,
        };
        // What the model can't answer is refused here as well as in `send`, so the request a
        // caller builds to look at first is one that sending would take.
        models::check_request(&request)?;
        Ok(request)
    }

    /// Sends `request` as it is, whatever model it names. A question the model can't answer (one
    /// that takes yes/no questions only, asked anything else: [`models::check_request`]) is refused
    /// here, before the call, as [`Error::Invalid`], and images it can't read as [`Error::Image`].
    /// The reply is checked against the request
    /// ([`DecisionResponse::check_against`]): an answer missing, of another type, or that isn't
    /// well formed is an error here, so every caller gets the same, checked reply.
    pub async fn send(&self, request: &DecisionRequest) -> Result<DecisionResponse> {
        models::check_request(request)?;
        let body = serde_json::to_vec(&self.body(request)).map_err(|e| Error::Decode(e.to_string()))?;
        let mut post = self.http.post(&self.url).header("content-type", "application/json").timeout(self.usable_timeout()).body(body);
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
        let reply: DecisionResponse = serde_json::from_slice(&body).map_err(|e| Error::Decode(e.to_string()))?;
        reply.check_against(&request.questions)?;
        Ok(reply)
    }

    /// What [`Client::send`] sends for `request`, to serialize: the request itself, or for Ollama
    /// ([`Client::with_ollama`]), the request with its images beside the state. It is never a
    /// [`serde_json::Value`] on the way, which would sort a Choice's options by name.
    pub fn body<'a>(&self, request: &'a DecisionRequest) -> impl Serialize + 'a {
        #[derive(Serialize)]
        #[serde(untagged)]
        enum Body<'a> {
            Request(&'a DecisionRequest),
            Ollama { model: &'a str, state: serde_json::Value, images: Vec<String>, questions: &'a BTreeMap<String, Question> },
        }
        match self.ollama.then(|| image::ollama_shape(&request.state)) {
            Some((state, images)) if !images.is_empty() => {
                Body::Ollama { model: &request.model, state, images, questions: &request.questions }
            }
            _ => Body::Request(request),
        }
    }

    /// The models on offer now, from the list at `url`, usually [`MODELS_URL`]: each one's context
    /// window, price and inputs, corrected by what calling it found, and what it can't answer
    /// ([`models::read_listing`]). The list is public, so no key is sent.
    pub async fn list_models(&self, url: &str) -> Result<Vec<ModelInfo>> {
        let reply = self.http.get(url).timeout(self.usable_timeout()).send().await.map_err(|e| Error::Http(e.to_string()))?;
        let status = reply.status();
        let body = reply.bytes().await.map_err(|e| Error::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(Error::Http(format!("{url} returned HTTP {}: {}", status.as_u16(), error_message(&body))));
        }
        models::read_listing(&body)
    }

    /// The timeout, cut to what the target's timer takes: reqwest on wasm32 hands JavaScript's
    /// `setTimeout` a 32-bit count of milliseconds, and panics on a longer one.
    fn usable_timeout(&self) -> Duration {
        if cfg!(target_arch = "wasm32") {
            self.timeout.min(Duration::from_millis(i32::MAX as u64))
        } else {
            self.timeout
        }
    }
}

/// Whether `url` is Ollama's System One endpoint, on whatever host: its path is `/v1/systemone`.
/// Liquid's own API, at `/decisions/v1/systemone`, is another one, and isn't sent Ollama's shape.
fn is_ollama(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|url| url.path().trim_end_matches('/') == "/v1/systemone")
}

/// What an error reply says: OpenRouter's `{"error": {"message": …}}`, Ollama's `{"error": "…"}`,
/// or the start of the body.
fn error_message(body: &[u8]) -> String {
    #[derive(serde::Deserialize)]
    struct Reply {
        error: Message,
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Message {
        Object { message: String },
        Text(String),
    }
    match serde_json::from_slice::<Reply>(body) {
        Ok(Reply { error: Message::Object { message } | Message::Text(message) }) => message,
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

    #[tokio::test]
    async fn refuses_what_a_yes_no_model_cant_answer_before_calling() {
        // Nothing listens on the discard port: the refusal comes before any connection is tried.
        let client = Client::new("key").with_url("http://127.0.0.1:9").with_model("respan/span-01");
        match client.decide("state", triage()).await {
            Err(Error::Invalid { id, why }) => {
                assert_eq!(id, "department");
                assert!(why.contains("respan/span-01 answers yes/no questions only"), "{why}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        // Building the request refuses it too, and a request built by hand is refused on sending.
        assert!(matches!(client.request("state", triage()), Err(Error::Invalid { .. })));
        let by_hand = Client::new("key")
            .request("state", triage())
            .map(|request| DecisionRequest { model: "respan/span-01".to_owned(), ..request })
            .unwrap();
        assert!(matches!(client.send(&by_hand).await, Err(Error::Invalid { .. })));
    }

    #[test]
    fn builds_the_documented_request() {
        let request = Client::new("key").request("Help! My payouts have been failing for 3 days.", triage()).unwrap();
        let expected = json!({
            "model": "~typesafe/jev-latest",
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
        assert_eq!(error_message(br#"{"error":"model 'd2' not found"}"#), "model 'd2' not found");
        assert_eq!(error_message(b"Bad Gateway"), "Bad Gateway");
    }

    #[test]
    fn knows_ollamas_endpoint_by_its_path() {
        for url in [OLLAMA_URL, "http://gpu-box:11434/v1/systemone/", "https://ollama.com/v1/systemone"] {
            assert!(is_ollama(url), "{url}");
        }
        for url in [DECISIONS_URL, "https://api.liquid.ai/decisions/v1/systemone", "http://localhost:11434/api/chat", "not a url"] {
            assert!(!is_ollama(url), "{url}");
        }
    }

    #[test]
    fn sends_images_beside_the_state_to_ollama() {
        let state = json!([crate::text_part("Returned by a customer."), crate::image_part(b"\x89PNG\r\n\x1a\nrest").unwrap()]);
        let questions = || [("damaged", Question::noul("Is it damaged?"))];
        let ollama = Client::new("").with_url(OLLAMA_URL).with_model("d1");
        let body = serde_json::to_value(ollama.body(&ollama.request(&state, questions()).unwrap())).unwrap();
        assert_eq!(
            body,
            json!({
                "model": "d1",
                "state": "Returned by a customer.",
                "images": ["iVBORw0KGgpyZXN0"],
                "questions": {"damaged": {"type": "noul", "instructions": "Is it damaged?"}}
            })
        );

        // OpenRouter, or Ollama's shape turned off, sends the request as it is.
        for client in [Client::new("key"), Client::new("").with_url(OLLAMA_URL).with_ollama(false)] {
            let request = client.request(&state, questions()).unwrap();
            assert_eq!(serde_json::to_string(&client.body(&request)).unwrap(), serde_json::to_string(&request).unwrap());
        }
        let after = Client::new("").with_ollama(true).with_url(DECISIONS_URL);
        assert!(serde_json::to_value(after.body(&ollama.request(&state, questions()).unwrap())).unwrap()["images"].is_null());

        // A Choice's options go in the order written, as they do to OpenRouter.
        let colour = [("colour", Question::choice("Which colour?", [("red", ""), ("blue", ""), ("green", "")]))];
        let sent = serde_json::to_string(&ollama.body(&ollama.request(&state, colour).unwrap())).unwrap();
        assert!(sent.contains(r#""criteria":{"red":"","blue":"","green":""}"#), "{sent}");
    }
}
