//! The models on the decisions endpoint that this crate supports, and what each of them answers.
//!
//! Any model id can be sent ([`crate::Client::with_model`]); these are the ones this crate is
//! checked against. Some answer only yes/no questions, and a request asking one of them anything
//! else is refused before it is sent ([`Model::check`]), rather than costing a call that fails.

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
}

/// The supported models, the default ([`crate::DEFAULT_MODEL`]) first.
pub const MODELS: [Model; 6] = [
    Model { id: "~typesafe/jev-latest", about: "TypeSafe's Jev, whichever version is the latest; the default", noul_only: false },
    Model { id: "typesafe/jev-1.13", about: "TypeSafe's Jev 1.13, pinned: the same model until you change it", noul_only: false },
    Model { id: "jaredpalmer/kev-4b", about: "Kev 4B, from jaredpalmer", noul_only: false },
    Model { id: "respan/span-01", about: "Respan's Span 01: yes/no questions only", noul_only: true },
    Model { id: "respan/span-01-lite", about: "Span 01 Lite, the lighter Span: yes/no questions only", noul_only: true },
    Model {
        id: "respan/span-01-lite:free",
        about: "Span 01 Lite, OpenRouter's free variant of it: yes/no questions only",
        noul_only: true,
    },
];

/// The supported model with this id, if it is one.
pub fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|model| model.id == id)
}

/// Whether `request`'s model can answer each of its questions ([`Model::check`]), when it is one of
/// [`MODELS`]; any other model is taken at its word. The first question it can't is an
/// [`Error::Invalid`]. [`crate::Client`] checks this where it builds a request and again where it
/// sends one, and the CLI where it builds one, so a dry run finds what sending would.
pub fn check_request(request: &DecisionRequest) -> crate::Result<()> {
    let Some(model) = model(&request.model) else { return Ok(()) };
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
