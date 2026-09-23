//! Outputs: a crisp axis with named fuzzy sets, for rules that conclude `OUTPUT IS SET` rather than an
//! item. Each such rule clips its set at its score, the clipped shapes are joined by the file's OR,
//! and the output's value is the centre of the shape that makes (Mamdani inference, centroid
//! defuzzification).
//!
//! ```toml
//! [output.irrigation]
//! range  = [0, 100]
//! drops  = [0, 0, 20, 40]     # a trapezoid: a, b, c, d
//! liter  = [30, 50, 70]       # a triangle: a, peak, c
//! gallon = [60, 80, 100, 100]
//! ```

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{is_term_name, Or, Then};

/// How many points along an output's range the centroid is taken over, and how many more across
/// each concluded set. Fine enough that the value is right to well under the two places it prints
/// with, however narrow a set is next to its range.
const SAMPLES: usize = 1001;
const PER_SET: usize = 200;

/// An `[output.NAME]` table as written: its range, and every other key a set.
#[derive(Deserialize)]
pub(super) struct OutputFile {
    range: Vec<f64>,
    #[serde(flatten)]
    sets: BTreeMap<String, Vec<f64>>,
}

/// An output, checked: its range and its sets in order along it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Output {
    pub(super) name: String,
    pub(super) range: (f64, f64),
    pub(super) sets: Vec<Set>,
}

/// A fuzzy set on an output's axis: a trapezoid `a, b, c, d`, rising from `a` to `b`, flat to `c`,
/// falling to `d`. A triangle is the trapezoid with `b` and `c` at its peak.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Set {
    pub(super) name: String,
    pub(super) points: [f64; 4],
}

impl Set {
    /// How much `x` belongs to the set.
    pub(super) fn membership(&self, x: f64) -> f64 {
        let [a, b, c, d] = self.points;
        if x < a || x > d {
            0.0
        } else if x < b {
            (x - a) / (b - a)
        } else if x <= c {
            1.0
        } else {
            (d - x) / (d - c)
        }
    }

    /// Where the set is at its highest, for a label.
    pub(super) fn middle(&self) -> f64 {
        (self.points[1] + self.points[2]) / 2.0
    }
}

impl Output {
    pub(super) fn parse(name: String, file: OutputFile) -> Result<Output, String> {
        let wrong = |why: String| format!("[output.{name}] {why}");
        if !is_term_name(&name) {
            return Err(wrong("the name is a lowercase word of letters, digits and `_`".to_owned()));
        }
        let (low, high) = match file.range[..] {
            [low, high] if low.is_finite() && high.is_finite() && low < high => (low, high),
            _ => return Err(wrong(format!("range is {:?}; it is two numbers, lowest first: range = [0, 100]", file.range))),
        };
        if file.sets.is_empty() {
            return Err(wrong("has no sets: add one as NAME = [a, b, c] (a triangle) or [a, b, c, d] (a trapezoid)".to_owned()));
        }
        let mut sets = Vec::with_capacity(file.sets.len());
        for (set, points) in file.sets {
            if !is_term_name(&set) {
                return Err(wrong(format!("`{set}`: a set's name is a lowercase word of letters, digits and `_`")));
            }
            let points = match points[..] {
                [a, peak, c] => [a, peak, peak, c],
                [a, b, c, d] => [a, b, c, d],
                _ => return Err(wrong(format!("{set} has {} points; a set is [a, b, c] (a triangle) or [a, b, c, d]", points.len()))),
            };
            if points.iter().any(|point| !point.is_finite() || *point < low || *point > high) {
                return Err(wrong(format!("{set} = {points:?} goes outside the range [{low}, {high}]")));
            }
            if points.windows(2).any(|pair| pair[0] > pair[1]) {
                return Err(wrong(format!("{set}'s points go down somewhere; they are in order along the axis")));
            }
            sets.push(Set { name: set, points });
        }
        sets.sort_by(|a, b| a.points.partial_cmp(&b.points).unwrap_or(std::cmp::Ordering::Equal));
        Ok(Output { name, range: (low, high), sets })
    }

    /// The x of each sample along the range.
    pub(super) fn samples(&self, count: usize) -> impl Iterator<Item = f64> + '_ {
        let (low, high) = self.range;
        (0..count).map(move |at| low + (high - low) * at as f64 / (count - 1) as f64)
    }

    /// The merged shape at `x`: each concluded set clipped at its rule's score, joined by `or`.
    pub(super) fn merged(&self, clipped: &[(usize, f64)], or: Or, x: f64) -> f64 {
        clipped.iter().fold(0.0, |merged, &(set, score)| or.apply(merged, self.sets[set].membership(x).min(score)))
    }

    /// The centre of the merged shape, or `None` when no concluded set scored above zero.
    ///
    /// The shape is integrated over an even grid across the range and a fine one across each
    /// concluded set, so a set narrow next to its range is never missed between two samples. A set
    /// with no width at all (`[5, 5, 5]`) has no area to take a centre of; when every scoring set is
    /// like that, the value is their points, weighted by score.
    pub(super) fn centroid(&self, clipped: &[(usize, f64)], or: Or) -> Option<f64> {
        let scoring: Vec<(usize, f64)> = clipped.iter().copied().filter(|(_, score)| *score > 0.0).collect();
        if scoring.is_empty() {
            return None;
        }
        let mut xs: Vec<f64> = self.samples(SAMPLES).collect();
        for &(set, _) in &scoring {
            let [a, _, _, d] = self.sets[set].points;
            xs.extend((0..=PER_SET).map(|at| a + (d - a) * at as f64 / PER_SET as f64));
            xs.extend(self.sets[set].points);
        }
        xs.sort_by(f64::total_cmp);
        xs.dedup();
        let heights: Vec<f64> = xs.iter().map(|x| self.merged(&scoring, or, *x)).collect();
        // The trapezoid rule over the uneven grid, for the area and its moment about zero.
        let (mut area, mut moment) = (0.0, 0.0);
        for at in 1..xs.len() {
            let (x0, x1, y0, y1) = (xs[at - 1], xs[at], heights[at - 1], heights[at]);
            area += (x1 - x0) * (y0 + y1) / 2.0;
            moment += (x1 - x0) * (x0 * y0 + x1 * y1) / 2.0;
        }
        if area > 0.0 {
            return Some(moment / area);
        }
        // Each set's score first, its rules joined by `or` as everywhere else, so that two rules
        // for one point count as the set's score does, not as their sum.
        let mut sets: Vec<(usize, f64)> = Vec::new();
        for &(set, score) in &scoring {
            match sets.iter_mut().find(|(seen, _)| *seen == set) {
                Some((_, joined)) => *joined = or.apply(*joined, score),
                None => sets.push((set, score)),
            }
        }
        let weight: f64 = sets.iter().map(|(_, score)| score).sum();
        Some(sets.iter().map(|(set, score)| self.sets[*set].middle() * score).sum::<f64>() / weight)
    }
}

impl Then {
    /// A rule's `then`: `OUTPUT IS SET` when `OUTPUT` is one of `outputs`, and anything else an
    /// item. An item may have `IS` in its name (`request IS urgent`), as it could before outputs
    /// existed, so only a declared output's name makes it an output.
    pub(super) fn parse(then: &str, outputs: &[Output]) -> Result<Then, String> {
        let then = then.trim();
        if then.is_empty() {
            return Err("the `then` is empty".to_owned());
        }
        let words: Vec<&str> = then.split_whitespace().collect();
        let Some(at) = (match words[..] {
            [name, "IS", _] => outputs.iter().position(|output| output.name == name),
            _ => None,
        }) else {
            return Ok(Then::Item(then.to_owned()));
        };
        let sets = &outputs[at].sets;
        match sets.iter().position(|set| set.name == words[2]) {
            Some(set) => Ok(Then::Output { output: at, set }),
            None => Err(format!(
                "[output.{}] has no set `{}`; its sets are {}",
                outputs[at].name,
                words[2],
                sets.iter().map(|set| set.name.as_str()).collect::<Vec<_>>().join(", ")
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::Rules;
    use crate::{DecisionResponse, Question};

    /// The irrigation example: how much it rained, as a Score.
    const RULES: &str = r#"
        [terms]
        scarce = "rainfall.Scarce"
        regular = "rainfall.Regular"
        large = "rainfall.Large"

        [output.irrigation]
        range = [0, 100]
        drops = [0, 0, 20, 40]
        liter = [30, 50, 70]
        gallon = [60, 80, 100, 100]

        [[rule]]
        if = "scarce"
        then = "irrigation IS gallon"

        [[rule]]
        if = "regular"
        then = "irrigation IS liter"

        [[rule]]
        if = "large"
        then = "irrigation IS drops"
    "#;

    fn questions() -> Vec<(String, Question)> {
        vec![("rainfall".to_owned(), Question::score("How much has it rained?", ["Scarce", "Regular", "Large"]))]
    }

    fn rules(text: &str) -> Result<Rules, String> {
        let questions = questions();
        Rules::parse(text, questions.iter().map(|(id, question)| (id.as_str(), question)))
    }

    fn reply(levels: [f64; 3]) -> DecisionResponse {
        serde_json::from_value(json!({
            "model": "m",
            "answers": {"rainfall": {"type": "score", "score": 1.0, "confidence": 0.5,
                "probabilities": {"0": levels[0], "1": levels[1], "2": levels[2]},
                "legend": {"0": "Scarce", "1": "Regular", "2": "Large"}}},
            "usage": {"input_tokens": 1, "output_tokens": 1},
        }))
        .unwrap()
    }

    #[test]
    fn a_symmetric_set_fired_alone_has_its_peak_as_its_centre() {
        let outcome = rules(RULES).unwrap().evaluate(&reply([0.0, 0.8, 0.0])).unwrap();
        let irrigation = &outcome.outputs[0];
        assert_eq!(irrigation.output, "irrigation");
        assert!((irrigation.value.unwrap() - 50.0).abs() < 0.01, "{:?}", irrigation.value);
        // The sets in order along the axis, each with the score its rules clip it at.
        let sets: Vec<(&str, f64)> = irrigation.sets.iter().map(|set| (set.set.as_str(), set.score)).collect();
        assert_eq!(sets, [("drops", 0.0), ("liter", 0.8), ("gallon", 0.0)]);
    }

    #[test]
    fn mostly_regular_with_some_large_pulls_the_centre_toward_drops() {
        let outcome = rules(RULES).unwrap().evaluate(&reply([0.0, 0.7, 0.3])).unwrap();
        let value = outcome.outputs[0].value.unwrap();
        assert!(value > 30.0 && value < 50.0, "{value}");
        assert_eq!(outcome.text(), format!("irrigation  {value:.2}  (drops 0.30, liter 0.70, gallon 0.00)\n"));
        // Items and outputs mix: an items-only line would carry the threshold, and this has none.
        assert!(!outcome.text().contains("threshold"));
    }

    #[test]
    fn no_rule_firing_leaves_no_value() {
        let outcome = rules(RULES).unwrap().evaluate(&reply([0.0, 0.0, 0.0])).unwrap();
        assert_eq!(outcome.outputs[0].value, None);
        assert!(outcome.text().starts_with("irrigation  -  "));
        assert_eq!(serde_json::to_value(&outcome).unwrap()["outputs"][0]["value"], serde_json::Value::Null);
    }

    #[test]
    fn a_set_narrow_next_to_its_range_is_not_missed() {
        // Between two points of an even grid over a million, this triangle would have no area.
        let narrow = r#"
            [terms]
            regular = "rainfall.Regular"
            [output.flow]
            range = [0, 1000000]
            trickle = [0.1, 0.2, 0.3]
            flood = [900000, 950000, 1000000]
            [[rule]]
            if = "regular"
            then = "flow IS trickle"
        "#;
        let value = rules(narrow).unwrap().evaluate(&reply([0.0, 0.8, 0.0])).unwrap().outputs[0].value.unwrap();
        assert!((value - 0.2).abs() < 1e-6, "{value}");
        // A set with no width has no area; its point is the value.
        let point = narrow.replace("[0.1, 0.2, 0.3]", "[5, 5, 5]");
        assert_eq!(rules(&point).unwrap().evaluate(&reply([0.0, 0.8, 0.0])).unwrap().outputs[0].value, Some(5.0));
    }

    #[test]
    fn points_are_weighted_by_their_sets_scores_joined_by_or() {
        // Two rules for the point at 10 (0.3 and 0.2) and one for the point at 20 (0.3). With max,
        // the point at 10 scores 0.3, as its set's reported score says, so the value is halfway:
        // summing the rules would weight it 0.5 and pull the value to 13.75.
        let points = |logic: &str| {
            format!(
                r#"
                {logic}
                [terms]
                scarce = "rainfall.Scarce"
                regular = "rainfall.Regular"
                large = "rainfall.Large"
                [output.dose]
                range = [0, 30]
                low = [10, 10, 10]
                high = [20, 20, 20]
                [[rule]]
                if = "scarce"
                then = "dose IS low"
                [[rule]]
                if = "regular"
                then = "dose IS low"
                [[rule]]
                if = "large"
                then = "dose IS high"
                "#
            )
        };
        let outcome = rules(&points("")).unwrap().evaluate(&reply([0.3, 0.2, 0.3])).unwrap();
        assert_eq!(outcome.outputs[0].sets[0].score, 0.3);
        assert!((outcome.outputs[0].value.unwrap() - 15.0).abs() < 1e-9, "{:?}", outcome.outputs[0].value);
        // With probsum the point at 10 scores 0.3 + 0.2 − 0.06 = 0.44, so the value leans toward it.
        let probsum = rules(&points("[logic]\nor = \"probsum\"")).unwrap().evaluate(&reply([0.3, 0.2, 0.3])).unwrap();
        let expected = (10.0 * 0.44 + 20.0 * 0.3) / 0.74;
        assert!((probsum.outputs[0].value.unwrap() - expected).abs() < 1e-9, "{:?}", probsum.outputs[0].value);
    }

    #[test]
    fn a_shoulder_holds_at_the_end_of_the_range() {
        let rules = rules(RULES).unwrap();
        let drops = &rules.outputs[0].sets[0];
        assert_eq!((drops.membership(0.0), drops.membership(20.0), drops.membership(30.0), drops.membership(40.0)), (1.0, 1.0, 0.5, 0.0));
        let liter = &rules.outputs[0].sets[1];
        assert_eq!((liter.membership(40.0), liter.membership(50.0), liter.membership(71.0)), (0.5, 1.0, 0.0));
    }

    #[test]
    fn says_what_is_wrong_with_an_output() {
        let with = |output: &str, then: &str| {
            rules(&format!("[terms]\nregular = \"rainfall.Regular\"\n{output}\n[[rule]]\nif = \"regular\"\nthen = \"{then}\"")).unwrap_err()
        };
        let output = "[output.water]\nrange = [0, 10]\nsome = [0, 5, 10]";
        assert!(with(output, "water IS lots").contains("[output.water] has no set `lots`; its sets are some"));
        // IS names an output only when the output is declared: anything else is an item's name.
        let rules = rules(&format!(
            "[terms]\nregular = \"rainfall.Regular\"\n{output}\n[[rule]]\nif = \"regular\"\nthen = \"request IS urgent\"\n[[rule]]\nif = \"regular\"\nthen = \"water IS some\""
        ))
        .unwrap();
        let outcome = rules.evaluate(&reply([0.0, 1.0, 0.0])).unwrap();
        assert_eq!(outcome.items[0].item, "request IS urgent");
        assert_eq!(outcome.outputs[0].output, "water");
        assert!(with("[output.water]\nrange = [10, 0]\nsome = [0, 5, 10]", "x").contains("lowest first"));
        assert!(with("[output.water]\nrange = [0, 10]\nsome = [0, 5, 20]", "x").contains("outside the range"));
        assert!(with("[output.water]\nrange = [0, 10]\nsome = [0, 5]", "x").contains("2 points"));
        assert!(with("[output.water]\nrange = [0, 10]\nsome = [5, 2, 10]", "x").contains("go down"));
        assert!(with("[output.water]\nrange = [0, 10]", "x").contains("has no sets"));
    }
}
