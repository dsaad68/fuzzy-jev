//! A real call to OpenRouter. Ignored by a plain `cargo test`, so it works offline; run it with
//! `cargo test -- --ignored`. It passes without calling anything when `OPENROUTER_API_KEY` isn't set.

#![cfg(not(target_arch = "wasm32"))]

use jev::{Client, Question};

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
