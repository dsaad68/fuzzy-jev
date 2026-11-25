//! `jev model list`: the models on the decisions endpoint, as OpenRouter lists them now, with what
//! each one reads and can't answer. Nothing here is kept in jev, so a model OpenRouter adds is
//! listed the day it does.
//!
//! It is read before the main command line, as `jev add skill` is: the state is a positional
//! argument, so `model` would otherwise be the state to judge.

use std::time::Duration;

use clap::{Parser, Subcommand};
use jev::print::{self, Format};
use jev::Client;

#[derive(Parser)]
#[command(name = "jev model", about = "The models on the decisions endpoint")]
struct Model {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print every model OpenRouter offers now, in a table: its context window, its price, what it
    /// reads, and what it can't answer. Needs no key
    List(List),
}

/// `jev model list`'s flags.
#[derive(clap::Args)]
pub struct List {
    /// Print the list as JSON, for scripts
    #[arg(long)]
    json: bool,

    /// How wide the table may be; the terminal's width by default
    #[arg(long, value_name = "COLUMNS")]
    width: Option<usize>,

    /// Where the list is
    #[arg(long, default_value = jev::MODELS_URL)]
    url: String,

    /// Give up after this many seconds
    #[arg(long, value_name = "SECONDS", default_value_t = jev::DEFAULT_TIMEOUT.as_secs_f64(), value_parser = super::seconds)]
    timeout: f64,
}

/// `jev model …`, read; `None` for any other command line, which the main parser then reads. A line
/// that goes on with a flag, `jev model --noul 'q=?'`, judges the state `model`, as it always has.
/// Asking for help, or a mistake, exits here as clap does.
pub fn command(words: &[&str]) -> Option<List> {
    read(words).map(|read| read.unwrap_or_else(|error| error.exit()))
}

/// What `command` reads, without exiting on a mistake: what the tests use.
fn read(words: &[&str]) -> Option<Result<List, clap::Error>> {
    match words {
        ["model", next, ..] if next.starts_with('-') && !matches!(*next, "-h" | "--help") => None,
        ["model", rest @ ..] => Some(
            Model::try_parse_from(std::iter::once("jev model").chain(rest.iter().copied()))
                .map(|Model { command: Command::List(list) }| list),
        ),
        _ => None,
    }
}

impl List {
    /// Asks OpenRouter for the list, and prints it.
    pub async fn run(self) -> anyhow::Result<()> {
        let format = if self.json { Format::Json } else { Format::Table };
        list_models(&self.url, self.timeout, format, super::width(self.width)).await
    }
}

/// Prints the models listed at `url`, in `format`. The list is public, so no key is sent.
pub async fn list_models(url: &str, timeout: f64, format: Format, width: usize) -> anyhow::Result<()> {
    let models = Client::new("").with_timeout(Duration::from_secs_f64(timeout)).list_models(url).await?;
    print!("{}", print::render_models(format, &models, width)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_jev_model_list() {
        let list = read(&["model", "list"]).unwrap().unwrap();
        assert!(!list.json);
        assert_eq!((list.url.as_str(), list.timeout, list.width), (jev::MODELS_URL, 60.0, None));
        let list = read(&["model", "list", "--json", "--width", "120", "--timeout", "5", "--url", "http://x/models"]).unwrap().unwrap();
        assert!(list.json);
        assert_eq!((list.url.as_str(), list.timeout, list.width), ("http://x/models", 5.0, Some(120)));
    }

    #[test]
    fn says_what_is_wrong_with_it() {
        for wrong in [&["model"][..], &["model", "lst"], &["model", "list", "--noul", "q=?"], &["model", "list", "--timeout", "0"]] {
            assert!(read(wrong).unwrap().is_err(), "{wrong:?}");
        }
        let help = read(&["model", "--help"]).unwrap().err().unwrap().to_string();
        assert!(help.contains("list") && help.contains("Print every model OpenRouter offers now"), "{help}");
    }

    #[test]
    fn leaves_a_state_called_model_to_the_main_command_line() {
        assert!(read(&["model", "--noul", "q=?"]).is_none());
        assert!(read(&["models"]).is_none());
        assert!(read(&["state", "model", "list"]).is_none());
        assert!(command(&["model", "--noul", "q=?"]).is_none());
    }
}
