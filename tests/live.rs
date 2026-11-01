//! A real call to OpenRouter. Ignored by a plain `cargo test`, so it works offline; run it with
//! `cargo test -- --ignored`. It passes without calling anything when `OPENROUTER_API_KEY` isn't set.

#![cfg(not(target_arch = "wasm32"))]

use jev::{image_part, text_part, Client, Question};
use serde_json::json;

/// The image has to reach the model, not only the request: Liquid's models, sent an image in
/// `assets` as the whole state, answer as confidently as ever without having seen it. Solid red, so
/// only a model that saw it says red, and an option list where a guess is usually wrong.
#[tokio::test]
#[ignore = "calls OpenRouter"]
async fn sees_an_image_beside_text() {
    let Ok(key) = std::env::var("OPENROUTER_API_KEY") else {
        eprintln!("OPENROUTER_API_KEY is not set: skipped");
        return;
    };
    let state = json!([text_part("A note from Marta in Lisbon."), image_part(include_bytes!("fixtures/red.png")).unwrap()]);
    for model in ["~typesafe/jev-latest", "~liquid/d1-latest", "~perplexity/pplx-decider-latest"] {
        let reply = Client::new(&key)
            .with_model(model)
            .decide(
                state.clone(),
                [
                    ("colour", Question::choice("What colour is the image?", [("red", ""), ("green", ""), ("blue", ""), ("yellow", "")])),
                    ("city", Question::noul("Does the text name the city of Lisbon?")),
                ],
            )
            .await
            .unwrap();
        assert_eq!(reply.choice("colour").unwrap().choice, "red", "{model}");
        assert!(reply.noul("city").unwrap() > 0.5, "{model}");
    }
}

#[tokio::test]
#[ignore = "calls OpenRouter"]
async fn answers_every_question_type() {
    let Ok(key) = std::env::var("OPENROUTER_API_KEY") else {
        eprintln!("OPENROUTER_API_KEY is not set: skipped");
        return;
    };
    let reply = Client::new(&key)
        .decide(
            "Help! My payouts have been failing for 3 days.",
            [
                ("is_urgent", Question::noul("Does this message convey urgency?")),
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
            ],
        )
        .await
        .unwrap();

    assert!((0.0..=1.0).contains(&reply.noul("is_urgent").unwrap()));
    let department = reply.choice("department").unwrap();
    assert!(["billing", "technical", "sales"].contains(&department.choice.as_str()));
    let frustration = reply.score("frustration").unwrap();
    assert!((0.0..=2.0).contains(&frustration.score));
    assert_eq!(frustration.probabilities.len(), 3);
}
