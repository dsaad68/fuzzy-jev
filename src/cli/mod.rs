//! The `jev` command's arguments, and what it does with them: build one request from a state and
//! the questions (from flags and a file), send it, and print the reply.

mod skill;

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser};
use jev::print::{self, Format};
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

  jev add skill              teach an agent in this project to use jev: writes the bundled
                             Agent Skill to .agents/skills/jev, or .claude/skills/jev with --claude.
                             A file already there and different stops it; --force replaces it

A question is ID=INSTRUCTIONS followed by `|`-separated criteria: two or more options (NAME or
NAME:DESCRIPTION) for --choice, two to ten levels from the lowest for --score, and none or
|WHAT YES MEANS|WHAT NO MEANS for --noul. A --questions file is a JSON object of ids to questions
in the endpoint's own shape. The key is read from OPENROUTER_API_KEY.";

#[derive(Parser)]
#[command(name = "jev", about = "Ask Jev typed questions about a state, through OpenRouter", after_help = EXAMPLES)]
pub struct Args {
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

    /// The model to ask
    #[arg(long, short = 'm', default_value = jev::DEFAULT_MODEL)]
    model: String,

    /// The decisions endpoint. Another one may add the key itself, so none is needed
    #[arg(long, default_value = jev::DECISIONS_URL)]
    url: String,

    /// Print one line per question (the default)
    #[arg(long, group = "format")]
    text: bool,

    /// Print a table with a column per field
    #[arg(long, group = "format")]
    table: bool,

    /// Print the reply as the endpoint sent it, for scripts
    #[arg(long, group = "format")]
    json: bool,

    /// How wide a --table may be; the terminal's width by default
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
    let (request, ids) = request(&invocation)?;
    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&request)?);
        return Ok(());
    }
    let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if key.trim().is_empty() && args.url == jev::DECISIONS_URL {
        bail!("OPENROUTER_API_KEY is not set");
    }
    let reply = Client::new(&key).with_url(&args.url).send(&request).await?;
    print!("{}", print::render(args.format(), &reply, &ids, args.width())?);
    Ok(())
}

/// The request to send, and its question ids in the order they were asked: the file's, then the
/// flags'.
fn request(Invocation { args, asked }: &Invocation) -> anyhow::Result<(DecisionRequest, Vec<String>)> {
    let state_from_stdin = args.state.is_none() && args.state_file.as_deref().is_none_or(|path| path == Path::new("-"));
    if state_from_stdin && args.questions.as_deref() == Some(Path::new("-")) {
        bail!("the state and --questions can't both come from standard input");
    }

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
    for (id, _) in &questions {
        if ids.contains(id) {
            bail!("question `{id}` is asked twice");
        }
        ids.push(id.clone());
    }

    let request = DecisionRequest { model: args.model.clone(), state: state(args)?, questions: questions.into_iter().collect() };
    Ok((request, ids))
}

/// The state: the argument, the file, or standard input, as text or as JSON.
fn state(args: &Args) -> anyhow::Result<Value> {
    let text = match (&args.state, &args.state_file) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => read(path)?,
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
    fn takes_one_output_format() {
        assert_eq!(invocation(&["state"]).args.format(), Format::Text);
        assert_eq!(invocation(&["state", "--text"]).args.format(), Format::Text);
        assert_eq!(invocation(&["state", "--table"]).args.format(), Format::Table);
        assert_eq!(invocation(&["state", "--json"]).args.format(), Format::Json);
        assert!(Args::command().try_get_matches_from(["jev", "state", "--table", "--json"]).is_err());
    }

    #[test]
    fn builds_the_request() {
        let (request, ids) = request(&invocation(&[
            "Help! My payouts have been failing for 3 days.",
            "--model",
            "typesafe/jev-latest",
            "--noul",
            "is_urgent=Does this message convey urgency?",
            "--score",
            "frustration=How frustrated is the customer?|Calm|Frustrated|Very angry",
        ]))
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
        let (request, _) =
            request(&invocation(&[r#"{"message": "hi", "order": {"id": "A-104"}}"#, "--state-json", "--noul", "n=?"])).unwrap();
        assert_eq!(request.state, json!({"message": "hi", "order": {"id": "A-104"}}));
        assert!(request_error(&["not json", "--state-json", "--noul", "n=?"]).contains("isn't JSON"));
    }

    fn request_error(argv: &[&str]) -> String {
        format!("{:#}", request(&invocation(argv)).unwrap_err())
    }

    #[test]
    fn refuses_what_it_cant_send() {
        assert!(request_error(&["state"]).contains("no questions"));
        assert!(request_error(&["state", "--noul", "a=?", "--noul", "a=Again?"]).contains("asked twice"));
        assert!(request_error(&["  ", "--noul", "a=?"]).contains("empty"));
        assert!(request_error(&["state", "--choice", "a=?|one"]).contains("at least two options"));
        assert!(request_error(&["--state-file", "-", "--questions", "-"]).contains("both come from standard input"));
    }
}
