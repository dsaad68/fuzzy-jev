//! The `jev` command's arguments, and what it does with them: build one request from a state and
//! the questions (from flags and a file), send it, and print the reply, or what a rules file makes
//! of it.

mod skill;

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context};
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser};
use jev::print::{self, Format};
use jev::rules::Rules;
use jev::spec::{self, Kind};
use jev::{Client, DecisionRequest};
use serde_json::Value;

pub use skill::command as skill_command;

const EXAMPLES: &str = "\
Examples:
  jev 'Help! My payouts have been failing for 3 days.' \\
    --noul 'is_urgent=Does this message convey urgency?' \\
    --choice 'department=Which team should handle this?|billing:Payments, refunds|technical:Bugs, outages|sales:Pricing' \\
    --score 'frustration=How frustrated is the customer?|Calm|Frustrated|Very angry'

  cat ticket.json | jev --state-json --questions questions.json --json

  jev '16°C, sticky air, light drizzle' -q weather.json -r wear.toml
                             fuzzy rules over the answers (AND, OR, NOT, VERY, SOMEWHAT, …):
                             prints each outcome's score, and yes at or over the threshold

  jev '16°C, sticky air, light drizzle' -q weather.json -r wear.toml --graph --svg wear.svg
  jev -q weather.json -r wear.toml --graph
                             draw the rules: a tree per rule in the terminal, and the rule base
                             as an SVG image; without a state, the structure alone and no call

  jev add skill              teach an agent in this project to use jev: writes the bundled
                             Agent Skill to .agents/skills/jev, or .claude/skills/jev with --claude.
                             A file already there and different stops it; --force replaces it

A question is ID=INSTRUCTIONS followed by `|`-separated criteria: two or more options (NAME or
NAME:DESCRIPTION) for --choice, two to ten levels from the lowest for --score, and none or
|WHAT YES MEANS|WHAT NO MEANS for --noul. A --questions file is a JSON object of ids to questions
in the endpoint's own shape. The key is read from OPENROUTER_API_KEY.";

#[derive(Parser)]
// `-v` as well as clap's `-V`: what most people type first.
#[command(
    name = "jev",
    version,
    disable_version_flag = true,
    about = "Ask Jev typed questions about a state, through OpenRouter",
    after_help = EXAMPLES
)]
pub struct Args {
    /// Print the version
    #[arg(short = 'v', visible_short_alias = 'V', long, action = clap::ArgAction::Version)]
    version: (),

    /// The state to judge, as text. Without it, it's read from --state-file or standard input
    state: Option<String>,

    /// Read the state from a file (`-` for standard input)
    #[arg(long, short = 'f', value_name = "PATH", conflicts_with = "state")]
    state_file: Option<PathBuf>,

    /// Parse the state as JSON (an object or an array) rather than sending it as text
    #[arg(long)]
    state_json: bool,

    /// A yes/no question: ID=INSTRUCTIONS[|WHAT YES MEANS|WHAT NO MEANS]; repeatable
    #[arg(long, value_name = "SPEC")]
    noul: Vec<String>,

    /// One of several options: ID=INSTRUCTIONS|NAME[:DESCRIPTION]|…; repeatable
    #[arg(long, value_name = "SPEC")]
    choice: Vec<String>,

    /// A level on a scale: ID=INSTRUCTIONS|LOWEST|…|HIGHEST; repeatable
    #[arg(long, value_name = "SPEC")]
    score: Vec<String>,

    /// Questions from a JSON file of ids to questions (`-` for standard input)
    #[arg(long, short = 'q', value_name = "PATH")]
    questions: Option<PathBuf>,

    /// Fuzzy rules over the answers, from a TOML file (`-` for standard input); prints their outcome
    /// instead of the answers
    #[arg(long, short = 'r', value_name = "PATH")]
    rules: Option<PathBuf>,

    /// The model to ask
    #[arg(long, short = 'm', default_value = jev::DEFAULT_MODEL)]
    model: String,

    /// The decisions endpoint. Another one may add the key itself, so none is needed
    #[arg(long, default_value = jev::DECISIONS_URL)]
    url: String,

    /// Give up on the request after this many seconds; it is not sent again
    #[arg(long, value_name = "SECONDS", default_value_t = jev::DEFAULT_TIMEOUT.as_secs_f64(), value_parser = seconds)]
    timeout: f64,

    /// Print one line per question (the default)
    #[arg(long, group = "format")]
    text: bool,

    /// Print a table with a column per field
    #[arg(long, group = "format")]
    table: bool,

    /// Print the reply as JSON, for scripts: the fields jev knows, re-encoded
    #[arg(long, group = "format")]
    json: bool,

    /// Draw the rules (-r) in the terminal: each rule's operators down to its terms, what it
    /// concludes, and the final scores. Without a state, the structure alone, and no call
    #[arg(long, group = "format", requires = "rules")]
    graph: bool,

    /// Draw the rules (-r) as an SVG image to PATH: premises, conclusions and the final values, a
    /// row per rule. Without a state, the structure alone, and no call
    #[arg(long, value_name = "PATH", requires = "rules")]
    svg: Option<PathBuf>,

    /// How wide a --table may be; the terminal's width by default. A table never gets narrower
    /// than its question ids and answers need, so below that it prints wider
    #[arg(long, value_name = "COLUMNS")]
    width: Option<usize>,

    /// Print the request as JSON instead of sending it
    #[arg(long)]
    dry_run: bool,
}

impl Args {
    /// How wide a table may be: what was asked for, the terminal's width, or 80 columns when the
    /// output isn't a terminal (a pipe or a file).
    fn width(&self) -> usize {
        self.width.unwrap_or_else(|| terminal_size::terminal_size().map_or(80, |(terminal_size::Width(columns), _)| usize::from(columns)))
    }

    /// How to print the reply: at most one of --text, --table and --json is given.
    fn format(&self) -> Format {
        match (self.table, self.json) {
            (true, _) => Format::Table,
            (_, true) => Format::Json,
            _ => Format::Text,
        }
    }
}

/// The arguments, and the flag questions in the order they were written, which clap keeps per flag.
pub struct Invocation {
    args: Args,
    asked: Vec<(Kind, String)>,
}

impl Invocation {
    /// The process's arguments. Asking for help, or a mistake in the flags, exits here as clap does.
    pub fn from_env() -> anyhow::Result<Invocation> {
        Invocation::from_matches(&Args::command().get_matches())
    }

    fn from_matches(matches: &ArgMatches) -> anyhow::Result<Invocation> {
        let args = Args::from_arg_matches(matches)?;
        let mut asked = Vec::new();
        for (kind, specs) in [(Kind::Noul, &args.noul), (Kind::Choice, &args.choice), (Kind::Score, &args.score)] {
            let indices = matches.indices_of(kind.flag()).into_iter().flatten();
            asked.extend(indices.zip(specs).map(|(index, spec)| (index, kind, spec.clone())));
        }
        asked.sort_by_key(|(index, _, _)| *index);
        let asked = asked.into_iter().map(|(_, kind, spec)| (kind, spec)).collect();
        Ok(Invocation { args, asked })
    }
}

/// Asks, and prints the answers.
pub async fn run(invocation: Invocation) -> anyhow::Result<()> {
    let args = &invocation.args;
    // A drawing without a state is the rules' structure, which needs no call. Standard input is read
    // here to tell: a script that pipes nothing in is asking for the structure, not an empty state.
    let draws = args.graph || args.svg.is_some();
    let mut piped = None;
    if draws && args.state.is_none() && args.state_file.is_none() {
        let dash = |path: &Option<PathBuf>| path.as_deref() == Some(Path::new("-"));
        let stdin_is_state = !std::io::stdin().is_terminal() && !dash(&args.questions) && !dash(&args.rules);
        let text = if stdin_is_state { read(Path::new("-"))? } else { String::new() };
        if text.trim().is_empty() {
            let (_, _, rules) = questions(&invocation, false)?;
            return draw(args, &rules.expect("--graph and --svg require --rules"), None);
        }
        piped = Some(text);
    }
    if draws && args.dry_run {
        let (_, _, rules) = questions(&invocation, false)?;
        return draw(args, &rules.expect("--graph and --svg require --rules"), None);
    }
    let (request, ids, rules) = request(&invocation, piped)?;
    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&request)?);
        return Ok(());
    }
    let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if key.trim().is_empty() && args.url == jev::DECISIONS_URL {
        bail!("OPENROUTER_API_KEY is not set");
    }
    let reply = Client::new(&key).with_url(&args.url).with_timeout(Duration::from_secs_f64(args.timeout)).send(&request).await?;
    match rules {
        Some(rules) if draws => {
            draw(args, &rules, Some(&reply))?;
            if args.graph {
                print!("{}", print::usage(&reply));
            } else {
                print!("{}", print::render_outcome(args.format(), &reply, &rules.evaluate(&reply)?, args.width())?);
            }
        }
        Some(rules) => print!("{}", print::render_outcome(args.format(), &reply, &rules.evaluate(&reply)?, args.width())?),
        None => print!("{}", print::render(args.format(), &reply, &ids, args.width())?),
    }
    Ok(())
}

/// Draws the rules as asked: the terminal's tree with --graph, the image with --svg.
fn draw(args: &Args, rules: &Rules, reply: Option<&jev::DecisionResponse>) -> anyhow::Result<()> {
    if args.graph {
        print!("{}", rules.graph_text(reply)?);
    }
    if let Some(path) = &args.svg {
        std::fs::write(path, rules.graph_svg(reply)?).with_context(|| format!("writing {}", path.display()))?;
        eprintln!("jev: drew {}", path.display());
    }
    Ok(())
}

/// The request to send, its question ids in the order they were asked (the file's, then the
/// flags'), and the rules to apply to the reply. The rules are checked against the questions here,
/// so a mistake in them costs no call, and `--dry-run` finds it too.
fn request(invocation: &Invocation, piped: Option<String>) -> anyhow::Result<(DecisionRequest, Vec<String>, Option<Rules>)> {
    let args = &invocation.args;
    let (questions, ids, rules) = questions(invocation, true)?;
    let request = DecisionRequest { model: args.model.clone(), state: state(args, piped)?, questions: questions.into_iter().collect() };
    Ok((request, ids, rules))
}

/// The questions in the order they were asked (the file's, then the flags'), their ids, and the
/// rules checked against them. `with_state` says whether a state will be read too: a drawing of the
/// structure reads none, so its rules or questions may have standard input to themselves.
#[allow(clippy::type_complexity)]
fn questions(
    Invocation { args, asked }: &Invocation,
    with_state: bool,
) -> anyhow::Result<(Vec<(String, jev::Question)>, Vec<String>, Option<Rules>)> {
    one_reader_of_stdin(args, with_state)?;

    let mut questions = match &args.questions {
        Some(path) => spec::questions_file(&read(path)?).map_err(|error| anyhow::anyhow!("--questions: {error}"))?,
        None => Vec::new(),
    };
    for (kind, spec) in asked {
        questions.push(spec::parse(*kind, spec).map_err(anyhow::Error::msg)?);
    }
    if questions.is_empty() {
        bail!("no questions: ask with --noul, --choice, --score or --questions (see --help)");
    }
    let mut ids: Vec<String> = Vec::with_capacity(questions.len());
    for (id, question) in &questions {
        // The limits `Client::request` checks, for a flag's question as much as a file's: a flag
        // needs at least two options or levels, and this adds the most the endpoint takes.
        question.check().map_err(|why| anyhow::anyhow!("question `{id}`: {why}"))?;
        if ids.contains(id) {
            bail!("question `{id}` is asked twice");
        }
        ids.push(id.clone());
    }

    let rules = match &args.rules {
        Some(path) => Some(
            Rules::parse(&read(path)?, questions.iter().map(|(id, question)| (id.as_str(), question)))
                .map_err(|error| anyhow::anyhow!("--rules {}: {error}", path.display()))?,
        ),
        None => None,
    };
    Ok((questions, ids, rules))
}

/// At most one of the state, `--questions` and `--rules` may come from standard input, which can be
/// read once. The state counts only when one will be read.
fn one_reader_of_stdin(args: &Args, with_state: bool) -> anyhow::Result<()> {
    let state_from_stdin = with_state && args.state.is_none() && args.state_file.as_deref().is_none_or(|path| path == Path::new("-"));
    let stdin = |path: &Option<PathBuf>| path.as_deref() == Some(Path::new("-"));
    if [state_from_stdin, stdin(&args.questions), stdin(&args.rules)].into_iter().filter(|from| *from).count() > 1 {
        bail!("only one of the state, --questions and --rules can come from standard input");
    }
    Ok(())
}

/// The state: the argument, the file, or standard input (read already when `piped`), as text or
/// as JSON.
fn state(args: &Args, piped: Option<String>) -> anyhow::Result<Value> {
    let text = match (&args.state, &args.state_file) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => read(path)?,
        (None, None) if piped.is_some() => piped.unwrap_or_default(),
        (None, None) if !std::io::stdin().is_terminal() => read(Path::new("-"))?,
        (None, None) => bail!("no state: give it as an argument, with --state-file, or on standard input"),
    };
    if text.trim().is_empty() {
        bail!("the state is empty");
    }
    if args.state_json {
        return serde_json::from_str(&text).context("--state-json: the state isn't JSON");
    }
    Ok(Value::String(text.trim_end_matches(['\n', '\r']).to_owned()))
}

/// `--timeout`: a number of seconds over zero.
fn seconds(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() && seconds > 0.0 && seconds < 1e9 => Ok(seconds),
        _ => Err("a number of seconds over zero, such as 30 or 2.5".to_owned()),
    }
}

/// A file's text, or standard input's for `-`.
fn read(path: &Path) -> anyhow::Result<String> {
    if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text).context("reading standard input")?;
        return Ok(text);
    }
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn invocation(argv: &[&str]) -> Invocation {
        let matches = Args::command().try_get_matches_from(std::iter::once("jev").chain(argv.iter().copied())).unwrap();
        Invocation::from_matches(&matches).unwrap()
    }

    #[test]
    fn keeps_the_questions_in_the_order_written() {
        let asked = invocation(&["state", "--score", "s=?|a|b", "--noul", "n=?", "--choice", "c=?|x|y", "--noul", "m=?"]).asked;
        let kinds: Vec<_> = asked.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(kinds, [Kind::Score, Kind::Noul, Kind::Choice, Kind::Noul]);
        assert_eq!(asked[3].1, "m=?");
    }

    #[test]
    fn a_drawing_of_the_structure_leaves_stdin_to_the_rules() {
        // `cat rules.toml | jev -q q.json -r - --graph` reads no state, so the rules may have stdin.
        let structure = invocation(&["-q", "q.json", "-r", "-", "--graph"]);
        assert!(one_reader_of_stdin(&structure.args, false).is_ok());
        // Asking about a state from stdin as well is still two readers.
        assert!(one_reader_of_stdin(&structure.args, true).is_err());
    }

    #[test]
    fn draws_only_with_rules() {
        assert!(Args::command().try_get_matches_from(["jev", "state", "--noul", "n=?", "--graph"]).is_err());
        assert!(Args::command().try_get_matches_from(["jev", "state", "--noul", "n=?", "--svg", "x.svg"]).is_err());
        assert!(Args::command().try_get_matches_from(["jev", "state", "-r", "r.toml", "--graph", "--json"]).is_err());
        assert!(Args::command().try_get_matches_from(["jev", "state", "-r", "r.toml", "--svg", "x.svg", "--json"]).is_ok());
    }

    #[test]
    fn takes_a_timeout_in_seconds() {
        assert_eq!(invocation(&["state"]).args.timeout, 60.0);
        assert_eq!(invocation(&["state", "--timeout", "2.5"]).args.timeout, 2.5);
        for wrong in ["0", "-1", "soon", "inf"] {
            assert!(Args::command().try_get_matches_from(["jev", "state", "--timeout", wrong]).is_err(), "{wrong}");
        }
    }

    #[test]
    fn takes_one_output_format() {
        assert_eq!(invocation(&["state"]).args.format(), Format::Text);
        assert_eq!(invocation(&["state", "--text"]).args.format(), Format::Text);
        assert_eq!(invocation(&["state", "--table"]).args.format(), Format::Table);
        assert_eq!(invocation(&["state", "--json"]).args.format(), Format::Json);
        assert!(Args::command().try_get_matches_from(["jev", "state", "--table", "--json"]).is_err());
    }

    #[test]
    fn builds_the_request() {
        let (request, ids, _) = request(
            &invocation(&[
                "Help! My payouts have been failing for 3 days.",
                "--model",
                "typesafe/jev-latest",
                "--noul",
                "is_urgent=Does this message convey urgency?",
                "--score",
                "frustration=How frustrated is the customer?|Calm|Frustrated|Very angry",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(ids, ["is_urgent", "frustration"]);
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "model": "typesafe/jev-latest",
                "state": "Help! My payouts have been failing for 3 days.",
                "questions": {
                    "is_urgent": {"type": "noul", "instructions": "Does this message convey urgency?"},
                    "frustration": {"type": "score", "instructions": "How frustrated is the customer?", "criteria": ["Calm", "Frustrated", "Very angry"]}
                }
            })
        );
    }

    #[test]
    fn reads_the_state_as_json() {
        let (request, _, _) =
            request(&invocation(&[r#"{"message": "hi", "order": {"id": "A-104"}}"#, "--state-json", "--noul", "n=?"]), None).unwrap();
        assert_eq!(request.state, json!({"message": "hi", "order": {"id": "A-104"}}));
        assert!(request_error(&["not json", "--state-json", "--noul", "n=?"]).contains("isn't JSON"));
    }

    fn request_error(argv: &[&str]) -> String {
        format!("{:#}", request(&invocation(argv), None).unwrap_err())
    }

    #[test]
    fn refuses_what_it_cant_send() {
        assert!(request_error(&["state"]).contains("no questions"));
        assert!(request_error(&["state", "--noul", "a=?", "--noul", "a=Again?"]).contains("asked twice"));
        assert!(request_error(&["  ", "--noul", "a=?"]).contains("empty"));
        assert!(request_error(&["state", "--choice", "a=?|one"]).contains("at least two options"));
        assert!(request_error(&["--state-file", "-", "--questions", "-"]).contains("only one of"));
        assert!(request_error(&["--questions", "q.json", "--rules", "-"]).contains("only one of"));
        // A flag's choice is held to the endpoint's limit too, not only a file's.
        let many: Vec<String> = (0..256).map(|at| format!("o{at}")).collect();
        let error = request_error(&["state", "--choice", &format!("c=?|{}", many.join("|"))]);
        assert!(error.contains("question `c`: a choice takes up to 255 options, not 256"), "{error}");
    }

    #[test]
    fn checks_the_rules_before_asking() {
        let dir = std::env::temp_dir().join(format!("jev-rules-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            path.display().to_string()
        };
        let good = file("good.toml", "[terms]\nhot = \"temp.Hot\"\n[[rule]]\nif = \"VERY hot\"\nthen = \"shorts\"\n");
        let bad = file("bad.toml", "[terms]\nhot = \"temp.hot\"\n[[rule]]\nif = \"hot\"\nthen = \"shorts\"\n");
        let temp = "temp=How warm is it?|Cold|Mild|Hot";

        let (_, _, rules) = request(&invocation(&["state", "--score", temp, "-r", &good]), None).unwrap();
        assert!(rules.is_some());
        let error = request_error(&["state", "--score", temp, "-r", &bad]);
        assert!(error.contains("--rules") && error.contains("`temp` has no level `hot`"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
