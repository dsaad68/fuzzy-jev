//! The rules drawn as a picture, laid out as a fuzzy rule base is: a row per rule with its premises
//! (each question's levels or options and their probabilities, the term's own marked), the operators that
//! combine them, and its conclusion (an item's bar, or an output's set clipped at the rule's score),
//! then the final column (each item's score against the threshold, and each output's merged shape
//! with its centre).
//!
//! Without a reply it is the structure alone, which costs no call; with one, every part carries its
//! number. [`Rules::graph_text`] draws it for a terminal, [`Rules::graph_svg`] as an SVG image.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{Expr, Kind, Rules, Then};
use crate::DecisionResponse;

/// One node of a rule's `if`, with what it came to when there is a reply.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Node {
    /// `AND`, `OR`, `NOT`, a hedge, or the term's name.
    pub(super) label: String,
    /// How it is worked out: `min`, `1 − x`, `x²`; empty for a term.
    pub(super) how: &'static str,
    pub(super) value: Option<f64>,
    pub(super) children: Vec<Node>,
    /// The term, for a leaf.
    pub(super) term: Option<String>,
}

impl Node {
    fn of(expr: &Expr, rules: &Rules, degrees: Option<&BTreeMap<String, f64>>) -> Node {
        let unary = |label: &str, how, inner: &Expr, apply: &dyn Fn(f64) -> f64| {
            let child = Node::of(inner, rules, degrees);
            Node { label: label.to_owned(), how, value: child.value.map(apply), children: vec![child], term: None }
        };
        match expr {
            Expr::Term(term) => Node {
                label: term.clone(),
                how: "",
                value: degrees.map(|degrees| degrees[term]),
                children: Vec::new(),
                term: Some(term.clone()),
            },
            Expr::Not(inner) => unary("NOT", "1 − x", inner, &|x| 1.0 - x),
            Expr::Hedge(hedge, inner) => {
                let (label, how) = hedge.describe();
                unary(label, how, inner, &|x| hedge.apply(x))
            }
            Expr::And(..) | Expr::Or(..) => {
                // `a AND b AND c` parses as ((a AND b) AND c); it is drawn as one AND of three.
                let mut operands = Vec::new();
                flatten(expr, &mut operands);
                let children: Vec<Node> = operands.iter().map(|operand| Node::of(operand, rules, degrees)).collect();
                let (label, how) = match expr {
                    Expr::And(..) => ("AND", rules.logic.and.describe()),
                    _ => ("OR", rules.logic.or.describe()),
                };
                let values: Option<Vec<f64>> = children.iter().map(|child| child.value).collect();
                let value = values.map(|values| {
                    values[1..].iter().fold(values[0], |joined, &value| match expr {
                        Expr::And(..) => rules.logic.and.apply(joined, value),
                        _ => rules.logic.or.apply(joined, value),
                    })
                });
                Node { label: label.to_owned(), how, value, children, term: None }
            }
        }
    }

    /// The terms under it, in the order written.
    pub(super) fn terms(&self, out: &mut Vec<String>) {
        match &self.term {
            Some(term) => out.push(term.clone()),
            None => self.children.iter().for_each(|child| child.terms(out)),
        }
    }
}

/// The operands of a run of the same operator, left to right.
fn flatten<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    let (a, b) = match expr {
        Expr::And(a, b) | Expr::Or(a, b) => (a, b),
        other => return out.push(other),
    };
    for side in [a, b] {
        let same = matches!((expr, side.as_ref()), (Expr::And(..), Expr::And(..)) | (Expr::Or(..), Expr::Or(..)));
        if same {
            flatten(side, out);
        } else {
            out.push(side);
        }
    }
}

/// A rule laid out for drawing.
pub(super) struct Row {
    pub(super) number: usize,
    pub(super) text: String,
    pub(super) then: Then,
    pub(super) weight: f64,
    pub(super) tree: Node,
    /// The rule's score: the tree's value times the weight.
    pub(super) score: Option<f64>,
}

impl Rules {
    /// Every rule laid out for drawing, with numbers when there is a reply.
    pub(super) fn rows(&self, degrees: Option<&BTreeMap<String, f64>>) -> Vec<Row> {
        self.rules
            .iter()
            .enumerate()
            .map(|(at, rule)| {
                let tree = Node::of(&rule.when, self, degrees);
                let score = tree.value.map(|value| value * rule.weight);
                Row { number: at + 1, text: rule.text.clone(), then: rule.then.clone(), weight: rule.weight, tree, score }
            })
            .collect()
    }

    /// Every term's degree in `reply`.
    pub(super) fn degrees(&self, reply: &DecisionResponse) -> crate::Result<BTreeMap<String, f64>> {
        self.check(reply)?;
        self.terms.iter().map(|(name, target)| Ok((name.clone(), target.degree(reply)?))).collect()
    }

    /// What a rule concludes, as written.
    pub(super) fn then_text(&self, then: &Then) -> String {
        match then {
            Then::Item(name) => name.clone(),
            Then::Output { output, set } => format!("{} IS {}", self.outputs[*output].name, self.outputs[*output].sets[*set].name),
        }
    }

    /// The rules for a terminal: each rule as a tree of its operators down to its terms, what it
    /// concludes, and then the final scores and each output's merged shape. With `reply`, every node
    /// carries its number; without it, this is the structure alone. Ends with a newline.
    pub fn graph_text(&self, reply: Option<&DecisionResponse>) -> crate::Result<String> {
        let degrees = reply.map(|reply| self.degrees(reply)).transpose()?;
        let outcome = reply.map(|reply| self.evaluate(reply)).transpose()?;
        let rows = self.rows(degrees.as_ref());
        let mut out = String::new();
        for row in &rows {
            let _ = writeln!(out, "R{}  {}  ⇒  {}", row.number, row.text, self.then_text(&row.then));
            self.tree_lines(&row.tree, reply, "    ", "    ", &mut out);
            let weight = if row.weight < 1.0 { format!(" (× weight {})", row.weight) } else { String::new() };
            let _ = match (&row.then, row.score) {
                (Then::Item(name), Some(score)) => writeln!(out, "    ⇒ {name}  {score:.2}{weight}  {}", bar(score, 20)),
                (Then::Item(name), None) => writeln!(out, "    ⇒ {name}{weight}"),
                (then @ Then::Output { .. }, Some(score)) => {
                    writeln!(out, "    ⇒ {}, clipped at {score:.2}{weight}", self.then_text(then))
                }
                (then @ Then::Output { .. }, None) => writeln!(out, "    ⇒ {}, clipped at the rule's score{weight}", self.then_text(then)),
            };
            out.push('\n');
        }

        let items: Vec<String> = self.item_names();
        if !items.is_empty() || !self.outputs.is_empty() {
            out.push_str("Final\n");
        }
        let width = items.iter().map(|item| item.chars().count()).max().unwrap_or(0);
        for name in &items {
            let by: Vec<String> = rows
                .iter()
                .filter(|row| matches!(&row.then, Then::Item(item) if item == name))
                .map(|row| format!("R{}", row.number))
                .collect();
            let _ = match outcome.as_ref().and_then(|outcome| outcome.items.iter().find(|item| &item.item == name)) {
                Some(item) => {
                    writeln!(out, "  {name:width$}  {:.2}  {}{}", item.score, bar(item.score, 20), if item.yes { "  yes" } else { "" })
                }
                None => writeln!(out, "  {name:width$}  from {}", by.join(" OR ")),
            };
        }
        if !items.is_empty() {
            let _ = writeln!(out, "  threshold {:.2}", self.threshold);
        }
        for (at, output) in self.outputs.iter().enumerate() {
            if at > 0 || !items.is_empty() {
                out.push('\n');
            }
            let scores: Option<Vec<f64>> = rows.iter().map(|row| row.score).collect();
            let clipped = scores.as_ref().map(|scores| self.set_scores(at, scores));
            let value = clipped.as_ref().and_then(|clipped| output.centroid(clipped, self.logic.or));
            let _ = match (&clipped, value) {
                (Some(_), Some(value)) => writeln!(out, "  {} = {value:.2}", output.name),
                (Some(_), None) => writeln!(out, "  {} = -  (no rule fired)", output.name),
                (None, _) => writeln!(out, "  {}  [{}, {}]", output.name, output.range.0, output.range.1),
            };
            self.plot(at, clipped.as_deref(), value, &mut out);
        }
        Ok(out)
    }

    /// Every item a `then` names, in the order the file first names it.
    pub(super) fn item_names(&self) -> Vec<String> {
        let mut items: Vec<String> = Vec::new();
        for rule in &self.rules {
            if let Then::Item(name) = &rule.then {
                if !items.contains(name) {
                    items.push(name.clone());
                }
            }
        }
        items
    }

    /// `node` and what is under it, as a tree drawn with box lines.
    fn tree_lines(&self, node: &Node, reply: Option<&DecisionResponse>, first: &str, rest: &str, out: &mut String) {
        let how = if node.how.is_empty() { String::new() } else { format!(" ({})", node.how) };
        let value = node.value.map(|value| format!("  {value:.2}")).unwrap_or_default();
        let source = match &node.term {
            Some(term) => format!("  ← {}", self.source(term, reply)),
            None => String::new(),
        };
        let _ = writeln!(out, "{first}{}{how}{value}{source}", node.label);
        for (at, child) in node.children.iter().enumerate() {
            let last = at + 1 == node.children.len();
            let (branch, under) = if last { ("└─ ", "   ") } else { ("├─ ", "│  ") };
            self.tree_lines(child, reply, &format!("{rest}{branch}"), &format!("{rest}{under}"), out);
        }
    }

    /// Where a term's degree comes from, and with a reply every level's or option's probability,
    /// the term's own marked: `temp: Cold 0.04 · Mild 0.95 · [Hot 0.01]`.
    fn source(&self, term: &str, reply: Option<&DecisionResponse>) -> String {
        let target = &self.terms[term];
        let probabilities = reply.and_then(|reply| target.probabilities(reply));
        let labels: Vec<String> = target
            .labels
            .iter()
            .enumerate()
            .map(|(at, label)| {
                let shown = match probabilities.as_ref().map(|probabilities| probabilities[at]) {
                    Some(Some(probability)) => format!("{label} {probability:.2}"),
                    Some(None) => format!("{label} missing"),
                    None => label.clone(),
                };
                if at == target.selected {
                    format!("[{shown}]")
                } else {
                    shown
                }
            })
            .collect();
        format!("{}: {}", target.id, labels.join(" · "))
    }

    /// An output's range as a plot: the merged shape filled in blocks, every set's outline shaded
    /// behind it, the sets' names under their peaks, and `↑` at the centre.
    fn plot(&self, at: usize, clipped: Option<&[f64]>, value: Option<f64>, out: &mut String) {
        const WIDTH: usize = 60;
        const HEIGHT: usize = 5;
        const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let output = &self.outputs[at];
        let xs: Vec<f64> = output.samples(WIDTH).collect();
        let outline: Vec<f64> = xs.iter().map(|x| output.sets.iter().map(|set| set.membership(*x)).fold(0.0, f64::max)).collect();
        let merged: Vec<f64> = xs.iter().map(|x| clipped.map_or(0.0, |clipped| output.merged(clipped, self.logic.or, *x))).collect();
        for line in (0..HEIGHT).rev() {
            let axis = match line {
                l if l == HEIGHT - 1 => "  1.0 ┤",
                _ => "      │",
            };
            let mut text = axis.to_owned();
            for (&shape, &merged) in outline.iter().zip(&merged) {
                let fill = ((merged * HEIGHT as f64 - line as f64) * 8.0).round().clamp(0.0, 8.0) as usize;
                let cell = if fill > 0 {
                    EIGHTHS[fill]
                } else if shape * HEIGHT as f64 - line as f64 >= 0.5 {
                    '░'
                } else {
                    ' '
                };
                text.push(cell);
            }
            let _ = writeln!(out, "{}", text.trim_end());
        }
        let column_of = |x: f64| (((x - output.range.0) / (output.range.1 - output.range.0)) * (WIDTH - 1) as f64).round() as usize;
        let mut axis: Vec<char> = "─".repeat(WIDTH).chars().collect();
        if let Some(value) = value {
            axis[column_of(value).min(WIDTH - 1)] = '┬';
        }
        let _ = writeln!(out, "  0.0 └{}", axis.into_iter().collect::<String>());
        // The range's ends and the centre, then the sets' names, each placed where there is room.
        let mut under = vec![' '; WIDTH + 12];
        let place = |line: &mut Vec<char>, column: usize, text: &str| {
            let start = column.min(line.len().saturating_sub(text.chars().count()));
            if line[start..(start + text.chars().count()).min(line.len())].iter().all(|cell| *cell == ' ') {
                for (offset, character) in text.chars().enumerate() {
                    if let Some(cell) = line.get_mut(start + offset) {
                        *cell = character;
                    }
                }
            }
        };
        if let Some(value) = value {
            place(&mut under, column_of(value), &format!("↑ {value:.2}"));
        }
        place(&mut under, 0, &trim_number(output.range.0));
        let high = trim_number(output.range.1);
        place(&mut under, WIDTH - high.chars().count(), &high);
        let _ = writeln!(out, "       {}", under.into_iter().collect::<String>().trim_end());
        let mut names = vec![' '; WIDTH + 12];
        for set in &output.sets {
            let column = column_of(set.middle()).saturating_sub(set.name.chars().count() / 2);
            place(&mut names, column, &set.name);
        }
        let _ = writeln!(out, "       {}", names.into_iter().collect::<String>().trim_end());
    }
}

/// A number without a needless `.0`.
pub(super) fn trim_number(number: f64) -> String {
    if number.fract() == 0.0 {
        format!("{number:.0}")
    } else {
        format!("{number}")
    }
}

/// `score` as a bar `width` cells long, to an eighth of a cell.
fn bar(score: f64, width: usize) -> String {
    const EIGHTHS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = (score.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    format!("{}{}", "█".repeat(eighths / 8), EIGHTHS[eighths % 8])
}

impl super::Target {
    /// Every level's or option's probability in `reply`, in the order of `labels`, and `None` for
    /// one the reply leaves out: a drawing that showed it as 0 would make a gap look like a
    /// confident no. For a Noul, no and yes.
    pub(super) fn probabilities(&self, reply: &DecisionResponse) -> Option<Vec<Option<f64>>> {
        match self.kind {
            Kind::Noul => reply.noul(&self.id).ok().map(|yes| vec![Some(1.0 - yes), Some(yes)]),
            Kind::Score => {
                let answer = reply.score(&self.id).ok()?;
                Some((0..self.labels.len()).map(|at| u8::try_from(at).ok().and_then(|at| answer.probabilities.get(&at)).copied()).collect())
            }
            Kind::Choice => {
                let answer = reply.choice(&self.id).ok()?;
                Some(self.labels.iter().map(|label| answer.probabilities.get(label).copied()).collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::Rules;
    use crate::{DecisionResponse, Question};

    const RULES: &str = r#"
        [terms]
        hot = "temp.Hot"
        raining = "raining"
        regular = "rain.Regular"

        [output.water]
        range = [0, 10]
        little = [0, 0, 2, 5]
        lots = [5, 8, 10, 10]

        [[rule]]
        if = "raining AND NOT VERY hot"
        then = "raincoat"

        [[rule]]
        if = "regular"
        then = "water IS little"
    "#;

    fn rules() -> Rules {
        let questions = [
            ("temp".to_owned(), Question::score("How warm?", ["Cold", "Mild", "Hot"])),
            ("raining".to_owned(), Question::noul("Raining?")),
            ("rain".to_owned(), Question::score("How much rain?", ["Scarce", "Regular"])),
        ];
        Rules::parse(RULES, questions.iter().map(|(id, question)| (id.as_str(), question))).unwrap()
    }

    fn reply() -> DecisionResponse {
        serde_json::from_value(json!({
            "model": "typesafe/jev-1.13-20260917",
            "answers": {
                "temp": {"type": "score", "score": 1.4, "confidence": 0.5, "probabilities": {"0": 0.0, "1": 0.6, "2": 0.4}},
                "raining": {"type": "noul", "noul": 0.9},
                "rain": {"type": "score", "score": 0.8, "confidence": 0.6, "probabilities": {"0": 0.2, "1": 0.8}},
            },
            "usage": {"input_tokens": 1, "output_tokens": 1},
        }))
        .unwrap()
    }

    #[test]
    fn draws_the_structure_without_a_reply() {
        let text = rules().graph_text(None).unwrap();
        let expected = "\
R1  raining AND NOT VERY hot  ⇒  raincoat
    AND (min)
    ├─ raining  ← raining: no · [yes]
    └─ NOT (1 − x)
       └─ VERY (x²)
          └─ hot  ← temp: Cold · Mild · [Hot]
    ⇒ raincoat
";
        assert!(text.starts_with(expected), "{text}");
        assert!(text.contains("R2  regular  ⇒  water IS little\n"), "{text}");
        assert!(text.contains("  raincoat  from R1\n  threshold 0.50\n"), "{text}");
        assert!(text.contains("  water  [0, 10]\n"), "{text}");
    }

    #[test]
    fn draws_every_number_with_a_reply() {
        let text = rules().graph_text(Some(&reply())).unwrap();
        // VERY 0.4 = 0.16, NOT that = 0.84, AND with 0.9 = 0.84.
        assert!(text.contains("    AND (min)  0.84\n"), "{text}");
        assert!(text.contains("       └─ VERY (x²)  0.16\n"), "{text}");
        assert!(text.contains("hot  0.40  ← temp: Cold 0.00 · Mild 0.60 · [Hot 0.40]"), "{text}");
        assert!(text.contains("    ⇒ water IS little, clipped at 0.80\n"), "{text}");
        assert!(text.contains("  raincoat  0.84  ████████████████▊  yes\n"), "{text}");
        let value = rules().evaluate(&reply()).unwrap().outputs[0].value.unwrap();
        assert!(text.contains(&format!("  water = {value:.2}\n")), "{text}");
        assert!(text.contains(&format!("↑ {value:.2}")), "{text}");
    }

    #[test]
    fn draws_an_svg_with_a_row_per_rule() {
        let svg = rules().graph_svg(Some(&reply())).unwrap();
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.trim_end().ends_with("</svg>"));
        for text in
            ["Premises", "Conclusions", "Final", ">R1<", ">R2<", "if raining AND NOT VERY hot  ⇒  raincoat", "items (threshold 0.50)"]
        {
            assert!(svg.contains(text), "{text} is missing");
        }
        let value = rules().evaluate(&reply()).unwrap().outputs[0].value.unwrap();
        assert!(svg.contains(&format!("water = {value:.2}")));
        // Every element that opens is closed, and each rule is a group a page can find.
        assert_eq!(svg.matches("<text").count(), svg.matches("</text>").count());
        assert_eq!(svg.matches("<g ").count(), svg.matches("</g>").count());
        assert!(svg.contains("<g class=\"rule\" data-rule=\"1\"") && svg.contains("data-then=\"raincoat\""));
        // The structure alone draws no numbers and says why.
        let structure = rules().graph_svg(None).unwrap();
        assert!(structure.contains("Structure only") && !structure.contains("water ="));
    }

    #[test]
    fn draws_a_missing_probability_as_missing() {
        // `rain` gives nothing for Scarce; the term reads Regular, so the rules still evaluate.
        let mut reply = reply();
        let crate::Answer::Score(rain) = reply.answers.get_mut("rain").unwrap() else { unreachable!() };
        rain.probabilities = [(1, 1.0)].into();
        let text = rules().graph_text(Some(&reply)).unwrap();
        assert!(text.contains("rain: Scarce missing · [Regular 1.00]"), "{text}");
        let svg = rules().graph_svg(Some(&reply)).unwrap();
        assert!(svg.contains(">missing<"));
        // A Score's expected level is its own marker, not a reading of the levels.
        assert!(svg.contains(">expected 1.40<"), "the temp marker is missing");
    }

    #[test]
    fn a_missing_answer_is_an_error() {
        let mut reply = reply();
        reply.answers.remove("raining");
        assert!(rules().graph_text(Some(&reply)).is_err());
        assert!(rules().graph_svg(Some(&reply)).is_err());
    }
}
