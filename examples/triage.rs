//! Triage a support message: `cargo run --example triage -- "My payouts have been failing"`.
//! Needs `OPENROUTER_API_KEY`.

use jev::{Client, Question};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let message = std::env::args().nth(1).unwrap_or_else(|| "Help! My payouts have been failing for 3 days.".to_owned());
    let key = std::env::var("OPENROUTER_API_KEY").map_err(|_| "OPENROUTER_API_KEY is not set")?;

    let reply = Client::new(&key)
        .decide(
            &message,
            [
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
            ],
        )
        .await?;

    let department = reply.choice("department")?;
    let frustration = reply.score("frustration")?;
    println!("urgent:      {:.2}", reply.noul("is_urgent")?);
    println!("department:  {} (confidence {:.2})", department.choice, department.confidence);
    println!("frustration: {:.2} of 2 (confidence {:.2})", frustration.score, frustration.confidence);
    // Confidence as a second axis: act on a confident answer, send the rest to a person.
    if department.confidence < 0.6 {
        println!("-> unsure which team: a person should route this");
    }
    println!("{} tokens in, {} out, by {}", reply.usage.input_tokens, reply.usage.output_tokens, reply.model);
    Ok(())
}
