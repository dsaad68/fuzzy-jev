//! The models on the decisions endpoint that this crate supports, and what each of them answers.
//!
//! Any model id can be sent ([`crate::Client::with_model`]); these are the ones this crate is
//! checked against. Some answer only yes/no questions, and a request asking one of them anything
//! else is refused before it is sent ([`Model::check`]), rather than costing a call that fails.
//! Some read no images, and a state with images in it is refused the same way.
//!
//! What each one takes comes from calling it, not from OpenRouter's listing, which says
//! `scaledown/classify-1` reads images when it refuses them.
//!
//! What changes without notice, a model's price and context window, isn't kept here: OpenRouter is
//! asked for it ([`crate::Client::list_models`]), and [`read_listing`] corrects what it says.

use serde::{Deserialize, Serialize};

use crate::{DecisionRequest, Error, Question};

/// A model this crate supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Its id, as a request names it.
    pub id: &'static str,
    /// What it is, in a line.
    pub about: &'static str,
    /// Whether it answers only yes/no (Noul) questions, and only ones whose instructions and
    /// criteria are plain text.
    pub noul_only: bool,
    /// Whether it reads images in the state ([`crate::image_part`]).
    pub images: bool,
}

/// The supported models, the default ([`crate::DEFAULT_MODEL`]) first.
pub const MODELS: [Model; 35] = [
    Model {
        id: "~typesafe/jev-latest",
        about: "TypeSafe's Jev, whichever version is the latest; the default",
        noul_only: false,
        images: true,
    },
    Model {
        id: "typesafe/jev-1.14",
        about: "TypeSafe's Jev 1.14, pinned: the same model until you change it",
        noul_only: false,
        images: true,
    },
    Model {
        id: "typesafe/jev-1.13",
        about: "TypeSafe's Jev 1.13, pinned: the same model until you change it",
        noul_only: false,
        images: true,
    },
    Model { id: "jaredpalmer/kev-4b", about: "Kev 4B, from jaredpalmer: text only", noul_only: false, images: false },
    Model { id: "respan/span-02", about: "Respan's Span 02", noul_only: false, images: true },
    Model { id: "respan/span-01", about: "Respan's Span 01: yes/no questions only, text only", noul_only: true, images: false },
    Model {
        id: "respan/span-01-lite",
        about: "Span 01 Lite, the lighter Span: yes/no questions only, text only",
        noul_only: true,
        images: false,
    },
    Model {
        id: "respan/span-01-lite:free",
        about: "Span 01 Lite, OpenRouter's free variant of it: yes/no questions only, text only",
        noul_only: true,
        images: false,
    },
    Model { id: "~liquid/d1-latest", about: "Liquid's d1, whichever version is the latest", noul_only: false, images: true },
    Model { id: "liquid/d1.1", about: "Liquid's d1.1, pinned", noul_only: false, images: true },
    Model { id: "liquid/d1", about: "Liquid's d1, pinned: leaves OpenRouter on 2026-11-20", noul_only: false, images: true },
    Model {
        id: "~liquid/d1-ultra-latest",
        about: "Liquid's largest, d1-ultra, whichever version is the latest",
        noul_only: false,
        images: true,
    },
    Model { id: "liquid/d1-ultra", about: "Liquid's d1-ultra, pinned", noul_only: false, images: true },
    Model {
        id: "~perplexity/pplx-decider-latest",
        about: "Perplexity's Decider, whichever version is the latest",
        noul_only: false,
        images: true,
    },
    Model { id: "perplexity/pplx-decider-v1.2", about: "Perplexity's Decider V1.2, pinned", noul_only: false, images: true },
    Model {
        id: "~perplexity/pplx-decider-flash-latest",
        about: "Decider Flash, the smaller Decider, whichever version is the latest",
        noul_only: false,
        images: true,
    },
    Model { id: "perplexity/pplx-decider-flash-v1.2", about: "Decider Flash V1.2, pinned", noul_only: false, images: true },
    Model {
        id: "parallel/decisions-fast:free",
        about: "Parallel's Decisions Fast, which also researches the live web; free, text only, at most 100 questions a call",
        noul_only: false,
        images: false,
    },
    Model { id: "baseten/mammoth-1", about: "Mammoth 1, Baseten's largest, 27B", noul_only: false, images: true },
    Model { id: "baseten/elephant-1", about: "Elephant 1, from Baseten, 9B", noul_only: false, images: true },
    Model { id: "baseten/joey-1", about: "Joey 1, Baseten's smallest, 2B", noul_only: false, images: true },
    Model { id: "cloudflare/clef", about: "Cloudflare's Clef, 27B", noul_only: false, images: true },
    Model { id: "cloudflare/clef-flash", about: "Clef Flash, the fast Clef, 9B", noul_only: false, images: true },
    Model { id: "cloudflare/clef-omni", about: "Clef Omni, the mixture-of-experts Clef", noul_only: false, images: true },
    Model { id: "nace-ai/drex-max", about: "Drex Max, Nace.AI's largest: at most 16 questions a call", noul_only: false, images: true },
    Model { id: "nace-ai/drex-v2.0", about: "Drex v2.0, from Nace.AI: text only", noul_only: false, images: false },
    Model { id: "nace-ai/drex-v1.5", about: "Drex v1.5, from Nace.AI: text only", noul_only: false, images: false },
    Model { id: "openai/gpt-6-luna-decisions", about: "GPT-6 Luna, served for decisions", noul_only: false, images: true },
    Model { id: "inception/mercury-decide", about: "Inception's Mercury Decide", noul_only: false, images: true },
    Model { id: "morph/reflex", about: "Morph's Reflex: at most 128 questions a call", noul_only: false, images: true },
    Model { id: "microsoft/microsoft-decision-1", about: "Microsoft's Decision 1: text only", noul_only: false, images: false },
    Model { id: "upstage/solar-decide", about: "Upstage's Solar Decide: text only", noul_only: false, images: false },
    Model {
        id: "upstage/solar-decide-flash",
        about: "Solar Decide Flash, the low-latency Solar: text only",
        noul_only: false,
        images: false,
    },
    Model {
        id: "togethercomputer/tev1-4b-experimental",
        about: "Tev1 4B, Together AI's experiment: text only",
        noul_only: false,
        images: false,
    },
    Model {
        id: "scaledown/classify-1",
        about: "ScaleDown's Classify 1: text only, and every Choice option needs a description",
        noul_only: false,
        images: false,
    },
];

/// Roughly how many tokens of a state these models read, where OpenRouter lists a context window of
/// 0. Measured: they take a state of any size, read its start and end, and skip the middle without
/// saying so. A fact in the middle of a long state scores as if it weren't there.
const MEASURED_CONTEXT: [(&str, u64); 3] = [("respan/span-01", 3000), ("respan/span-01-lite", 3000), ("respan/span-01-lite:free", 3000)];

/// The most questions these models take in one call, where it is fewer than the endpoint's own
/// limit. Measured: one more is refused with a 422, after the call.
const MAX_QUESTIONS: [(&str, usize); 3] = [("nace-ai/drex-max", 16), ("parallel/decisions-fast:free", 100), ("morph/reflex", 128)];

/// A model as it is on offer now: OpenRouter's listing, corrected by what this crate has checked.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelInfo {
    /// Its id, as a request names it.
    pub id: String,
    /// How many tokens it reads at once, if that is known.
    pub context: Option<u64>,
    /// Whether [`ModelInfo::context`] was measured by this crate rather than listed by OpenRouter.
    pub measured: bool,
    /// Dollars per million input tokens, if it is listed; 0 for a free model.
    pub price: Option<f64>,
    /// What a state can hold for it: `text`, `image`, and for some `audio`, `video` or `file`.
    pub inputs: Vec<String>,
    /// Whether it is one of [`MODELS`], the ones this crate is checked against.
    pub supported: bool,
}

/// One entry of OpenRouter's listing, as far as this crate reads it.
#[derive(Deserialize)]
struct Listed {
    id: String,
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
    fn info(&self, model: Option<&Model>) -> ModelInfo {
        let listed = self.context_length.filter(|context| *context > 0);
        let measured = MEASURED_CONTEXT.iter().find(|(id, _)| *id == self.id).map(|(_, tokens)| *tokens);
        let price = self.pricing.prompt.as_deref().and_then(|price| price.parse::<f64>().ok());
        let mut inputs = self.architecture.input_modalities.clone();
        // A call settles this where the listing and this crate differ: see the top of this file.
        inputs.retain(|input| input != "image" || model.is_none_or(|model| model.images));
        ModelInfo {
            id: self.id.clone(),
            context: listed.or(measured),
            measured: listed.is_none() && measured.is_some(),
            // To the millionth of a dollar: multiplying leaves 0.042 as 0.042000000000000004. A
            // price below zero is OpenRouter's way of saying it varies.
            price: price.filter(|price| *price >= 0.0).map(|price| (price * 1e12).round() / 1e6),
            inputs,
            supported: model.is_some(),
        }
    }
}

/// OpenRouter's listing ([`crate::MODELS_URL`]) as [`ModelInfo`]s: [`MODELS`] in their order, and
/// then any model the listing has that they don't. One the listing has dropped is kept, with what
/// this crate knows of it and neither a price nor a context window.
pub fn read_listing(body: &[u8]) -> crate::Result<Vec<ModelInfo>> {
    #[derive(Deserialize)]
    struct Listing {
        data: Vec<Listed>,
    }
    let Listing { data } = serde_json::from_slice(body).map_err(|e| Error::Decode(format!("the models list: {e}")))?;
    let known = |one: &Model| match data.iter().find(|listed| listed.id == one.id) {
        Some(listed) => listed.info(Some(one)),
        None => ModelInfo {
            id: one.id.to_owned(),
            context: None,
            measured: false,
            price: None,
            inputs: if one.images { vec!["text".to_owned(), "image".to_owned()] } else { vec!["text".to_owned()] },
            supported: true,
        },
    };
    let mut models: Vec<ModelInfo> = MODELS.iter().map(known).collect();
    models.extend(data.iter().filter(|listed| model(&listed.id).is_none()).map(|listed| listed.info(None)));
    Ok(models)
}

/// The supported model with this id, if it is one.
pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|model| model.id == id)
}

/// Whether `request`'s model can answer each of its questions ([`Model::check`]) and read its state,
/// when it is one of [`MODELS`]; any other model is taken at its word. The first question it can't
/// is an [`Error::Invalid`], as is a question past the most it takes in a call, and images it can't
/// read an [`Error::Image`]. [`crate::Client`] checks
/// this where it builds a request and again where it sends one, and the CLI where it builds one, so
/// a dry run finds what sending would.
pub fn check_request(request: &DecisionRequest) -> crate::Result<()> {
    let Some(model) = model(&request.model) else { return Ok(()) };
    if !model.images && crate::has_images(&request.state) {
        return Err(Error::Image(format!("{} reads text only, and this state has images in it", model.id)));
    }
    for (at, (id, question)) in request.questions.iter().enumerate() {
        model.check(question).map_err(|why| Error::Invalid { id: id.clone(), why })?;
        if let Some((_, most)) = MAX_QUESTIONS.iter().find(|(limited, _)| *limited == model.id).filter(|(_, most)| at >= *most) {
            let why = format!(
                "{} takes at most {most} questions a call, and this request asks {}: split them",
                model.id,
                request.questions.len()
            );
            return Err(Error::Invalid { id: id.clone(), why });
        }
    }
    Ok(())
}

impl Model {
    /// Whether this model can answer `question`: every model answers every type, except one that
    /// is [`Model::noul_only`], which takes a yes/no question with plain-text instructions and
    /// criteria, and nothing else. The error says why, as [`crate::Error::Invalid`]'s `why`.
    pub fn check(&self, question: &Question) -> Result<(), String> {
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
    fn lists_each_model_once_with_the_default_first() {
        assert_eq!(MODELS[0].id, crate::DEFAULT_MODEL);
        for (at, one) in MODELS.iter().enumerate() {
            assert!(MODELS[..at].iter().all(|earlier| earlier.id != one.id), "{} twice", one.id);
        }
        assert_eq!(model("jaredpalmer/kev-4b").map(|model| model.noul_only), Some(false));
        assert!(model("someone/else").is_none());
    }

    #[test]
    fn says_in_its_line_what_a_model_cant_do() {
        // `jev --help` shows `about` alone, so a limit that isn't in it is one nobody is told about.
        for model in MODELS {
            assert_eq!(model.noul_only, model.about.contains("yes/no questions only"), "{}", model.id);
            assert_eq!(model.images, !model.about.contains("text only"), "{}", model.id);
        }
    }

    #[test]
    fn reads_the_listing_and_corrects_it() {
        let listing = json!({"data": [
            {"id": "someone/new", "context_length": 1024, "pricing": {"prompt": "0.0000005", "completion": "0"},
             "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "typesafe/jev-1.14", "context_length": 262144, "pricing": {"prompt": "0.000000042", "completion": "0"},
             "architecture": {"input_modalities": ["text", "image"]}},
            {"id": "scaledown/classify-1", "context_length": 1000000, "pricing": {"prompt": "0.00000004"},
             "architecture": {"input_modalities": ["text", "image", "file"]}},
            {"id": "cloudflare/clef-omni", "context_length": 65536, "pricing": {"prompt": "-1"},
             "architecture": {"input_modalities": ["text", "image", "audio", "video"]}},
            {"id": "respan/span-01", "context_length": 0, "pricing": {"prompt": "0.00000002"},
             "architecture": {"input_modalities": ["text"]}},
            {"id": "respan/span-01-lite:free", "context_length": 0, "pricing": {"prompt": "0"},
             "architecture": {"input_modalities": ["text"]}},
        ]});
        let models = read_listing(listing.to_string().as_bytes()).unwrap();
        let find = |id: &str| models.iter().find(|model| model.id == id).unwrap();

        // The supported models in their own order, the default first, and then the rest.
        assert_eq!(models.len(), MODELS.len() + 1);
        assert_eq!(models[0].id, crate::DEFAULT_MODEL);
        let new = &models[MODELS.len()];
        assert_eq!((new.id.as_str(), new.supported, new.context), ("someone/new", false, Some(1024)));
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

        // Gone from the listing, it is still supported, and nothing is made up about it.
        let gone = find("liquid/d1");
        assert_eq!((gone.context, gone.price, gone.supported), (None, None, true));
        assert_eq!(gone.inputs, ["text", "image"]);

        assert!(read_listing(b"<html>").unwrap_err().to_string().contains("the models list"));
    }

    #[test]
    fn a_listed_context_replaces_a_measured_one() {
        let listing = json!({"data": [{"id": "respan/span-01", "context_length": 8192}]});
        let models = read_listing(listing.to_string().as_bytes()).unwrap();
        let span = models.iter().find(|model| model.id == "respan/span-01").unwrap();
        assert_eq!((span.context, span.measured), (Some(8192), false));
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
        // A model this crate doesn't know is taken at its word.
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
        for (id, _) in MAX_QUESTIONS {
            assert!(model(id).is_some_and(|model| model.about.contains("questions a call")), "{id}");
        }
    }

    #[test]
    fn a_noul_only_model_takes_plain_yes_no_questions_alone() {
        let span = model("respan/span-01").unwrap();
        assert!(span.check(&Question::noul("Is it raining?")).is_ok());
        assert!(span.check(&Question::noul_with_criteria("Is it raining?", "Rain is falling", "It is dry")).is_ok());
        let score = span.check(&Question::score("How warm?", ["Cold", "Hot"])).unwrap_err();
        assert!(score.contains("respan/span-01 answers yes/no questions only") && score.contains("a score"), "{score}");
        assert!(span.check(&Question::choice("Which?", [("a", "A"), ("b", "B")])).unwrap_err().contains("a choice"));
        assert!(span.check(&Question::noul(json!({"what": "structured"}))).unwrap_err().contains("plain text"));
        assert!(span.check(&Question::noul_with_criteria("Is it?", json!(["yes"]), "no")).is_err());
        // Every other model takes every type.
        assert!(model("jaredpalmer/kev-4b").unwrap().check(&Question::score("How warm?", ["Cold", "Hot"])).is_ok());
    }
}
