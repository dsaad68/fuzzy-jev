# jev

[![CI](https://github.com/dsaad68/jev-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/dsaad68/jev-cli/actions/workflows/ci.yml)
[![Release](https://github.com/dsaad68/jev-cli/actions/workflows/release.yml/badge.svg)](https://github.com/dsaad68/jev-cli/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/Rust-2021_edition-B7410E?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platforms](https://img.shields.io/badge/binaries-Linux%20%7C%20macOS-informational)](https://github.com/dsaad68/jev-cli/releases)
[![OpenRouter](https://img.shields.io/badge/endpoint-OpenRouter%20decisions-6566F1)](https://openrouter.ai)

Ask [Jev](https://openrouter.ai) typed questions about a piece of text and get calibrated
probabilities back, rather than prose. A **Choice** between named options, a **Score** on an
ordered scale, or a **Noul** — the probability that something is true.

A command for your terminal, and a Rust library that also compiles for `wasm32-unknown-unknown`.

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

**From source**, with a [Rust toolchain](https://rustup.rs):

```sh
cargo install --git https://github.com/dsaad68/jev-cli
```

Then set `OPENROUTER_API_KEY` from an [OpenRouter key](https://openrouter.ai/keys). `--dry-run`
prints the request instead of sending it, and needs no key.

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

`jev add skill` writes an [Agent Skill](https://code.claude.com/docs/en/skills) into a project, so
a coding agent knows when to reach for this rather than judging by eye.

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
