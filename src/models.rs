//! The models on the decisions endpoint: which there are, and what some of them can't do.
//!
//! Which models there are is OpenRouter's to say, and it is asked each time
//! ([`crate::Client::list_models`], `jev model list`), so a model it adds is there without a new
//! version of this crate, and one it drops is gone. Any id can be sent
//! ([`crate::Client::with_model`]).
//!
//! What its listing doesn't say, or says wrongly, was found by calling the models, and is kept here
//! as limits, for those models alone: the ones that answer only yes/no questions, read no images,
//! take fewer questions a call than the endpoint does, or read less of a state than they're given.
//! A request asking one of them what it can't answer is refused before it is sent
//! ([`check_request`]), rather than costing a call that fails. A model with no limits here is taken
//! at its word.
//!
//! The listing says `scaledown/classify-1` reads images, for one, when it refuses them; and it lists
//! no context window for the Span 01 models, which read about 3,000 tokens of a state.

use serde::{Deserialize, Serialize};

use crate::{DecisionRequest, Error, Question};

/// What calling a model found it can't do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Limits {
    /// Its id, as a request names it.
    id: &'static str,
    /// Whether it answers only yes/no (Noul) questions, and only ones whose instructions and
    /// criteria are plain text.
    noul_only: bool,
    /// Whether it reads images in the state ([`crate::image_part`]).
    images: bool,
    /// The most questions it takes in one call, where that is fewer than the endpoint's own limit.
    /// One more is refused with a 422, after the call.
    max_questions: Option<usize>,
    /// Roughly how many tokens of a state it reads, where OpenRouter lists a context window of 0. It
    /// takes a state of any size, reads its start and end, and skips the middle without saying so:
    /// a fact in the middle of a long state scores as if it weren't there.
    context: Option<u64>,
}

/// No limits, for the table to start each entry from.
const NONE: Limits = Limits { id: "", noul_only: false, images: true, max_questions: None, context: None };

/// The models with limits: every other one is taken at its word. Text-only models are here, though
/// the listing says so too, so an image sent to one is refused without asking the listing.
const LIMITS: [Limits; 14] = [
    Limits { id: "respan/span-01", noul_only: true, images: false, context: Some(3000), ..NONE },
    Limits { id: "respan/span-01-lite", noul_only: true, images: false, context: Some(3000), ..NONE },
    Limits { id: "respan/span-01-lite:free", noul_only: true, images: false, context: Some(3000), ..NONE },
    Limits { id: "jaredpalmer/kev-4b", images: false, ..NONE },
    Limits { id: "parallel/decisions-fast:free", images: false, max_questions: Some(100), ..NONE },
    Limits { id: "nace-ai/drex-max", max_questions: Some(16), ..NONE },
    Limits { id: "nace-ai/drex-v2.0", images: false, ..NONE },
    Limits { id: "nace-ai/drex-v1.5", images: false, ..NONE },
    Limits { id: "morph/reflex", max_questions: Some(128), ..NONE },
    Limits { id: "microsoft/microsoft-decision-1", images: false, ..NONE },
    Limits { id: "upstage/solar-decide", images: false, ..NONE },
    Limits { id: "upstage/solar-decide-flash", images: false, ..NONE },
    Limits { id: "togethercomputer/tev1-4b-experimental", images: false, ..NONE },
    // Also: every Choice option needs a description, which isn't checked here.
    Limits { id: "scaledown/classify-1", images: false, ..NONE },
];

/// The limits of the model with this id, if it has any.
fn limits(id: &str) -> Option<&'static Limits> {
    LIMITS.iter().find(|limits| limits.id == id)
}

/// A model as it is on offer now: OpenRouter's listing, corrected by what calling it found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelInfo {
    /// Its id, as a request names it.
    pub id: String,
    /// What OpenRouter calls it: `TypeSafe: Jev 1.14`.
    pub name: String,
    /// How many tokens it reads at once, if that is known.
    pub context: Option<u64>,
    /// Whether [`ModelInfo::context`] was measured by this crate rather than listed by OpenRouter.
    pub measured: bool,
    /// Dollars per million input tokens, if it is listed; 0 for a free model.
    pub price: Option<f64>,
    /// What a state can hold for it: `text`, `image`, and for some `audio`, `video` or `file`.
    pub inputs: Vec<String>,
    /// Whether it answers only yes/no (Noul) questions with plain-text instructions and criteria.
    pub noul_only: bool,
    /// The most questions it takes in one call, where that is fewer than the endpoint takes.
    pub max_questions: Option<usize>,
}

/// One entry of OpenRouter's listing, as far as this crate reads it.
#[derive(Deserialize)]
struct Listed {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    pricing: Pricing,
    #[serde(default)]
    architecture: Architecture,
}

#[derive(Default, Deserialize)]
struct Pricing {
    /// Dollars per input token, as a string: `"0.000000042"`.
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Default, Deserialize)]
struct Architecture {
    #[serde(default)]
    input_modalities: Vec<String>,
}

impl Listed {
    fn info(&self) -> ModelInfo {
        let limits = limits(&self.id);
        let listed = self.context_length.filter(|context| *context > 0);
        let measured = limits.and_then(|limits| limits.context);
        let price = self.pricing.prompt.as_deref().and_then(|price| price.parse::<f64>().ok());
        let mut inputs = self.architecture.input_modalities.clone();
        // A call settles this where the listing and this crate differ: see the top of this file.
        inputs.retain(|input| input != "image" || limits.is_none_or(|limits| limits.images));
        ModelInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            context: listed.or(measured),
            measured: listed.is_none() && measured.is_some(),
            // To the millionth of a dollar: multiplying leaves 0.042 as 0.042000000000000004. A
            // price below zero is OpenRouter's way of saying it varies.
            price: price.filter(|price| *price >= 0.0).map(|price| (price * 1e12).round() / 1e6),
            inputs,
            noul_only: limits.is_some_and(|limits| limits.noul_only),
            max_questions: limits.and_then(|limits| limits.max_questions),
        }
    }
}

/// OpenRouter's listing ([`crate::MODELS_URL`]) as [`ModelInfo`]s: the default model
/// ([`crate::DEFAULT_MODEL`]) first, and then the rest by id, a `~` id beside the ones it follows.
/// Each is what the listing says of it, corrected by its limits, if it has any; a model that isn't
/// listed isn't there.
pub fn read_listing(body: &[u8]) -> crate::Result<Vec<ModelInfo>> {
    #[derive(Deserialize)]
    struct Listing {
        data: Vec<Listed>,
    }
    let Listing { data } = serde_json::from_slice(body).map_err(|e| Error::Decode(format!("the models list: {e}")))?;
    let mut models: Vec<ModelInfo> = data.iter().map(Listed::info).collect();
    models.sort_by_cached_key(|model| (model.id != crate::DEFAULT_MODEL, model.id.trim_start_matches('~').to_owned(), model.id.clone()));
    Ok(models)
}

/// Whether `request`'s model can answer each of its questions and read its state, when it is one
/// with limits; any other model is taken at its word. The first question it can't answer is an
/// [`Error::Invalid`], as is a question past the most it takes in a call, and images it can't read
/// an [`Error::Image`]. [`crate::Client`] checks this where it builds a request and again where it
/// sends one, and the CLI where it builds one, so a dry run finds what sending would.
pub fn check_request(request: &DecisionRequest) -> crate::Result<()> {
    let Some(limits) = limits(&request.model) else { return Ok(()) };
    if !limits.images && crate::has_images(&request.state) {
        return Err(Error::Image(format!("{} reads text only, and this state has images in it", limits.id)));
    }
    for (at, (id, question)) in request.questions.iter().enumerate() {
        limits.check(question).map_err(|why| Error::Invalid { id: id.clone(), why })?;
        if let Some(most) = limits.max_questions.filter(|most| at >= *most) {
            let why = format!(
                "{} takes at most {most} questions a call, and this request asks {}: split them",
                limits.id,
                request.questions.len()
            );
            return Err(Error::Invalid { id: id.clone(), why });
        }
    }
    Ok(())
}

impl Limits {
    /// Whether this model can answer `question`: every model answers every type, except one that
    /// is `noul_only`, which takes a yes/no question with plain-text instructions and criteria, and
    /// nothing else. The error says why, as [`crate::Error::Invalid`]'s `why`.
    fn check(&self, question: &Question) -> Result<(), String> {
        if !self.noul_only {
            return Ok(());
        }
        let only = |what: &str| format!("{} answers yes/no questions only, and this is {what}", self.id);
        match question {
            Question::Choice { .. } => Err(only("a choice")),
            Question::Score { .. } => Err(only("a score")),
            Question::Noul { instructions, criteria } => {
                let plain = instructions.is_string()
                    && criteria.as_ref().is_none_or(|criteria| criteria.yes.is_string() && criteria.no.is_string());
                match plain {
                    true => Ok(()),
                    false => Err(format!("{} takes yes/no questions whose instructions and criteria are plain text", self.id)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keeps_each_models_limits_once() {
        for (at, one) in LIMITS.iter().enumerate() {
            assert!(LIMITS[..at].iter().all(|earlier| earlier.id != one.id), "{} twice", one.id);
            // An entry that limits nothing would only be a list of models.
            assert_ne!(Limits { id: one.id, ..NONE }, *one, "{} has no limits", one.id);
        }
        // The default has none: it answers everything, and reads images.
        assert!(limits(crate::DEFAULT_MODEL).is_none());
        assert!(limits("someone/else").is_none());
    }

    #[test]
    fn reads_the_listing_and_corrects_it() {
        let listing = json!({"data": [
            {"id": "someone/new", "name": "Someone: New", "context_length": 1024, "pricing": {"prompt": "0.0000005", "completion": "0"},
             "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "typesafe/jev-1.14", "name": "TypeSafe: Jev 1.14", "context_length": 262144,
             "pricing": {"prompt": "0.000000042", "completion": "0"}, "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "scaledown/classify-1", "context_length": 1000000, "pricing": {"prompt": "0.00000004"},
             "architecture": {"input_modalities": ["text", "image", "file"]}},
            {"id": "cloudflare/clef-omni", "context_length": 65536, "pricing": {"prompt": "-1"},
             "architecture": {"input_modalities": ["text", "image", "audio", "video"]}},
            {"id": "respan/span-01", "context_length": 0, "pricing": {"prompt": "0.00000002"},
             "architecture": {"input_modalities": ["text"]}},
            {"id": "respan/span-01-lite:free", "context_length": 0, "pricing": {"prompt": "0"},
             "architecture": {"input_modalities": ["text"]}},
            {"id": "nace-ai/drex-max", "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "~liquid/d1-latest", "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "liquid/d1.2", "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "~typesafe/jev-latest", "architecture": {"input_modalities": ["text", "image"]}},
        ]});
        let models = read_listing(listing.to_string().as_bytes()).unwrap();
        let find = |id: &str| models.iter().find(|model| model.id == id).unwrap();

        // What the listing has, and nothing else: the default first, then by id, `~` or not.
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "~typesafe/jev-latest",
                "cloudflare/clef-omni",
                "~liquid/d1-latest",
                "liquid/d1.2",
                "nace-ai/drex-max",
                "respan/span-01",
                "respan/span-01-lite:free",
                "scaledown/classify-1",
                "someone/new",
                "typesafe/jev-1.14",
            ]
        );
        let new = find("someone/new");
        assert_eq!((new.name.as_str(), new.context, new.noul_only, new.max_questions), ("Someone: New", Some(1024), false, None));
        assert_eq!(new.inputs, ["text", "image"]);

        // Exactly, where multiplying by a million alone wouldn't be.
        assert_eq!(find("typesafe/jev-1.14").price, Some(0.042));
        assert_eq!(find("typesafe/jev-1.14").context, Some(262_144));
        assert!(!find("typesafe/jev-1.14").measured);
        assert_eq!(find("respan/span-01-lite:free").price, Some(0.0));
        // The listing says this one reads images, and it refuses them; its other input stays.
        assert_eq!(find("scaledown/classify-1").inputs, ["text", "file"]);
        assert_eq!(find("cloudflare/clef-omni").inputs, ["text", "image", "audio", "video"]);
        // A price that varies isn't a price.
        assert_eq!(find("cloudflare/clef-omni").price, None);
        // A context of 0 means none is listed, which a measurement fills in.
        assert_eq!((find("respan/span-01").context, find("respan/span-01").measured), (Some(3000), true));
        // What the listing doesn't say at all.
        assert!(find("respan/span-01").noul_only);
        assert_eq!(find("nace-ai/drex-max").max_questions, Some(16));
        assert_eq!((find("nace-ai/drex-max").context, find("nace-ai/drex-max").price), (None, None));

        assert!(read_listing(b"<html>").unwrap_err().to_string().contains("the models list"));
    }

    #[test]
    fn a_listed_context_replaces_a_measured_one() {
        let listing = json!({"data": [{"id": "respan/span-01", "context_length": 8192}]});
        let models = read_listing(listing.to_string().as_bytes()).unwrap();
        assert_eq!((models[0].context, models[0].measured), (Some(8192), false));
    }

    #[test]
    fn a_text_only_model_is_refused_images() {
        let request = |model: &str, state| DecisionRequest {
            model: model.to_owned(),
            state,
            questions: [("q".to_owned(), Question::noul("Is it?"))].into(),
        };
        let image = crate::image_part(b"\x89PNG\r\n\x1a\n").unwrap();
        let refused = check_request(&request("scaledown/classify-1", json!([image]))).unwrap_err().to_string();
        assert!(refused.contains("scaledown/classify-1 reads text only"), "{refused}");
        assert!(check_request(&request("scaledown/classify-1", json!("text"))).is_ok());
        assert!(check_request(&request("~liquid/d1-latest", json!([image]))).is_ok());
        // A model with no limits here is taken at its word.
        assert!(check_request(&request("someone/else", json!([image]))).is_ok());
    }

    #[test]
    fn refuses_more_questions_than_a_model_takes_in_a_call() {
        let request = |model: &str, count: usize| DecisionRequest {
            model: model.to_owned(),
            state: json!("text"),
            questions: (0..count).map(|at| (format!("q{at:02}"), Question::noul("Is it?"))).collect(),
        };
        assert!(check_request(&request("nace-ai/drex-max", 16)).is_ok());
        let refused = check_request(&request("nace-ai/drex-max", 17)).unwrap_err().to_string();
        assert_eq!(refused, "question `q16`: nace-ai/drex-max takes at most 16 questions a call, and this request asks 17: split them");
        // Every other model takes as many as the endpoint does.
        assert!(check_request(&request("~typesafe/jev-latest", 17)).is_ok());
    }

    #[test]
    fn a_noul_only_model_takes_plain_yes_no_questions_alone() {
        let span = limits("respan/span-01").unwrap();
        assert!(span.check(&Question::noul("Is it raining?")).is_ok());
        assert!(span.check(&Question::noul_with_criteria("Is it raining?", "Rain is falling", "It is dry")).is_ok());
        let score = span.check(&Question::score("How warm?", ["Cold", "Hot"])).unwrap_err();
        assert!(score.contains("respan/span-01 answers yes/no questions only") && score.contains("a score"), "{score}");
        assert!(span.check(&Question::choice("Which?", [("a", "A"), ("b", "B")])).unwrap_err().contains("a choice"));
        assert!(span.check(&Question::noul(json!({"what": "structured"}))).unwrap_err().contains("plain text"));
        assert!(span.check(&Question::noul_with_criteria("Is it?", json!(["yes"]), "no")).is_err());
        // Every other model takes every type.
        assert!(limits("jaredpalmer/kev-4b").unwrap().check(&Question::score("How warm?", ["Cold", "Hot"])).is_ok());
    }
}
