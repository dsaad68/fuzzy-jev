//! The rules as an SVG image, laid out as a fuzzy rule base is drawn: premises, then each rule's
//! operators, then its conclusion, a row per rule, and a final column. See [`super::graph`].

use std::fmt::Write as _;

use super::graph::{trim_number, Node, Row};
use super::output::Set;
use super::{Kind, Rules, Target, Then};
use crate::DecisionResponse;

/// A panel's plot, and the gaps around it.
const PANEL_WIDTH: f64 = 180.0;
const PANEL_HEIGHT: f64 = 76.0;
const GAP: f64 = 22.0;
/// The column of operator trees, and the final column, which holds labels as well as a plot.
const TREE_WIDTH: f64 = 200.0;
const FINAL_WIDTH: f64 = 260.0;
/// Room for "R1" to the left of the rows, and for the column titles above them.
const GUTTER: f64 = 44.0;
const TOP: f64 = 64.0;
/// A row: its caption, its panel, and under the panel's axis the labels and a marker's value.
const CAPTION: f64 = 20.0;
const UNDER: f64 = 34.0;
const ROW: f64 = CAPTION + PANEL_HEIGHT + UNDER + 12.0;

const INK: &str = "#1f2328";
const FAINT: &str = "#9aa0a6";
const FRAME: &str = "#d0d4d9";
const FILL: &str = "#b8c4d0";
const YES: &str = "#2f7d4a";
const MARK: &str = "#c2410c";

impl Rules {
    /// The rules as an SVG image: a row per rule with its premises (each question's levels or
    /// options as bars of their probabilities, the term's own in bold), the tree of operators that
    /// combine them, and its conclusion (an item's bar, or an output's set clipped at the rule's
    /// score); then a final column with every item against the threshold and each output's merged
    /// shape and its centre. Without `reply`, the structure alone.
    pub fn graph_svg(&self, reply: Option<&DecisionResponse>) -> crate::Result<String> {
        let degrees = reply.map(|reply| self.degrees(reply)).transpose()?;
        let rows = self.rows(degrees.as_ref());

        // The premise columns: one per question, in the order the rules first use them.
        let mut questions: Vec<String> = Vec::new();
        for row in &rows {
            for term in terms(&row.tree) {
                let id = &self.terms[&term].id;
                if !questions.contains(id) {
                    questions.push(id.clone());
                }
            }
        }
        // The conclusion columns: each output a rule concludes in, then the items.
        let mut conclusions: Vec<Option<usize>> = Vec::new();
        for row in &rows {
            let column = match row.then {
                Then::Output { output, .. } => Some(output),
                Then::Item(_) => None,
            };
            if !conclusions.contains(&column) {
                conclusions.push(column);
            }
        }
        conclusions.sort_by_key(|column| column.unwrap_or(usize::MAX));

        let premise_x = |at: usize| GUTTER + at as f64 * (PANEL_WIDTH + GAP);
        let tree_x = premise_x(questions.len());
        let conclusion_x = |at: usize| tree_x + TREE_WIDTH + GAP + at as f64 * (PANEL_WIDTH + GAP);
        let final_x = conclusion_x(conclusions.len()) + GAP;
        let width = final_x + FINAL_WIDTH + GAP;

        let mut svg = Svg::default();
        let span = |from: f64, to: f64| (from + to) / 2.0;
        if !questions.is_empty() {
            svg.text(span(premise_x(0), premise_x(questions.len()) - GAP), 22.0, 14.0, "middle", "bold", INK, "Premises");
        }
        svg.text(tree_x + TREE_WIDTH / 2.0, 22.0, 14.0, "middle", "bold", INK, "Rules");
        svg.text(span(conclusion_x(0), conclusion_x(conclusions.len()) - GAP), 22.0, 14.0, "middle", "bold", INK, "Conclusions");
        svg.text(final_x + FINAL_WIDTH / 2.0, 22.0, 14.0, "middle", "bold", INK, "Final");
        for (at, id) in questions.iter().enumerate() {
            svg.text(premise_x(at) + PANEL_WIDTH / 2.0, 42.0, 12.0, "middle", "normal", FAINT, id);
        }
        for (at, column) in conclusions.iter().enumerate() {
            let title = column.map_or("items", |output| self.outputs[output].name.as_str());
            svg.text(conclusion_x(at) + PANEL_WIDTH / 2.0, 42.0, 12.0, "middle", "normal", FAINT, title);
        }

        for (at, row) in rows.iter().enumerate() {
            // A group per rule, named by its number and what it concludes, so a page that shows the
            // drawing can point at a rule. It draws nothing itself.
            let then = match &row.then {
                Then::Item(name) => name.as_str(),
                Then::Output { output, .. } => self.outputs[*output].name.as_str(),
            };
            let _ = writeln!(svg.body, "<g class=\"rule\" data-rule=\"{}\" data-then=\"{}\">", row.number, escape(then));
            let top = TOP + at as f64 * ROW;
            let plot_top = top + CAPTION;
            svg.text(8.0, plot_top + PANEL_HEIGHT / 2.0 + 5.0, 15.0, "start", "bold", INK, &format!("R{}", row.number));
            let caption = format!("if {}  ⇒  {}", row.text, self.then_text(&row.then));
            svg.text(GUTTER, top + 13.0, 12.0, "start", "normal", INK, &caption);

            let used = terms(&row.tree);
            for (column, id) in questions.iter().enumerate() {
                let targets: Vec<&Target> = used.iter().map(|term| &self.terms[term]).filter(|target| &target.id == id).collect();
                if targets.is_empty() {
                    continue;
                }
                let frame = Frame::new(premise_x(column), plot_top);
                self.premise(&mut svg, &frame, &targets, reply);
            }

            if !questions.is_empty() {
                svg.arrow(tree_x - GAP + 4.0, tree_x - 4.0, plot_top + PANEL_HEIGHT / 2.0);
            }
            self.tree(&mut svg, row, tree_x, plot_top);
            let conclusion = match row.then {
                Then::Output { output, .. } => conclusions.iter().position(|column| *column == Some(output)),
                Then::Item(_) => conclusions.iter().position(Option::is_none),
            };
            if let Some(column) = conclusion {
                let frame = Frame::new(conclusion_x(column), plot_top);
                svg.arrow(tree_x + TREE_WIDTH + 4.0, frame.x - 4.0, plot_top + PANEL_HEIGHT / 2.0);
                self.conclusion(&mut svg, &frame, row);
            }
            svg.body.push_str("</g>\n");
        }

        let bottom = self.finals(&mut svg, &rows, final_x, reply);
        let height = (TOP + rows.len() as f64 * ROW).max(bottom) + 28.0;
        let note = match reply {
            Some(reply) => format!("{} · AND = {}, OR = {}", reply.model, self.logic.and.describe(), self.logic.or.describe()),
            None => "Structure only: give a state to see Jev's degrees.".to_owned(),
        };
        svg.text(GUTTER, height - 10.0, 11.0, "start", "normal", FAINT, &note);
        Ok(svg.finish(width, height))
    }

    /// A question's panel: a bar per level, option, or no and yes, in order, filled to the
    /// probability Jev gave it. `targets` are the terms of this rule that read it, drawn in bold with
    /// their number. A Score's expected level is a marker of its own: it isn't a probability, and two
    /// different spreads can have the same one. A probability the reply leaves out is marked as
    /// missing rather than drawn as zero.
    fn premise(&self, svg: &mut Svg, frame: &Frame, targets: &[&Target], reply: Option<&DecisionResponse>) {
        svg.frame(frame);
        let target = targets[0];
        let selected: Vec<usize> = targets.iter().map(|target| target.selected).collect();
        let probabilities = reply.and_then(|reply| target.probabilities(reply));
        let count = target.labels.len();
        let slot = frame.width / count as f64;
        let levels = frame.domain(-0.5, count as f64 - 0.5);
        for (at, label) in target.labels.iter().enumerate() {
            let bold = selected.contains(&at);
            let x = frame.x + at as f64 * slot + 4.0;
            let bar = slot - 8.0;
            match probabilities.as_ref().map(|probabilities| probabilities[at]) {
                Some(Some(probability)) => {
                    let height = probability * (frame.height - 10.0);
                    let fill = if bold { FILL } else { FRAME };
                    svg.rect(x, frame.bottom() - height, bar, height, fill, "none", 0.0);
                    if bold {
                        // Inside the bar's top when it is tall enough, over it when it isn't.
                        let y = if height > 16.0 { frame.bottom() - height + 12.0 } else { frame.bottom() - height - 3.0 };
                        svg.text(x + bar / 2.0, y, 10.0, "middle", "bold", INK, &format!("{probability:.2}"));
                    }
                }
                Some(None) => svg.text(x + bar / 2.0, frame.bottom() - 4.0, 9.0, "middle", "normal", MARK, "missing"),
                None => {}
            }
            let (stroke, width) = if bold { (INK, 2.0) } else { (FAINT, 1.0) };
            svg.rect(x, frame.y + 10.0, bar, frame.height - 10.0, "none", stroke, width);
            svg.label(&levels, at as f64, label, count, bold);
        }
        if target.kind == Kind::Score {
            if let Some(score) = reply.and_then(|reply| reply.score(&target.id).ok()) {
                svg.tick(&levels, score.score, &format!("expected {:.2}", score.score));
            }
        }
    }

    /// A rule's `if` as a tree of its operators, each with its value when there is one.
    fn tree(&self, svg: &mut Svg, row: &Row, x: f64, y: f64) {
        svg.rect(x, y, TREE_WIDTH, PANEL_HEIGHT, "#f6f8fa", FRAME, 1.0);
        let mut lines = Vec::new();
        tree_lines(&row.tree, "", "", &mut lines);
        if row.weight < 1.0 {
            lines.push(format!("× weight {}", row.weight));
        }
        let most = ((PANEL_HEIGHT - 8.0) / 13.0) as usize;
        if lines.len() > most {
            lines.truncate(most - 1);
            lines.push("…".to_owned());
        }
        for (at, line) in lines.iter().enumerate() {
            svg.mono(x + 8.0, y + 16.0 + at as f64 * 13.0, line);
        }
    }

    /// A rule's conclusion: an output's sets with the one it concludes in bold and clipped at the
    /// rule's score, or the item's bar filled to it.
    fn conclusion(&self, svg: &mut Svg, frame: &Frame, row: &Row) {
        svg.frame(frame);
        match &row.then {
            Then::Output { output, set: concluded } => {
                let output = &self.outputs[*output];
                let frame = frame.domain(output.range.0, output.range.1);
                for (at, set) in output.sets.iter().enumerate() {
                    let bold = at == *concluded;
                    if let (true, Some(score)) = (bold, row.score) {
                        svg.cut(&frame, set, score);
                    }
                    svg.shape(&frame, set, bold);
                    svg.label(&frame, set.middle(), &set.name, output.sets.len(), bold);
                }
            }
            Then::Item(name) => {
                let y = frame.y + frame.height / 2.0 - 6.0;
                let (x, width) = (frame.x + 6.0, frame.width - 12.0);
                svg.text(x, frame.y + 16.0, 12.0, "start", "bold", INK, &clip(name, 20));
                if let Some(score) = row.score {
                    let colour = if score >= self.threshold { YES } else { FILL };
                    svg.rect(x, y, width * score, 22.0, colour, "none", 0.0);
                    svg.text(x + width, frame.y + 16.0, 12.0, "end", "normal", INK, &format!("{score:.2}"));
                }
                svg.rect(x, y, width, 22.0, "none", INK, 1.2);
                let threshold = x + width * self.threshold;
                svg.dashed(threshold, y - 5.0, threshold, y + 27.0, MARK);
            }
        }
    }

    /// The final column: each output's merged shape and its centre, then every item against the
    /// threshold. Returns where it ends.
    fn finals(&self, svg: &mut Svg, rows: &[Row], x: f64, reply: Option<&DecisionResponse>) -> f64 {
        let scores: Option<Vec<f64>> = rows.iter().map(|row| row.score).collect();
        let mut y = TOP;
        for (at, output) in self.outputs.iter().enumerate() {
            let clipped = scores.as_ref().map(|scores| self.set_scores(at, scores));
            let value = clipped.as_ref().and_then(|clipped| output.centroid(clipped, self.logic.or));
            let caption = match (&clipped, value) {
                (Some(_), Some(value)) => format!("{} = {value:.2}", output.name),
                (Some(_), None) => format!("{}: no rule fired", output.name),
                (None, _) => output.name.clone(),
            };
            svg.text(x, y + 13.0, 12.0, "start", "bold", INK, &caption);
            let frame = Frame { x, y: y + CAPTION, width: FINAL_WIDTH, height: PANEL_HEIGHT, low: output.range.0, high: output.range.1 };
            svg.frame(&frame);
            if let Some(clipped) = &clipped {
                let points: Vec<(f64, f64)> = output.samples(241).map(|at| (at, output.merged(clipped, self.logic.or, at))).collect();
                svg.area(&frame, &points);
            }
            for set in &output.sets {
                svg.shape(&frame, set, false);
                svg.label(&frame, set.middle(), &set.name, output.sets.len(), false);
            }
            if let Some(value) = value {
                svg.marker(&frame, value, &format!("{value:.2}"));
            }
            svg.text(frame.x, frame.bottom() + 28.0, 10.0, "start", "normal", FAINT, &trim_number(output.range.0));
            svg.text(frame.x + frame.width, frame.bottom() + 28.0, 10.0, "end", "normal", FAINT, &trim_number(output.range.1));
            y += ROW + 12.0;
        }

        let items = self.item_names();
        if items.is_empty() {
            return y;
        }
        let outcome = reply.and_then(|reply| self.evaluate(reply).ok());
        svg.text(x, y + 13.0, 12.0, "start", "bold", INK, &format!("items (threshold {:.2})", self.threshold));
        let top = y + CAPTION;
        let (label, bar) = (100.0, FINAL_WIDTH - 140.0);
        for (at, name) in items.iter().enumerate() {
            let line = top + 8.0 + at as f64 * 22.0;
            svg.text(x, line + 12.0, 11.0, "start", "normal", INK, &clip(name, 16));
            if let Some(item) = outcome.as_ref().and_then(|outcome| outcome.items.iter().find(|item| &item.item == name)) {
                let colour = if item.yes { YES } else { FILL };
                svg.rect(x + label, line, bar * item.score, 16.0, colour, "none", 0.0);
                let verdict = if item.yes { format!("{:.2} yes", item.score) } else { format!("{:.2}", item.score) };
                svg.text(x + label + bar + 6.0, line + 12.0, 11.0, "start", if item.yes { "bold" } else { "normal" }, INK, &verdict);
            }
            svg.rect(x + label, line, bar, 16.0, "none", FAINT, 1.0);
        }
        let bottom = top + 8.0 + items.len() as f64 * 22.0;
        let threshold = x + label + bar * self.threshold;
        svg.dashed(threshold, top + 2.0, threshold, bottom, MARK);
        bottom + 10.0
    }
}

/// The terms a rule's tree reads, in the order written.
fn terms(tree: &Node) -> Vec<String> {
    let mut out = Vec::new();
    tree.terms(&mut out);
    out
}

/// A tree's lines, drawn with box lines: `AND (min) 0.96`, `├ raining 0.96`, …
fn tree_lines(node: &Node, first: &str, rest: &str, out: &mut Vec<String>) {
    let how = if node.how.is_empty() { String::new() } else { format!(" ({})", node.how) };
    let value = node.value.map(|value| format!("  {value:.2}")).unwrap_or_default();
    out.push(format!("{first}{}{how}{value}", node.label));
    for (at, child) in node.children.iter().enumerate() {
        let last = at + 1 == node.children.len();
        let (branch, under) = if last { ("└ ", "  ") } else { ("├ ", "│ ") };
        tree_lines(child, &format!("{rest}{branch}"), &format!("{rest}{under}"), out);
    }
}

/// `text` cut to `most` characters, with `…` when it was longer.
fn clip(text: &str, most: usize) -> String {
    match text.chars().count() > most {
        true => format!("{}…", text.chars().take(most.saturating_sub(1)).collect::<String>()),
        false => text.to_owned(),
    }
}

/// A plot: where it is, and the domain its x axis covers. Heights run from 0 at the bottom to 1
/// ten pixels under the top, which leaves room for a value's label.
#[derive(Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    low: f64,
    high: f64,
}

impl Frame {
    fn new(x: f64, y: f64) -> Frame {
        Frame { x, y, width: PANEL_WIDTH, height: PANEL_HEIGHT, low: 0.0, high: 1.0 }
    }

    fn domain(&self, low: f64, high: f64) -> Frame {
        Frame { low, high, ..*self }
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn px(&self, x: f64) -> f64 {
        self.x + (x - self.low) / (self.high - self.low) * self.width
    }

    fn py(&self, degree: f64) -> f64 {
        self.bottom() - degree.clamp(0.0, 1.0) * (self.height - 10.0)
    }
}

#[derive(Default)]
struct Svg {
    body: String,
}

impl Svg {
    fn finish(self, width: f64, height: f64) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" viewBox=\"0 0 {width:.0} {height:.0}\" \
             font-family=\"-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif\">\n\
             <rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n{}</svg>\n",
            self.body
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f64, y: f64, size: f64, anchor: &str, weight: &str, fill: &str, text: &str) {
        let _ = writeln!(
            self.body,
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{size}\" text-anchor=\"{anchor}\" font-weight=\"{weight}\" fill=\"{fill}\">{}</text>",
            escape(text)
        );
    }

    fn mono(&mut self, x: f64, y: f64, text: &str) {
        let _ = writeln!(
            self.body,
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"11\" font-family=\"ui-monospace, Menlo, Consolas, monospace\" fill=\"{INK}\" \
             xml:space=\"preserve\">{}</text>",
            escape(text)
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn rect(&mut self, x: f64, y: f64, width: f64, height: f64, fill: &str, stroke: &str, stroke_width: f64) {
        let _ = writeln!(
            self.body,
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{:.1}\" height=\"{:.1}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\"/>",
            width.max(0.0),
            height.max(0.0)
        );
    }

    fn frame(&mut self, frame: &Frame) {
        self.rect(frame.x, frame.y, frame.width, frame.height, "none", FRAME, 1.0);
    }

    fn dashed(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str) {
        let _ = writeln!(
            self.body,
            "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" stroke=\"{stroke}\" stroke-width=\"1.2\" stroke-dasharray=\"4 3\"/>"
        );
    }

    fn arrow(&mut self, from: f64, to: f64, y: f64) {
        let _ = writeln!(
            self.body,
            "<line x1=\"{from:.1}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"{INK}\" stroke-width=\"1.2\"/>\n\
             <path d=\"M{to:.1},{y:.1} l-8,-4 v8 z\" fill=\"{INK}\"/>",
            to - 6.0
        );
    }

    /// A set's outline, bold when it is the one a rule reads or concludes.
    fn shape(&mut self, frame: &Frame, set: &Set, bold: bool) {
        let [a, b, c, d] = set.points;
        let points = [(a, 0.0), (b, 1.0), (c, 1.0), (d, 0.0)];
        let (stroke, width) = if bold { (INK, 2.2) } else { (FAINT, 1.0) };
        let path: Vec<String> = points.iter().map(|(x, degree)| format!("{:.1},{:.1}", frame.px(*x), frame.py(*degree))).collect();
        let _ = writeln!(
            self.body,
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{width}\" stroke-linejoin=\"round\"/>",
            path.join(" ")
        );
    }

    /// The part of `set` under `degree`, shaded, with the cut drawn across the panel and its value.
    fn cut(&mut self, frame: &Frame, set: &Set, degree: f64) {
        let [a, b, c, d] = set.points;
        let rising = a + (b - a) * degree;
        let falling = d - (d - c) * degree;
        let corners = [(a, 0.0), (rising, degree), (falling, degree), (d, 0.0)];
        let path: Vec<String> =
            corners.iter().map(|(x, y)| format!("{:.1},{:.1}", frame.px(x.clamp(frame.low, frame.high)), frame.py(*y))).collect();
        let _ = writeln!(self.body, "<polygon points=\"{}\" fill=\"{FILL}\" stroke=\"none\"/>", path.join(" "));
        let y = frame.py(degree);
        self.dashed(frame.px(falling.clamp(frame.low, frame.high)), y, frame.x + frame.width, y, INK);
        self.text(frame.x + frame.width - 2.0, y - 3.0, 10.0, "end", "bold", INK, &format!("{degree:.2}"));
    }

    /// A shape given as points along the axis, filled.
    fn area(&mut self, frame: &Frame, points: &[(f64, f64)]) {
        let mut path = vec![format!("{:.1},{:.1}", frame.px(frame.low), frame.bottom())];
        path.extend(points.iter().map(|(x, degree)| format!("{:.1},{:.1}", frame.px(*x), frame.py(*degree))));
        path.push(format!("{:.1},{:.1}", frame.px(frame.high), frame.bottom()));
        let _ = writeln!(self.body, "<polygon points=\"{}\" fill=\"{FILL}\" stroke=\"{INK}\" stroke-width=\"1\"/>", path.join(" "));
    }

    /// A set's or level's name under the axis, at `x`, cut to fit its share of the panel.
    fn label(&mut self, frame: &Frame, x: f64, name: &str, count: usize, bold: bool) {
        let room = ((frame.width / count as f64) / 6.5).max(3.0) as usize;
        let weight = if bold { "bold" } else { "normal" };
        let fill = if bold { INK } else { FAINT };
        self.text(frame.px(x), frame.bottom() + 12.0, 10.0, "middle", weight, fill, &clip(name, room));
    }

    /// An arrowhead under the axis at `x`, with no line up through the plot: a Score's expected
    /// level, which would otherwise cross the bars and their numbers.
    fn tick(&mut self, frame: &Frame, x: f64, label: &str) {
        let at = frame.px(x.clamp(frame.low, frame.high));
        let _ = writeln!(self.body, "<path d=\"M{at:.1},{:.1} l-4,7 h8 z\" fill=\"{MARK}\"/>", frame.bottom() + 14.0);
        self.text(at, frame.bottom() + 30.0, 10.0, "middle", "bold", MARK, label);
    }

    /// An arrow up to the axis at `x`: an output's centre.
    fn marker(&mut self, frame: &Frame, x: f64, label: &str) {
        let at = frame.px(x.clamp(frame.low, frame.high));
        let _ = writeln!(
            self.body,
            "<line x1=\"{at:.1}\" y1=\"{:.1}\" x2=\"{at:.1}\" y2=\"{:.1}\" stroke=\"{MARK}\" stroke-width=\"1.6\"/>\n\
             <path d=\"M{at:.1},{:.1} l-4,7 h8 z\" fill=\"{MARK}\"/>",
            frame.y + 6.0,
            frame.bottom(),
            frame.bottom() + 14.0
        );
        self.text(at, frame.bottom() + 30.0, 10.0, "middle", "bold", MARK, label);
    }
}

/// Text made safe to put inside an SVG element.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text() {
        assert_eq!(escape("a < b & \"c\""), "a &lt; b &amp; &quot;c&quot;");
    }

    #[test]
    fn clips_long_labels() {
        assert_eq!(clip("Very angry", 6), "Very …");
        assert_eq!(clip("Hot", 6), "Hot");
    }
}
