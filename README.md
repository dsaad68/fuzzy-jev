# jev

[![CI](https://github.com/dsaad68/jev-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/dsaad68/jev-cli/actions/workflows/ci.yml)
[![Release](https://github.com/dsaad68/jev-cli/actions/workflows/release.yml/badge.svg)](https://github.com/dsaad68/jev-cli/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2021_edition-B7410E?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/binaries-Linux%20%7C%20macOS-informational)](https://github.com/dsaad68/jev-cli/releases)
[![OpenRouter](https://img.shields.io/badge/endpoint-OpenRouter%20decisions-6566F1)](https://openrouter.ai)

Ask [Jev](https://openrouter.ai) typed questions about a piece of text and get calibrated
probabilities back, rather than prose. A **Choice** between named options, a **Score** on an
ordered scale, or a **Noul** — the probability that something is true.

A command for your terminal, a Rust library that also compiles for `wasm32-unknown-unknown`, and
an [Agent Skill](#the-agent-skill) that teaches a coding agent when to ask Jev instead of judging
by eye — `jev add skill` writes it into your project.

```sh
export OPENROUTER_API_KEY=sk-or-...

jev 'Help! My payouts have been failing for 3 days.' \
  --noul 'is_urgent=Does this message convey urgency?' \
  --choice 'department=Which team should handle this?|billing:Payments, invoicing, refunds|technical:Bugs, outages, integrations|sales:Pricing, upgrades, new accounts' \
  --score 'frustration=How frustrated is the customer?|Calm|Frustrated|Very angry'
```

```text
is_urgent    0.95 yes
department   billing  confidence 0.84  (billing 0.89, technical 0.11, sales 0.00)
frustration  1.04 of 2, nearest "Frustrated"  confidence 0.94
427 tokens in, 73 out, $0.000018, typesafe/jev-1.13-20260917
```

Every question sees the same state and is answered on its own, so ask all of them in one call: an
extra question costs a few tokens and no extra round trip.

## Install

**A built binary.** Each [release](https://github.com/dsaad68/jev-cli/releases) carries a
`.tar.gz` per platform — Linux and macOS, x86-64 and Arm — with a `.sha256` beside it:

```sh
tar -xzf jev-0.1.0-aarch64-apple-darwin.tar.gz
./jev --help
```

**With cargo, from this repository.** No release needed, and no clone: cargo fetches the source
and builds it. Needs a [Rust toolchain](https://rustup.rs).

```sh
cargo install --git https://github.com/dsaad68/jev-cli --locked
```

It lands in `~/.cargo/bin`, which rustup puts on your PATH, so `jev` works in any folder. A few
variants:

```sh
# a particular release, rather than whatever main says today
cargo install --git https://github.com/dsaad68/jev-cli --tag v0.1.0 --locked

# a branch, to try something before it is merged
cargo install --git https://github.com/dsaad68/jev-cli --branch some-branch --locked

# over an older copy, when cargo says one is already installed
cargo install --git https://github.com/dsaad68/jev-cli --locked --force
```

`--locked` builds with the dependency versions in `Cargo.lock`, which is what CI tested; leave it
out to let cargo pick newer ones. To run it from a clone instead:

```sh
git clone https://github.com/dsaad68/jev-cli
cd jev-cli
cargo install --path . --locked      # or: cargo run -- --help
```

Then set `OPENROUTER_API_KEY` from an [OpenRouter key](https://openrouter.ai/keys). `--dry-run`
prints the request instead of sending it, and needs no key; `cargo uninstall jev` takes it off
your PATH again.

## Asking

A question on the command line is `ID=INSTRUCTIONS`, then `|`-separated criteria:

| Flag | Criteria |
| --- | --- |
| `--noul` | none, or `WHAT YES MEANS\|WHAT NO MEANS` |
| `--choice` | two or more options, each `NAME` or `NAME:DESCRIPTION` |
| `--score` | two to ten levels, lowest first |

Each flag can be repeated, and the answers print in the order the questions were written. Text
with a `|` in it, or structured criteria, goes in a JSON file of ids to questions passed with
`--questions` (`-q`); file questions come first, then the flags'.

| Option | |
| --- | --- |
| `STATE` | The state, as text. Without it, it's read from `--state-file` (`-f`, `-` for standard input), or from standard input when that isn't a terminal. |
| `--state-json` | Parse the state as JSON: `cat ticket.json \| jev --state-json -q questions.json` |
| `--model` (`-m`) | Another model; the default is `typesafe/jev-1.13`. |
| `--url` | Another endpoint. With one, `OPENROUTER_API_KEY` may be unset, for an endpoint that adds the key. |
| `--text` | One line per question. The default. |
| `--table` | A table: question, type, answer, confidence, and every option's probability. |
| `--json` | The reply as the endpoint sent it, for `jq` and scripts. |
| `--dry-run` | Print the request instead of sending it. No key needed. |

## The Agent Skill

A coding agent asked to "sort these tickets" or "which of these need a human?" will usually read
them itself and write a paragraph of opinion. The skill teaches it to reach for `jev` instead: one
call, a probability per item, and a number it can threshold on.

```sh
cd your-project
jev add skill
```

```text
created .agents/skills/jev/SKILL.md
created .agents/skills/jev/references/patterns.md
The jev skill is in .agents/skills/jev. Agents that read .agents will find it.
```

| | |
| --- | --- |
| `jev add skill` | writes it to `.agents/skills/jev`, the convention most agents read |
| `jev add skill --claude` | writes it to `.claude/skills/jev` instead |
| `jev add skill --force` | replaces files that are there and differ |

The two files are compiled into the binary, so an installed `jev` carries its own skill and needs
no source tree to hand it over. Nothing is overwritten without `--force`: a file that is already
what would be written is left alone, so running it twice says `unchanged` rather than churning
your diff.

What it teaches is when the tool fits — classifying, routing, triaging, rating, flagging, and
anything where a confidence number beats an opinion — how to write the three question types, and
the patterns for putting many items through one call. Read
[`skills/jev/SKILL.md`](skills/jev/SKILL.md) and
[`skills/jev/references/patterns.md`](skills/jev/references/patterns.md) before installing it, as
you would any instruction you're adding to a project.

## As a library

```toml
[dependencies]
jev = { git = "https://github.com/dsaad68/jev-cli", default-features = false }
```

```rust
let client = jev::Client::new(&std::env::var("OPENROUTER_API_KEY")?);
let reply = client
    .decide("I was charged twice this month", [("urgent", jev::Question::noul("Is this urgent?"))])
    .await?;
```

`default-features = false` leaves out the CLI's dependencies; the library then builds for
`wasm32-unknown-unknown` too, where requests go through the host's `fetch`. The `command` feature
adds question specs (`jev::spec`) and the command's own printing (`jev::print`) without the
command, for another program that wants to offer `jev` the way the terminal does.

## Development

```sh
cargo test                          # offline
cargo test --tests -- --ignored     # one real call; passes without calling when the key isn't set
cargo run --example triage -- "Could you tell me what the enterprise plan costs?"
```

CI runs `cargo fmt --check`, clippy for the host and for `wasm32-unknown-unknown`, and the tests.
Pushing a tag such as `v0.1.0` builds the four binaries and puts them on a Release; the same build
can be started by hand from the Actions tab, which leaves them as artifacts.

Extracted from [wasm-agent](https://github.com/dsaad68/wasm-agent), where this began as
`crates/jev` and where dx's shell offers the same command to an agent.
