//! The models on the decisions endpoint that this crate supports, and what each of them answers.
//!
//! Any model id can be sent ([`crate::Client::with_model`]); these are the ones this crate is
//! checked against. Some answer only yes/no questions, and a request asking one of them anything
//! else is refused before it is sent ([`Model::check`]), rather than costing a call that fails.
//! Some read no images, and a state with images in it is refused the same way.
//!
//! What each one takes comes from calling it, not from OpenRouter's listing, which says
//! `scaledown/classify-1` reads images when it refuses them.

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
pub const MODELS: [Model; 33] = [
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
    Model { id: "baseten/mammoth-1", about: "Mammoth 1, Baseten's largest, 27B", noul_only: false, images: true },
    Model { id: "baseten/elephant-1", about: "Elephant 1, from Baseten, 9B", noul_only: false, images: true },
    Model { id: "baseten/joey-1", about: "Joey 1, Baseten's smallest, 2B", noul_only: false, images: true },
    Model { id: "cloudflare/clef", about: "Cloudflare's Clef, 27B", noul_only: false, images: true },
    Model { id: "cloudflare/clef-flash", about: "Clef Flash, the fast Clef, 9B", noul_only: false, images: true },
    Model { id: "cloudflare/clef-omni", about: "Clef Omni, the mixture-of-experts Clef", noul_only: false, images: true },
    Model { id: "nace-ai/drex-max", about: "Drex Max, Nace.AI's largest", noul_only: false, images: true },
    Model { id: "nace-ai/drex-v2.0", about: "Drex v2.0, from Nace.AI: text only", noul_only: false, images: false },
    Model { id: "nace-ai/drex-v1.5", about: "Drex v1.5, from Nace.AI: text only", noul_only: false, images: false },
    Model { id: "openai/gpt-6-luna-decisions", about: "GPT-6 Luna, served for decisions", noul_only: false, images: true },
    Model { id: "inception/mercury-decide", about: "Inception's Mercury Decide", noul_only: false, images: true },
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

/// The supported model with this id, if it is one.
pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|model| model.id == id)
}

/// Whether `request`'s model can answer each of its questions ([`Model::check`]) and read its state,
/// when it is one of [`MODELS`]; any other model is taken at its word. The first question it can't
/// is an [`Error::Invalid`], and images it can't read an [`Error::Image`]. [`crate::Client`] checks
/// this where it builds a request and again where it sends one, and the CLI where it builds one, so
/// a dry run finds what sending would.
pub fn check_request(request: &DecisionRequest) -> crate::Result<()> {
    let Some(model) = model(&request.model) else { return Ok(()) };
    if !model.images && crate::has_images(&request.state) {
        return Err(Error::Image(format!("{} reads text only, and this state has images in it", model.id)));
    }
    for (id, question) in &request.questions {
        model.check(question).map_err(|why| Error::Invalid { id: id.clone(), why })?;
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
