//! A reply in the format asked for: text or a table for a person, with the questions in the order
//! they were asked and then what the call used, or JSON for scripts.

use std::fmt::Write;

use crate::rules::Outcome;
use crate::{Answer, DecisionResponse};
use serde_json::{json, Value};
use unicode_width::UnicodeWidthStr;

/// How the reply is printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// One line per question.
    Text,
    /// A table with a column per field.
    Table,
    /// The reply as this crate reads it (the fields it knows, re-encoded), for scripts.
    Json,
}

/// `reply`'s answers to `ids`, in that order, in `format`. A table is kept within `width`
/// columns. Ends with a newline.
pub fn render(format: Format, reply: &DecisionResponse, ids: &[String], width: usize) -> serde_json::Result<String> {
    Ok(match format {
        Format::Text => text(reply, ids),
        Format::Table => table(reply, ids, width),
        Format::Json => serde_json::to_string_pretty(reply)? + "\n",
    })
}

/// What rules made of `reply`, in `format`: a line or a row per item, or `{"reply", "outcome"}` as
/// JSON so a script still has every answer. A table is kept within `width` columns. Ends with a
/// newline.
pub fn render_outcome(format: Format, reply: &DecisionResponse, outcome: &Outcome, width: usize) -> serde_json::Result<String> {
    Ok(match format {
        Format::Text => outcome.text() + &usage(reply),
        Format::Table => outcome_table(outcome, width) + &usage(reply),
        Format::Json => serde_json::to_string_pretty(&json!({"reply": reply, "outcome": outcome}))? + "\n",
    })
}

/// One answer's cells: its type, the answer, its confidence, and every option's probability.
struct Row {
    kind: &'static str,
    answer: String,
    confidence: Option<f64>,
    probabilities: String,
}

impl Row {
    fn of(answer: Option<&Answer>) -> Row {
        match answer {
            None => Row { kind: "-", answer: "no answer".to_owned(), confidence: None, probabilities: String::new() },
            Some(Answer::Noul(answer)) => Row {
                kind: "noul",
                answer: if answer.noul >= 0.5 { "yes" } else { "no" }.to_owned(),
                confidence: None,
                probabilities: format!("yes {:.2}", answer.noul),
            },
            Some(Answer::Choice(answer)) => {
                let mut options: Vec<_> = answer.probabilities.iter().collect();
                options.sort_by(|a, b| b.1.total_cmp(a.1).then_with(|| a.0.cmp(b.0)));
                let options: Vec<_> = options.iter().map(|(name, p)| format!("{name} {p:.2}")).collect();
                Row {
                    kind: "choice",
                    answer: answer.choice.clone(),
                    confidence: Some(answer.confidence),
                    probabilities: options.join(", "),
                }
            }
            Some(Answer::Score(answer)) => {
                let top = answer.probabilities.keys().max().copied().unwrap_or(0);
                let nearest = answer.score.round().clamp(0.0, f64::from(top)) as u8;
                let described = match answer.legend.get(&nearest) {
                    Some(level) => format!(", nearest {}", level_text(level)),
                    None => String::new(),
                };
                let levels: Vec<_> = answer
                    .probabilities
                    .iter()
                    .map(|(level, p)| match answer.legend.get(level) {
                        Some(Value::String(name)) => format!("{name} {p:.2}"),
                        _ => format!("{level} {p:.2}"),
                    })
                    .collect();
                Row {
                    kind: "score",
                    answer: format!("{:.2} of {top}{described}", answer.score),
                    confidence: Some(answer.confidence),
                    probabilities: levels.join(", "),
                }
            }
            Some(other @ Answer::Other(_)) => Row {
                kind: "other",
                answer: format!("{}: a type this version doesn't know (see --json)", other.kind()),
                confidence: None,
                probabilities: String::new(),
            },
        }
    }

    fn confidence(&self) -> String {
        self.confidence.map_or_else(|| "-".to_owned(), |confidence| format!("{confidence:.2}"))
    }
}

/// One line per question: `id  answer  confidence  (probabilities)`.
fn text(reply: &DecisionResponse, ids: &[String]) -> String {
    let width = ids.iter().map(|id| columns(id)).max().unwrap_or(0);
    let mut out = String::new();
    for id in ids {
        let row = Row::of(reply.answers.get(id));
        let mut line = match reply.answers.get(id) {
            // A noul's answer is its probability; yes or no alone would hide it.
            Some(Answer::Noul(answer)) => format!("{:.2} {}", answer.noul, row.answer),
            _ => row.answer.clone(),
        };
        if row.confidence.is_some() {
            let _ = write!(line, "  confidence {}", row.confidence());
        }
        if row.kind == "choice" {
            let _ = write!(line, "  ({})", row.probabilities);
        }
        let _ = writeln!(out, "{}  {line}", pad(id, width));
    }
    out + &usage(reply)
}

/// The columns, and the order they are dropped in when the table doesn't fit: the probabilities
/// first, since the answer already summarizes them, then the type, which the answer implies.
const COLUMNS: [&str; 5] = ["question", "type", "answer", "confidence", "probabilities"];
const DROP_ORDER: [usize; 3] = [4, 1, 3];

/// Which column gives up space first, for the same reason: the question ids and their answers are
/// what the table is for, so they keep their width while the rest wraps.
const SQUEEZE_ORDER: [usize; 5] = [4, 3, 1, 2, 0];

/// A boxed table: question, type, answer, confidence, probabilities, within `width` columns.
/// Cells wrap, and columns come out when wrapping alone would leave them unreadable.
fn table(reply: &DecisionResponse, ids: &[String], width: usize) -> String {
    let mut rows = vec![COLUMNS.map(String::from).to_vec()];
    for id in ids {
        let row = Row::of(reply.answers.get(id));
        let confidence = row.confidence();
        rows.push(vec![id.clone(), row.kind.to_owned(), row.answer, confidence, row.probabilities]);
    }
    boxed(rows, &DROP_ORDER, &SQUEEZE_ORDER, width) + &usage(reply)
}

/// An outcome's columns. The rules behind each score come out first when the table doesn't fit,
/// and give up space first: the item and whether it is a yes are what the table is for.
const OUTCOME_COLUMNS: [&str; 4] = ["item", "score", "yes?", "rules"];
const OUTCOME_DROP_ORDER: [usize; 1] = [3];
const OUTCOME_SQUEEZE_ORDER: [usize; 4] = [3, 2, 1, 0];

/// A boxed table of an outcome: each item, its score, whether it is a yes, and every rule that
/// gave it a score, with what that rule came to.
fn outcome_table(outcome: &Outcome, width: usize) -> String {
    let mut rows = vec![OUTCOME_COLUMNS.map(String::from).to_vec()];
    for item in &outcome.items {
        let rules: Vec<String> = item.rules.iter().map(|rule| format!("{} = {:.2}", rule.when, rule.score)).collect();
        let yes = if item.yes { "yes" } else { "" };
        rows.push(vec![item.item.clone(), format!("{:.2}", item.score), yes.to_owned(), rules.join("; ")]);
    }
    // An output's row: its value, and each set a rule concluded in, with those rules.
    for output in &outcome.outputs {
        let sets: Vec<String> = output
            .sets
            .iter()
            .filter(|set| !set.rules.is_empty())
            .map(|set| {
                let rules: Vec<String> = set.rules.iter().map(|rule| format!("{} = {:.2}", rule.when, rule.score)).collect();
                format!("{}: {}", set.set, rules.join(", "))
            })
            .collect();
        rows.push(vec![output.output.clone(), output.value_text(), String::new(), sets.join("; ")]);
    }
    let threshold = if outcome.items.is_empty() { String::new() } else { format!("threshold {:.2}\n", outcome.threshold) };
    boxed(rows, &OUTCOME_DROP_ORDER, &OUTCOME_SQUEEZE_ORDER, width) + &threshold
}

/// `rows`, the first of them the header, in a boxed table within `width` columns. When it doesn't
/// fit, the columns in `drop_order` come out until every one left fits at its narrowest, and then
/// the ones in `squeeze_order` give up space in turn.
fn boxed(mut rows: Vec<Vec<String>>, drop_order: &[usize], squeeze_order: &[usize], width: usize) -> String {
    let count = rows[0].len();
    let natural = |rows: &[Vec<String>], column: usize| rows.iter().map(|row| columns(&row[column])).max().unwrap_or(0);
    // A column wraps down to its longest word, which is as narrow as it gets without cutting a
    // word in half. Question ids are one word, so they stay whole.
    let wrapped_to =
        |rows: &[Vec<String>], column: usize| rows.iter().flat_map(|row| row[column].split_whitespace()).map(columns).max().unwrap_or(0);

    let mut widths: Vec<usize> = (0..count).map(|column| natural(&rows, column)).collect();
    let mut narrowest: Vec<usize> = (0..count).map(|column| wrapped_to(&rows, column)).collect();

    // Drop columns until what's left fits with every column at its narrowest, then share the rest.
    for &column in drop_order {
        if fits(&narrowest, width) {
            break;
        }
        widths[column] = 0;
        narrowest[column] = 0;
        for row in &mut rows {
            row[column] = String::new();
        }
    }
    let kept: Vec<usize> = (0..count).filter(|column| widths[*column] > 0).collect();
    let order: Vec<usize> = squeeze_order.iter().filter_map(|column| kept.iter().position(|kept| kept == column)).collect();
    let narrowest: Vec<usize> = kept.iter().map(|column| narrowest[*column]).collect();
    let mut widths: Vec<usize> = kept.iter().map(|column| widths[*column]).collect();
    let rows: Vec<Vec<String>> = rows.iter().map(|row| kept.iter().map(|column| row[*column].clone()).collect()).collect();

    // Squeeze the least important column down to its narrowest, then the next, until the table
    // fits. Below that, words have to be cut, so it only happens when nothing else is left.
    for floor in [&narrowest[..], &vec![1; widths.len()][..]] {
        for &column in &order {
            while !fits(&widths, width) && widths[column] > floor[column] {
                widths[column] -= 1;
            }
        }
    }
    // Whatever is left over goes to the columns that matter most, up to what they'd take anyway.
    for &column in order.iter().rev() {
        while widths[column] < natural(&rows, column) && fits_with(&widths, column, width) {
            widths[column] += 1;
        }
    }

    let wrapped: Vec<Vec<Vec<String>>> =
        rows.iter().map(|row| row.iter().zip(&widths).map(|(cell, width)| wrap(cell, *width)).collect()).collect();
    let rule = |left: &str, middle: &str, right: &str| {
        let lines: Vec<String> = widths.iter().map(|width| "─".repeat(width + 2)).collect();
        format!("{left}{}{right}\n", lines.join(middle))
    };
    let block = |cells: &Vec<Vec<String>>| {
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        let mut out = String::new();
        for line in 0..height {
            let empty = String::new();
            let cells: Vec<String> =
                cells.iter().zip(&widths).map(|(cell, width)| format!(" {} ", pad(cell.get(line).unwrap_or(&empty), *width))).collect();
            let _ = writeln!(out, "│{}│", cells.join("│"));
        }
        out
    };
    let mut out = rule("┌", "┬", "┐");
    out += &block(&wrapped[0]);
    out += &rule("├", "┼", "┤");
    for row in &wrapped[1..] {
        out += &block(row);
    }
    out += &rule("└", "┴", "┘");
    out
}

/// Whether the table fits with one more column of space for `column`.
fn fits_with(widths: &[usize], column: usize, width: usize) -> bool {
    let mut widths = widths.to_vec();
    widths[column] += 1;
    fits(&widths, width)
}

/// Whether a table of these columns fits: a border, and each column padded by a space on each side.
fn fits(widths: &[usize], width: usize) -> bool {
    let columns: Vec<usize> = widths.iter().copied().filter(|width| *width > 0).collect();
    columns.iter().sum::<usize>() + 3 * columns.len() < width
}

/// How many terminal columns `text` takes. A CJK character or an emoji takes two, so counting
/// `char`s would leave a table's borders out of line.
fn columns(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// `text` in a cell `width` columns wide, with the spaces to fill it.
fn pad(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(columns(text))))
}

/// `text` in lines of at most `width` columns, split between words, and inside a word too long
/// for a line of its own.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let mut word = word;
        match lines.last_mut() {
            Some(line) if columns(line) + 1 + columns(word) <= width => {
                line.push(' ');
                line.push_str(word);
                continue;
            }
            _ => {}
        }
        // A word of its own is cut only when no line could ever hold it, at the last character
        // that still fits: a character is one or two columns wide, so the cut isn't a count.
        while columns(word) > width {
            let mut cut = word.len();
            let mut so_far = 0;
            for (at, character) in word.char_indices() {
                so_far += columns(character.encode_utf8(&mut [0; 4]));
                if so_far > width {
                    cut = at;
                    break;
                }
            }
            let cut = cut.max(word.chars().next().map_or(1, char::len_utf8));
            lines.push(word[..cut].to_owned());
            word = &word[cut..];
        }
        lines.push(word.to_owned());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// What the call used, and the model that answered.
pub fn usage(reply: &DecisionResponse) -> String {
    let usage = &reply.usage;
    let mut out = format!("{} tokens in, {} out", usage.input_tokens, usage.output_tokens);
    if let Some(cost) = usage.cost {
        let _ = write!(out, ", ${cost:.6}");
    }
    let _ = writeln!(out, ", {}", reply.model);
    out
}

/// A level's description: quoted text, or compact JSON when it's structured.
fn level_text(level: &Value) -> String {
    match level {
        Value::String(text) => format!("\"{text}\""),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (DecisionResponse, [String; 4]) {
        let reply = serde_json::from_str(include_str!("../tests/fixtures/decision.json")).unwrap();
        (reply, ["is_urgent", "department", "frustration", "missing"].map(String::from))
    }

    #[test]
    fn prints_text_in_the_order_asked() {
        let (reply, ids) = fixture();
        assert_eq!(
            render(Format::Text, &reply, &ids, 120).unwrap(),
            "\
is_urgent    0.95 yes
department   billing  confidence 0.82  (billing 0.88, technical 0.12, sales 0.00)
frustration  1.04 of 2, nearest \"Frustrated\"  confidence 0.94
missing      no answer
427 tokens in, 73 out, $0.000018, typesafe/jev-1.13-20260917
"
        );
    }

    #[test]
    fn prints_a_table() {
        let (reply, ids) = fixture();
        assert_eq!(
            render(Format::Table, &reply, &ids, 120).unwrap(),
            "\
┌─────────────┬────────┬─────────────────────────────────┬────────────┬─────────────────────────────────────────────┐
│ question    │ type   │ answer                          │ confidence │ probabilities                               │
├─────────────┼────────┼─────────────────────────────────┼────────────┼─────────────────────────────────────────────┤
│ is_urgent   │ noul   │ yes                             │ -          │ yes 0.95                                    │
│ department  │ choice │ billing                         │ 0.82       │ billing 0.88, technical 0.12, sales 0.00    │
│ frustration │ score  │ 1.04 of 2, nearest \"Frustrated\" │ 0.94       │ Calm 0.00, Frustrated 0.96, Very angry 0.04 │
│ missing     │ -      │ no answer                       │ -          │                                             │
└─────────────┴────────┴─────────────────────────────────┴────────────┴─────────────────────────────────────────────┘
427 tokens in, 73 out, $0.000018, typesafe/jev-1.13-20260917
"
        );
    }

    #[test]
    fn fits_a_table_to_a_narrow_terminal() {
        let (reply, ids) = fixture();
        for width in [30, 40, 60, 80, 120] {
            let table = render(Format::Table, &reply, &ids, width).unwrap();
            // The line about what the call used stands on its own, outside the table.
            let widest =
                table.lines().filter(|line| line.starts_with(['┌', '│', '├', '└'])).map(|line| line.chars().count()).max().unwrap();
            assert!(widest <= width, "{width} columns: a line of {widest}\n{table}");
            assert!(table.contains("question") && table.contains("answer"), "{width} columns dropped a column it should keep\n{table}");
        }
    }

    #[test]
    fn keeps_the_question_ids_whole_while_anything_else_can_give() {
        let (reply, ids) = fixture();
        for width in [30, 40, 60, 80] {
            let table = render(Format::Table, &reply, &ids, width).unwrap();
            for id in &ids {
                assert!(table.contains(id.as_str()), "{width} columns broke up `{id}`\n{table}");
            }
        }
    }

    #[test]
    fn measures_wide_characters_as_two_columns() {
        let mut reply: DecisionResponse = serde_json::from_str(include_str!("../tests/fixtures/decision.json")).unwrap();
        let answer = reply.answers.remove("department").unwrap();
        reply.answers.insert("緊急度".to_owned(), answer);
        let ids = ["緊急度".to_owned()];
        for width in [30, 40, 80] {
            let table = render(Format::Table, &reply, &ids, width).unwrap();
            let lines: Vec<&str> = table.lines().filter(|line| line.starts_with(['┌', '│', '├', '└'])).collect();
            let widest = lines.iter().map(|line| columns(line)).max().unwrap();
            assert!(widest <= width, "{width} columns: a line of {widest}\n{table}");
            // Every line of a table is the same width, or its borders don't line up.
            assert!(lines.iter().all(|line| columns(line) == widest), "{width} columns: ragged borders\n{table}");
        }
        assert_eq!(wrap("緊急度です", 4), ["緊急", "度で", "す"]);
    }

    #[test]
    fn wraps_words_and_cuts_only_what_cannot_fit() {
        assert_eq!(wrap("billing 0.88, technical 0.12", 14), ["billing 0.88,", "technical 0.12"]);
        assert_eq!(wrap("", 5), [""]);
        assert_eq!(wrap("unsplittable", 5), ["unspl", "ittab", "le"]);
    }

    /// The fixture's answers through two rules: an urgent, fairly frustrated billing ticket.
    fn outcome() -> (DecisionResponse, Outcome) {
        let (reply, _) = fixture();
        let questions = [
            ("is_urgent".to_owned(), crate::Question::noul("Urgent?")),
            ("department".to_owned(), crate::Question::choice("Team?", [("billing", ""), ("technical", ""), ("sales", "")])),
            ("frustration".to_owned(), crate::Question::score("Frustrated?", ["Calm", "Frustrated", "Very angry"])),
        ];
        let rules = crate::rules::Rules::parse(
            r#"
            [terms]
            urgent = "is_urgent"
            billing = "department.billing"
            angry = "frustration.Very angry"
            [[rule]]
            if = "urgent AND billing"
            then = "page billing"
            [[rule]]
            if = "VERY angry"
            then = "escalate"
            "#,
            questions.iter().map(|(id, question)| (id.as_str(), question)),
        )
        .unwrap();
        let outcome = rules.evaluate(&reply).unwrap();
        (reply, outcome)
    }

    #[test]
    fn prints_an_outcome() {
        let (reply, outcome) = outcome();
        assert_eq!(
            render_outcome(Format::Text, &reply, &outcome, 120).unwrap(),
            "\
page billing  0.88  yes
escalate      0.00
threshold 0.50
427 tokens in, 73 out, $0.000018, typesafe/jev-1.13-20260917
"
        );
        assert_eq!(
            render_outcome(Format::Table, &reply, &outcome, 120).unwrap(),
            "\
┌──────────────┬───────┬──────┬───────────────────────────┐
│ item         │ score │ yes? │ rules                     │
├──────────────┼───────┼──────┼───────────────────────────┤
│ page billing │ 0.88  │ yes  │ urgent AND billing = 0.88 │
│ escalate     │ 0.00  │      │ VERY angry = 0.00         │
└──────────────┴───────┴──────┴───────────────────────────┘
threshold 0.50
427 tokens in, 73 out, $0.000018, typesafe/jev-1.13-20260917
"
        );
        // Narrow, the rules come out and the rest stays whole.
        let narrow = render_outcome(Format::Table, &reply, &outcome, 34).unwrap();
        assert!(!narrow.contains("rules") && narrow.contains("page billing"), "{narrow}");
        // JSON keeps the reply, so a script loses nothing by asking for the outcome.
        let json: Value = serde_json::from_str(&render_outcome(Format::Json, &reply, &outcome, 120).unwrap()).unwrap();
        assert_eq!(json["reply"]["answers"]["is_urgent"]["noul"], 0.95);
        assert_eq!(json["outcome"]["items"][0]["item"], "page billing");
        assert_eq!(json["outcome"]["items"][0]["yes"], true);
    }

    #[test]
    fn prints_json_that_reads_back() {
        let (reply, ids) = fixture();
        let json = render(Format::Json, &reply, &ids, 120).unwrap();
        assert_eq!(serde_json::from_str::<DecisionResponse>(&json).unwrap(), reply);
        assert!(json.ends_with("}\n"));
    }
}
