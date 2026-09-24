//! Outputs: a crisp axis with named fuzzy sets, for rules that conclude `OUTPUT IS SET` rather than an
//! item. Each set's rules are joined by the file's OR into the set's score, each set is clipped at its
//! score, the clipped shapes are joined by the file's OR, and the output's value is the centre of the
//! shape that makes (Mamdani inference, centroid defuzzification).
//!
//! ```toml
//! [output.irrigation]
//! range  = [0, 100]           # millimetres of water, say: the file gives no unit
//! drops  = [0, 0, 20, 40]     # a trapezoid: a, b, c, d
//! liter  = [30, 50, 70]       # a triangle: a, peak, c
//! gallon = [60, 80, 100, 100]
//! ```
//!
//! An output's sets are all shapes or all points (`[5, 5, 5]`). A point has no area, so an output
//! of points takes its value as the points' average, weighted by their sets' scores; a point among
//! shapes would have no area to add, and is refused.
//!
//! The value says where the support lies, not how strong it is, and it can fall between two sets
//! that both have support, where neither does.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::{is_term_name, Or, Then};

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
/// falling to `d`. A triangle is the trapezoid with `b` and `c` at its peak, and a point the one
/// with all four at one place.
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

    /// Whether it is a point, with no width.
    fn is_point(&self) -> bool {
        self.points[0] == self.points[3]
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
        let (points, shapes): (Vec<&Set>, Vec<&Set>) = sets.iter().partition(|set| set.is_point());
        if let (Some(point), Some(shape)) = (points.first(), shapes.first()) {
            return Err(wrong(format!(
                "mixes a point ({}) with a set that has width ({}): a point has no area to weigh against a shape's, so an \
                 output's sets are all points or all shapes",
                point.name, shape.name
            )));
        }
        sets.sort_by(|a, b| a.points.partial_cmp(&b.points).unwrap_or(std::cmp::Ordering::Equal));
        Ok(Output { name, range: (low, high), sets })
    }

    /// Where `x` is along the range, from 0 at its low end to 1 at its high end. Halved first, so
    /// that a range as wide as `f64` allows doesn't overflow.
    pub(super) fn position(&self, x: f64) -> f64 {
        along(x, self.range.0, self.range.1)
    }

    /// The point at `position` along the range: the inverse of [`Output::position`].
    pub(super) fn at(&self, position: f64) -> f64 {
        let (low, high) = self.range;
        match high - low {
            span if span.is_finite() => low + position * span,
            _ => (low / 2.0 + position * (high / 2.0 - low / 2.0)) * 2.0,
        }
    }

    /// The x of each sample along the range, for drawing.
    pub(super) fn samples(&self, count: usize) -> impl Iterator<Item = f64> + '_ {
        (0..count).map(move |at| self.at(at as f64 / (count - 1) as f64))
    }

    /// Where the merged shape changes course: every scoring set's corners and the points where its
    /// clip meets its sides. A drawing that includes them misses no narrow set and no point.
    pub(super) fn corners(&self, scores: &[f64]) -> Vec<f64> {
        let mut corners: Vec<f64> = Vec::new();
        for (set, &score) in self.sets.iter().zip(scores).filter(|(_, score)| **score > 0.0) {
            let [a, b, c, d] = set.points;
            corners.extend([a, b, c, d, a + (b - a) * score.min(1.0), d - (d - c) * score.min(1.0)]);
        }
        corners.sort_by(f64::total_cmp);
        corners.dedup();
        corners
    }

    /// The merged shape at `x`: each set clipped at its score (`scores`, one per set), joined by `or`.
    pub(super) fn merged(&self, scores: &[f64], or: Or, x: f64) -> f64 {
        let clipped = self.sets.iter().zip(scores).filter(|(_, score)| **score > 0.0).map(|(set, &score)| set.membership(x).min(score));
        or.join(clipped)
    }

    /// Whether every set is a point, so that the output is a weighted average of points.
    pub(super) fn is_points(&self) -> bool {
        self.sets.iter().all(Set::is_point)
    }

    /// The centre of the merged shape: `Ok(None)` when no set scored above zero, and an error when
    /// sets did but the arithmetic couldn't give a finite centre inside the range, which is never
    /// passed off as no support. `scores` has one score per set.
    ///
    /// The integral is taken piece by piece, in closed form rather than by sampling, on the range
    /// scaled to 0–1 (so that a wide range can't overflow). Between two neighbouring corners every
    /// clipped set is a straight line. Under max and bounded the join of those lines is itself
    /// straight once each piece is cut where max's lines cross or bounded's sum reaches 1, and a
    /// straight piece's area and moment are exact. Under probsum the join `1 − Π(1 − aᵢ)` is a
    /// polynomial of degree n in the n sets active on the piece: it is evaluated as
    /// `−expm1(Σ ln(1 − aᵢ))`, which keeps a tiny support and cancels nothing, and integrated by
    /// Gauss–Legendre with enough points to be exact for that degree. What remains is ordinary
    /// floating-point rounding.
    pub(super) fn centroid(&self, scores: &[f64], or: Or) -> Result<Option<f64>, String> {
        let scoring: Vec<(&Set, f64)> =
            self.sets.iter().zip(scores).filter(|(_, score)| **score > 0.0).map(|(set, score)| (set, score.min(1.0))).collect();
        if scoring.is_empty() {
            return Ok(None);
        }
        let centre = if scoring.iter().all(|(set, _)| set.is_point()) {
            // A weighted average of positions from 0 to 1, which can't overflow.
            let weight: f64 = scoring.iter().map(|(_, score)| score).sum();
            scoring.iter().map(|(set, score)| self.position(set.points[0]) * (score / weight)).sum::<f64>()
        } else {
            self.shape_centre(&scoring, or)?
        };
        if !centre.is_finite() || !(-1e-9..=1.0 + 1e-9).contains(&centre) {
            return Err(format!("the centre came to {centre} of the range, which isn't inside it; the numbers are too extreme to add up"));
        }
        Ok(Some(self.at(centre.clamp(0.0, 1.0))))
    }

    /// The centre of the clipped shapes of `scoring`, as a position from 0 to 1 along the range.
    fn shape_centre(&self, scoring: &[(&Set, f64)], or: Or) -> Result<f64, String> {
        // Each set in positions from 0 to 1.
        let sets: Vec<(Set, f64)> = scoring
            .iter()
            .map(|(set, score)| (Set { name: String::new(), points: set.points.map(|point| self.position(point)) }, *score))
            .collect();
        let mut breaks: Vec<f64> = Vec::new();
        for (set, score) in &sets {
            let [a, b, c, d] = set.points;
            breaks.extend([a, b, c, d, a + (b - a) * score, d - (d - c) * score]);
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        let (mut area, mut moment) = (0.0, 0.0);
        for pair in breaks.windows(2) {
            let (u0, width) = (pair[0], pair[1] - pair[0]);
            if width <= 0.0 {
                continue;
            }
            // Each set's line across the piece, as its values at the two ends, read from inside (at
            // a third and two thirds), since a vertical side at an end has two values. A set that
            // is zero all along the piece adds nothing, whatever the OR, and is left out.
            let lines: Vec<(f64, f64)> = sets
                .iter()
                .map(|(set, score)| {
                    let at = |t: f64| set.membership(u0 + width * t).min(*score);
                    let (first, second) = (at(1.0 / 3.0), at(2.0 / 3.0));
                    ((2.0 * first - second).clamp(0.0, 1.0), (2.0 * second - first).clamp(0.0, 1.0))
                })
                .filter(|(from, to)| *from > 0.0 || *to > 0.0)
                .collect();
            if lines.is_empty() {
                continue;
            }
            for (t0, t1) in or.pieces(&lines) {
                let (start, span) = (u0 + width * t0, width * (t1 - t0));
                let ends: Vec<(f64, f64)> = lines.iter().map(|(from, to)| (from + (to - from) * t0, from + (to - from) * t1)).collect();
                let (piece_area, piece_moment) = match or {
                    Or::Max | Or::Bounded => straight(or.straight_ends(&ends), start, span),
                    Or::Probsum => gauss(&ends, start, span),
                };
                area += piece_area;
                moment += piece_moment;
            }
        }
        match area > 0.0 && area.is_finite() && moment.is_finite() {
            true => Ok(moment / area),
            false => {
                Err(format!("its sets have support, but their shape's area came to {area}: too small or too large to take a centre of"))
            }
        }
    }
}

/// Where `x` is between `low` and `high`, from 0 to 1. Directly when `high − low` is a number, and
/// from the halves only when it would overflow: halving always would turn a range as small as
/// `[0, 5e-324]` into 0/0.
pub(super) fn along(x: f64, low: f64, high: f64) -> f64 {
    match high - low {
        span if span.is_finite() => (x - low) / span,
        _ => (x / 2.0 - low / 2.0) / (high / 2.0 - low / 2.0),
    }
}

impl Or {
    /// Joins degrees, as the file's OR does. Probsum is taken as `−expm1(Σ ln(1 − a))`: the same
    /// `1 − Π(1 − a)`, without subtracting from 1 a number close to 1, which would lose a small
    /// degree altogether.
    pub(super) fn join(self, degrees: impl Iterator<Item = f64>) -> f64 {
        match self {
            Or::Probsum => -degrees.map(|degree| (-degree.min(1.0)).ln_1p()).sum::<f64>().exp_m1(),
            _ => degrees.fold(0.0, |joined, degree| self.apply(joined, degree)),
        }
    }

    /// Where, between 0 and 1 along a piece, `lines` (each its values at the two ends) must be cut
    /// for their join to be straight on each part: where two cross, for max, and where their sum
    /// reaches 1, for bounded. Probsum is integrated as the polynomial it is, with no cuts.
    fn pieces(self, lines: &[(f64, f64)]) -> Vec<(f64, f64)> {
        let crossing = |from: f64, to: f64| (from * to < 0.0).then(|| from / (from - to));
        let mut cuts: Vec<f64> = match self {
            Or::Max => (0..lines.len())
                .flat_map(|i| (i + 1..lines.len()).map(move |j| (i, j)))
                .filter_map(|(i, j)| crossing(lines[i].0 - lines[j].0, lines[i].1 - lines[j].1))
                .collect(),
            Or::Bounded => {
                let (from, to) = lines.iter().fold((0.0, 0.0), |(from, to), line| (from + line.0, to + line.1));
                crossing(from - 1.0, to - 1.0).into_iter().collect()
            }
            Or::Probsum => Vec::new(),
        };
        cuts.sort_by(f64::total_cmp);
        let bounds: Vec<f64> = std::iter::once(0.0).chain(cuts).chain(std::iter::once(1.0)).collect();
        bounds.windows(2).map(|pair| (pair[0], pair[1])).filter(|(t0, t1)| t1 > t0).collect()
    }

    /// The join's values at the two ends of a piece with no cut inside, for max and bounded, whose
    /// join is straight there.
    fn straight_ends(self, lines: &[(f64, f64)]) -> (f64, f64) {
        match self {
            // No two lines cross inside, so the highest at the middle is the highest throughout.
            Or::Max => lines.iter().copied().max_by(|a, b| (a.0 + a.1).total_cmp(&(b.0 + b.1))).unwrap_or((0.0, 0.0)),
            // The sum doesn't reach 1 inside, so it is either under 1 throughout or at it.
            _ => {
                let (from, to) = lines.iter().fold((0.0, 0.0), |(from, to), line| (from + line.0, to + line.1));
                (from.min(1.0), to.min(1.0))
            }
        }
    }
}

/// The area under a straight piece from `from` to `to` over `start` to `start + width`, and its
/// moment about zero: exact.
fn straight((from, to): (f64, f64), start: f64, width: f64) -> (f64, f64) {
    let area = width * (from + to) / 2.0;
    (area, start * area + width * width * (from / 6.0 + to / 3.0))
}

/// The area and moment of probsum's join of `lines` over `start` to `start + width`, by
/// Gauss–Legendre: with n lines the join is a polynomial of degree n and the moment's integrand of
/// degree n + 1, which m points integrate exactly when 2m − 1 ≥ n + 1, so m = ⌈(n + 2) / 2⌉.
fn gauss(lines: &[(f64, f64)], start: f64, width: f64) -> (f64, f64) {
    let (mut area, mut moment) = (0.0, 0.0);
    for (node, weight) in legendre(lines.len().div_ceil(2) + 1) {
        let t = (node + 1.0) / 2.0;
        let height = Or::Probsum.join(lines.iter().map(|(from, to)| from + (to - from) * t));
        let w = weight / 2.0 * width;
        area += w * height;
        moment += w * height * (start + width * t);
    }
    (area, moment)
}

/// The nodes and weights of `count`-point Gauss–Legendre quadrature on −1 to 1, by Newton's method
/// on the Legendre polynomial from the usual first guesses.
fn legendre(count: usize) -> Vec<(f64, f64)> {
    let n = count as f64;
    (0..count)
        .map(|i| {
            let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (n + 0.5)).cos();
            let mut derivative = 1.0;
            for _ in 0..100 {
                // P_n(x) and P_{n−1}(x) by the three-term recurrence, then P_n'(x).
                let (mut p, mut previous) = (1.0, 0.0);
                for k in 1..=count {
                    let k = k as f64;
                    (p, previous) = (((2.0 * k - 1.0) * x * p - (k - 1.0) * previous) / k, p);
                }
                derivative = n * (x * p - previous) / (x * x - 1.0);
                let step = p / derivative;
                x -= step;
                if step.abs() < 1e-15 {
                    break;
                }
            }
            (x, 2.0 / ((1.0 - x * x) * derivative * derivative))
        })
        .collect()
}

impl Then {
    /// A rule's `then`: `OUTPUT IS SET`, or an item. Three words with `IS` between them always
    /// conclude in an output, so a typo in the output's name is an error rather than an item that
    /// quietly takes the rule; an item is named any other way (`request is urgent`).
    pub(super) fn parse(then: &str, outputs: &[Output]) -> Result<Then, String> {
        let then = then.trim();
        if then.is_empty() {
            return Err("the `then` is empty".to_owned());
        }
        let words: Vec<&str> = then.split_whitespace().collect();
        let [name, "IS", set] = words[..] else {
            return Ok(Then::Item(then.to_owned()));
        };
        let Some(at) = outputs.iter().position(|output| output.name == name) else {
            let names: Vec<&str> = outputs.iter().map(|output| output.name.as_str()).collect();
            let hint = match closest(name, &names) {
                Some(close) => format!("; did you mean `{close} IS {set}`?"),
                None if names.is_empty() => format!(
                    "; declare it, as [output.{name}] with a range and its sets, or name the item without `IS`, as `{name} is {set}`"
                ),
                None => format!("; the outputs are {}, or name an item without `IS`, as `{name} is {set}`", names.join(", ")),
            };
            return Err(format!("`{then}` concludes in an output, and there is no [output.{name}]{hint}"));
        };
        let sets = &outputs[at].sets;
        match sets.iter().position(|candidate| candidate.name == set) {
            Some(set) => Ok(Then::Output { output: at, set }),
            None => {
                let names: Vec<&str> = sets.iter().map(|set| set.name.as_str()).collect();
                let hint = closest(set, &names).map(|close| format!("; did you mean `{close}`?")).unwrap_or_default();
                Err(format!("[output.{}] has no set `{set}`; its sets are {}{hint}", outputs[at].name, names.join(", ")))
            }
        }
    }
}

/// The name in `names` a typo of `name` most likely meant: within two edits, and the nearest.
fn closest<'a>(name: &str, names: &[&'a str]) -> Option<&'a str> {
    names
        .iter()
        .map(|candidate| (edits(name, candidate), *candidate))
        .filter(|(distance, _)| *distance <= 2)
        .min()
        .map(|(_, candidate)| candidate)
}

/// How many one-character insertions, deletions and substitutions turn `a` into `b`.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let next = (row[j + 1] + 1).min(row[j] + 1).min(diagonal + usize::from(x != *y));
            diagonal = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{Or, Rules};
    use super::*;
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
            "answers": {"rainfall": {"type": "score", "score": levels[1] + 2.0 * levels[2], "confidence": 0.5,
                "probabilities": {"0": levels[0], "1": levels[1], "2": levels[2]},
                "legend": {"0": "Scarce", "1": "Regular", "2": "Large"}}},
            "usage": {"input_tokens": 1, "output_tokens": 1},
        }))
        .unwrap()
    }

    /// An output `y` over `range` with `sets`, one rule per set concluding it from `rainfall`'s
    /// levels in turn, under `logic`.
    fn output(logic: &str, range: &str, sets: &[(&str, &str)], thens: &[(&str, &str)]) -> Rules {
        let mut text = format!(
            "{logic}\n[terms]\nscarce = \"rainfall.Scarce\"\nregular = \"rainfall.Regular\"\nlarge = \"rainfall.Large\"\n[output.y]\nrange = {range}\n"
        );
        for (name, points) in sets {
            text.push_str(&format!("{name} = {points}\n"));
        }
        for (when, set) in thens {
            text.push_str(&format!("[[rule]]\nif = \"{when}\"\nthen = \"y IS {set}\"\n"));
        }
        rules(&text).unwrap()
    }

    fn value(rules: &Rules, levels: [f64; 3]) -> f64 {
        rules.evaluate(&reply(levels)).unwrap().outputs[0].value.unwrap()
    }

    #[test]
    fn a_symmetric_set_fired_alone_has_its_peak_as_its_centre() {
        let outcome = rules(RULES).unwrap().evaluate(&reply([0.0, 1.0, 0.0])).unwrap();
        let irrigation = &outcome.outputs[0];
        assert_eq!(irrigation.output, "irrigation");
        assert!((irrigation.value.unwrap() - 50.0).abs() < 1e-9, "{:?}", irrigation.value);
        // The sets in order along the axis, each with the score its rules clip it at.
        let sets: Vec<(&str, f64)> = irrigation.sets.iter().map(|set| (set.set.as_str(), set.score)).collect();
        assert_eq!(sets, [("drops", 0.0), ("liter", 1.0), ("gallon", 0.0)]);
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
        let only_scarce = output("", "[0, 100]", &[("low", "[0, 10, 20]")], &[("scarce", "low")]);
        let outcome = only_scarce.evaluate(&reply([0.0, 1.0, 0.0])).unwrap();
        assert_eq!(outcome.outputs[0].value, None);
        assert!(outcome.text().starts_with("y  -  "));
        assert_eq!(serde_json::to_value(&outcome).unwrap()["outputs"][0]["value"], serde_json::Value::Null);
    }

    #[test]
    fn a_set_narrow_next_to_its_range_is_not_missed() {
        let narrow = output(
            "",
            "[0, 1000000]",
            &[("trickle", "[0.1, 0.2, 0.3]"), ("flood", "[900000, 950000, 1000000]")],
            &[("regular", "trickle")],
        );
        let value = value(&narrow, [0.2, 0.8, 0.0]);
        assert!((value - 0.2).abs() < 1e-9, "{value}");
    }

    #[test]
    fn two_equal_triangles_meet_in_the_middle_under_every_or() {
        // Symmetric about 65, so the centre is 65 whatever joins them.
        for logic in ["", "[logic]\nor = \"probsum\"", "[logic]\nor = \"bounded\""] {
            let two = output(logic, "[0, 100]", &[("a", "[30, 50, 70]"), ("b", "[60, 80, 100]")], &[("regular", "a"), ("large", "b")]);
            let value = value(&two, [0.0, 0.5, 0.5]);
            assert!((value - 65.0).abs() < 1e-9, "{logic}: {value}");
        }
    }

    #[test]
    fn the_centre_is_exact_not_sampled() {
        // Two triangles 10 wide that don't overlap, centred on 5 and 20. Clipped at h, each is a
        // trapezoid of area h × (10 + 10(1 − h)) / 2 about its own centre.
        let rules = output("", "[0, 30]", &[("a", "[0, 5, 10]"), ("b", "[15, 20, 25]")], &[("regular", "a"), ("large", "b")]);
        let area = |h: f64| h * (10.0 + 10.0 * (1.0 - h)) / 2.0;
        for (a, b) in [(0.5, 0.5), (0.75, 0.25), (0.9, 0.1)] {
            let want = (area(a) * 5.0 + area(b) * 20.0) / (area(a) + area(b));
            let got = value(&rules, [0.0, a, b]);
            assert!((got - want).abs() < 1e-12, "{a}, {b}: {got} vs {want}");
        }
    }

    #[test]
    fn probsum_and_bounded_match_a_fine_sum() {
        // Overlapping sets, where the join is curved (probsum) or meets 1 (bounded): the closed form
        // agrees with a sum over a million steps.
        let sets = [("a", "[0, 20, 40, 60]"), ("b", "[30, 50, 70]"), ("c", "[45, 80, 100, 100]")];
        let thens = [("scarce", "a"), ("regular", "b"), ("large", "c")];
        for (logic, or) in [("[logic]\nor = \"probsum\"", Or::Probsum), ("[logic]\nor = \"bounded\"", Or::Bounded), ("", Or::Max)] {
            let rules = output(logic, "[0, 100]", &sets, &thens);
            let levels = [0.3, 0.9, 0.6];
            let levels = [levels[0] / 1.8, levels[1] / 1.8, levels[2] / 1.8];
            let got = value(&rules, levels);
            let output = &rules.outputs[0];
            let (mut area, mut moment) = (0.0, 0.0);
            let steps = 1_000_000;
            for at in 0..steps {
                let x = (at as f64 + 0.5) * 100.0 / steps as f64;
                let y = output.merged(&levels, or, x);
                area += y;
                moment += x * y;
            }
            assert!((got - moment / area).abs() < 1e-6, "{logic}: {got} vs {}", moment / area);
        }
    }

    #[test]
    fn points_are_weighted_by_their_sets_scores() {
        // The review's case: two points far apart on a wide range, equally scored.
        let points = output(
            "",
            "[0, 1000]",
            &[("low", "[0.0001, 0.0001, 0.0001]"), ("high", "[5, 5, 5]")],
            &[("regular", "low"), ("large", "high")],
        );
        assert!((value(&points, [0.0, 0.5, 0.5]) - 2.50005).abs() < 1e-12);
        let points = output("", "[0, 100]", &[("low", "[0, 0, 0]"), ("high", "[10, 10, 10]")], &[("regular", "low"), ("large", "high")]);
        assert!((value(&points, [0.0, 0.5, 0.5]) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn a_sets_rules_join_before_it_is_clipped() {
        // Two rules for the point at 10 (0.3 and 0.2) and one for the point at 20 (0.5). With max
        // the point at 10 scores 0.3, as its set's reported score says: (3 + 10) / 0.8.
        let sets = [("low", "[10, 10, 10]"), ("high", "[20, 20, 20]")];
        let thens = [("scarce", "low"), ("regular", "low"), ("large", "high")];
        let max = output("", "[0, 30]", &sets, &thens);
        let outcome = max.evaluate(&reply([0.3, 0.2, 0.5])).unwrap();
        assert_eq!(outcome.outputs[0].sets[0].score, 0.3);
        assert!((outcome.outputs[0].value.unwrap() - 16.25).abs() < 1e-12);
        // With probsum the point at 10 scores 0.3 + 0.2 − 0.06 = 0.44.
        let probsum = output("[logic]\nor = \"probsum\"", "[0, 30]", &sets, &thens);
        let expected = (10.0 * 0.44 + 20.0 * 0.5) / 0.94;
        assert!((value(&probsum, [0.3, 0.2, 0.5]) - expected).abs() < 1e-12);

        // Shapes too, the review's case: both triangles at 1, and the first's rule written twice.
        // Its set is still at 1 under any OR, so the value stays 65; clipping each rule's copy
        // apart and joining them by probsum would have pulled it to about 62.8.
        let shapes = [("a", "[30, 50, 70]"), ("b", "[60, 80, 100]")];
        for logic in ["", "[logic]\nor = \"probsum\"", "[logic]\nor = \"bounded\""] {
            let twice = output(logic, "[0, 100]", &shapes, &[("NOT scarce", "a"), ("NOT scarce", "a"), ("NOT scarce", "b")]);
            let got = value(&twice, [0.0, 0.5, 0.5]);
            assert!((got - 65.0).abs() < 1e-9, "{logic}: {got}");
        }
    }

    #[test]
    fn many_overlapping_sets_keep_their_centre() {
        // Identical triangles, all at 1: the shape is symmetric about 0.5 however high probsum
        // stacks it. Expanding 1 − Π(1 − a) into powers of x lost this by 60 sets and gave no value
        // by 100.
        for count in [1, 3, 20, 60, 100, 200] {
            let names: Vec<String> = (0..count).map(|at| format!("s{at}")).collect();
            let sets: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "[0, 0.5, 1]")).collect();
            let thens: Vec<(&str, &str)> = names.iter().map(|name| ("NOT scarce", name.as_str())).collect();
            for logic in ["", "[logic]\nor = \"probsum\"", "[logic]\nor = \"bounded\""] {
                let rules = output(logic, "[0, 1]", &sets, &thens);
                let got = value(&rules, [0.0, 0.5, 0.5]);
                assert!((got - 0.5).abs() < 1e-9, "{count} sets, {logic}: {got}");
            }
        }
    }

    #[test]
    fn a_tiny_support_is_still_support() {
        // A weight of 1e-20: 1 − (1 − 1e-20) is 0 in f64, which read as no support at all.
        let text = "[logic]\nor = \"probsum\"\n[terms]\nscarce = \"rainfall.Scarce\"\n[output.y]\nrange = [0, 1]\nflat = [0, 0, 1, 1]\n\
                    [[rule]]\nif = \"NOT scarce\"\nthen = \"y IS flat\"\nweight = 1e-20\n";
        let got = value(&rules(text).unwrap(), [0.0, 0.5, 0.5]);
        assert!((got - 0.5).abs() < 1e-9, "{got}");
    }

    #[test]
    fn a_wide_range_does_not_overflow() {
        // Squaring a width of 1e200 made the moment infinite.
        let wide = output("", "[0, 1e200]", &[("all", "[0, 0, 1e200, 1e200]")], &[("NOT scarce", "all")]);
        let got = value(&wide, [0.0, 0.5, 0.5]);
        assert!((got / 5e199 - 1.0).abs() < 1e-9, "{got}");
        // And across the whole of f64, points and shapes alike.
        let points = output(
            "",
            "[-1.7e308, 1.7e308]",
            &[("low", "[-1.6e308, -1.6e308, -1.6e308]"), ("high", "[1.6e308, 1.6e308, 1.6e308]")],
            &[("NOT scarce", "low"), ("NOT scarce", "high")],
        );
        assert!(value(&points, [0.0, 0.5, 0.5]).abs() < 1e300);
        let shape = output("", "[-1.7e308, 1.7e308]", &[("all", "[-1.7e308, -1.7e308, 1.7e308, 1.7e308]")], &[("NOT scarce", "all")]);
        assert!(value(&shape, [0.0, 0.5, 0.5]).is_finite());
    }

    #[test]
    fn a_tiny_range_keeps_its_scale() {
        // Halving 5e-324 gives 0, and 0/0 is no position at all.
        let point = output("", "[0, 5e-324]", &[("top", "[5e-324, 5e-324, 5e-324]")], &[("NOT scarce", "top")]);
        assert_eq!(value(&point, [0.0, 0.5, 0.5]), 5e-324);
        let flat = output("", "[0, 1e-310]", &[("all", "[0, 0, 1e-310, 1e-310]")], &[("NOT scarce", "all")]);
        assert!((value(&flat, [0.0, 0.5, 0.5]) / 5e-311 - 1.0).abs() < 1e-6);
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
        assert!(with(output, "water IS somr").contains("did you mean `some`?"));
        // `X IS Y` always concludes in an output: a typo in its name is an error, not an item.
        let typo = with(output, "wter IS some");
        assert!(typo.contains("there is no [output.wter]; did you mean `water IS some`?"), "{typo}");
        let far = with(output, "request IS urgent");
        assert!(far.contains("the outputs are water") && far.contains("`request is urgent`"), "{far}");
        let none = with("", "request IS urgent");
        assert!(none.contains("declare it, as [output.request]"), "{none}");
        // Any other shape is an item's name.
        let rules =
            rules(&format!("[terms]\nregular = \"rainfall.Regular\"\n{output}\n[[rule]]\nif = \"regular\"\nthen = \"request is urgent\""))
                .unwrap();
        assert_eq!(rules.evaluate(&reply([0.0, 1.0, 0.0])).unwrap().items[0].item, "request is urgent");

        assert!(with("[output.water]\nrange = [10, 0]\nsome = [0, 5, 10]", "x").contains("lowest first"));
        assert!(with("[output.water]\nrange = [0, 10]\nsome = [0, 5, 20]", "x").contains("outside the range"));
        assert!(with("[output.water]\nrange = [0, 10]\nsome = [0, 5]", "x").contains("2 points"));
        assert!(with("[output.water]\nrange = [0, 10]\nsome = [5, 2, 10]", "x").contains("go down"));
        assert!(with("[output.water]\nrange = [0, 10]", "x").contains("has no sets"));
        let mixed = with("[output.water]\nrange = [0, 10]\nsome = [0, 5, 10]\nexact = [5, 5, 5]", "x");
        assert!(mixed.contains("mixes a point (exact) with a set that has width (some)"), "{mixed}");
    }

    #[test]
    fn counts_edits() {
        assert_eq!(edits("water", "water"), 0);
        assert_eq!(edits("wter", "water"), 1);
        assert_eq!(edits("watre", "water"), 2);
        assert_eq!(closest("irigation", &["irrigation", "dose"]), Some("irrigation"));
        assert_eq!(closest("request", &["irrigation"]), None);
    }
}
