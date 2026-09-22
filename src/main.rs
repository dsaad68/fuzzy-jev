//! The `jev` command: ask Jev typed questions about a state from the terminal. It lives in `cli`;
//! the library it calls is the crate itself.

mod cli;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let words: Vec<String> = std::env::args().skip(1).collect();
    // `jev add skill` writes the bundled Agent Skill. It comes before clap because the state is a
    // positional argument, so `add` would otherwise be read as the state to judge.
    if let Some(done) = cli::skill_command(&words.iter().map(String::as_str).collect::<Vec<_>>()) {
        return done;
    }
    cli::run(cli::Invocation::from_env()?).await
}
