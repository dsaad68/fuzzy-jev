# fuzzy-jev

[![crates.io](https://img.shields.io/crates/v/fuzzy-jev?logo=rust&logoColor=white)](https://crates.io/crates/fuzzy-jev)
[![Python versions](https://img.shields.io/badge/Python-3.10%2B-3776AB?logo=python&logoColor=white)](docs/python.md)
[![Rust](https://img.shields.io/badge/Rust-2021_edition-B7410E?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

A client for [OpenRouter](https://openrouter.ai)'s decision models. Ask one typed questions about
text or images and get probabilities back, rather than prose: a **Choice** between named options, a
**Score** on an ordered scale, or a **Noul**, the probability that something is true.

[35 models](#models) from 16 providers are supported, and 23 of them read [images](#images). The
default is **Jev**, the [TypeSafe](https://typesafe.ai) model this project began with and is named
after; `-m` picks another. They all sit on one endpoint
(`https://openrouter.ai/api/alpha/decisions`), so the key you need is an OpenRouter one, and `--url`
points the same questions at another endpoint.

A [rules file](#fuzzy-rules) is a policy over those answers: it reads each probability as the
degree to which a condition holds, and combines them with `AND`, `OR`, `NOT` and hedges into a
support score per decision, or into a crisp amount through [fuzzy outputs](#outputs-a-crisp-amount).
A support score is the policy's, not a probability: [what the numbers mean](docs/rules.md#what-the-numbers-mean).
`--svg` [draws the whole rule base](#drawing-the-rules). **[`docs/rules.md`](docs/rules.md)** is the
full guide to `rules.toml`: every table, operator and formula, the output formats, the errors, and
an example worked by hand.

![The weather rules drawn as a fuzzy rule base: each question's probabilities, each rule's operators, and what to wear against the threshold](docs/wear.svg)

A command for your terminal, a Rust library that also compiles for `wasm32-unknown-unknown`, a
[Python package](docs/python.md), and an [Agent Skill](#the-agent-skill) that teaches a coding agent when to ask a model instead of judging
by eye — `jev add skill` writes it into your project. The package is `fuzzy-jev`, and the library
and the command are both called `jev`.

```sh
export OPENROUTER_API_KEY=sk-or-...

jev 'Help! My payouts have been failing for 3 days.' \
  --noul 'is_urgent=Does this message convey urgency?' \
  --choice 'department=Which team should handle this?|billing:Payments, invoicing, refunds|technical:Bugs, outages, integrations|sales:Pricing, upgrades, new accounts' \
  --score 'frustration=How frustrated is the customer?|Calm|Frustrated|Very angry'
```

```text
is_urgent    0.95 yes
department   billing  confidence 0.80  (billing 0.87, technical 0.13, sales 0.00)
frustration  1.02 of 2, nearest "Frustrated"  confidence 0.97
403 tokens in, 73 out, $0.000017, typesafe/jev-1.14-20261014
```

Every question sees the same state and is answered on its own, so ask all of them in one call: an
extra question costs a few tokens and no extra round trip.

## Contents

- [Install](#install)
- [Asking](#asking)
  - [Models](#models)
  - [A structured state](#a-structured-state)
  - [Images](#images)
- [Fuzzy rules](#fuzzy-rules)
  - [Outputs: a crisp amount](#outputs-a-crisp-amount)
- [Drawing the rules](#drawing-the-rules)
- [Exploring in the browser](#exploring-in-the-browser)
- [The Agent Skill](#the-agent-skill)
- [As a library](#as-a-library)
- [From Python](#from-python)
- [Development](#development)
- [License](#license)

More in [`docs/`](docs): [the rules guide](docs/rules.md) and [the Python guide](docs/python.md).

## Install

**From PyPI.** For Python, the package is `fuzzy-jev`, imported as `jev`: see
[From Python](#from-python). Wheels are built for Linux and macOS, x86-64 and Arm, for CPython 3.10
and later.

```sh
uv add fuzzy-jev        # or: pip install fuzzy-jev
```

This is the library, for Python code; the `jev` command comes from cargo, below.

**From crates.io.** The package is `fuzzy-jev`, since `jev` on crates.io is another project; the
command it installs is `jev`. Needs a [Rust toolchain](https://rustup.rs).

```sh
cargo install fuzzy-jev --locked
```

It lands in `~/.cargo/bin`, which rustup puts on your PATH, so `jev` works in any folder.
`cargo install fuzzy-jev --locked --force` updates it; `cargo uninstall fuzzy-jev` takes it off
your PATH again.

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
with a `|` in it, or structured criteria, goes in a JSON or TOML file of ids to questions passed
with `--questions` (`-q`); file questions come first, then the flags'.

| Option | |
| --- | --- |
| `STATE` | The state, as text. Without it, it's read from `--state-file` (`-f`, `-` for standard input), or from standard input when that isn't a terminal. |
| `--state-json` | Parse the state as JSON: `cat ticket.json \| jev --state-json -q questions.json` |
| `--image` (`-i`) | A PNG or JPEG to judge, after any text; repeatable. See [Images](#images). |
| `--model` (`-m`) | Another model; the default is `~typesafe/jev-latest`. See [Models](#models). |
| `--url` | Another endpoint. With one, `OPENROUTER_API_KEY` may be unset, for an endpoint that adds the key. |
| `--text` | One line per question. The default. |
| `--table` | A table: question, type, answer, confidence, and every option's probability. |
| `--json` | The reply as this crate reads it (the fields it knows, re-encoded), for `jq` and scripts. |
| `--rules` (`-r`) | [Fuzzy rules](#fuzzy-rules) over the answers, from a TOML file; prints their outcome instead of the answers. |
| `--graph` | With `-r`: [draw the rules](#drawing-the-rules) in the terminal. |
| `--svg PATH` | With `-r`: draw the rules as an SVG image. Without a state, either one draws the structure alone, with no call. |
| `--timeout SECONDS` | Give up on the request after this long; 60 by default. It is not sent again. |
| `--dry-run` | Print the request instead of sending it. No key needed. |

### Models

35 models on the same endpoint are supported, and `-m` picks one. Every one was called to see what it
takes, since OpenRouter's listing isn't always right about it:

| Model | | Questions | Images |
| --- | --- | --- | --- |
| `~typesafe/jev-latest` | TypeSafe's Jev, whichever version is the latest. **The default.** | all | yes |
| `typesafe/jev-1.14` | TypeSafe's Jev 1.14, pinned: the same model until you change it | all | yes |
| `typesafe/jev-1.13` | TypeSafe's Jev 1.13, pinned: the same model until you change it | all | yes |
| `jaredpalmer/kev-4b` | Kev 4B, from jaredpalmer | all | no |
| `respan/span-02` | Respan's Span 02 | all | yes |
| `respan/span-01` | Respan's Span 01 | yes/no only | no |
| `respan/span-01-lite` | Span 01 Lite, the lighter Span | yes/no only | no |
| `respan/span-01-lite:free` | Span 01 Lite, OpenRouter's free variant of it | yes/no only | no |
| `~liquid/d1-latest` | Liquid's d1, whichever version is the latest | all | yes |
| `liquid/d1.1` | Liquid's d1.1, pinned | all | yes |
| `liquid/d1` | Liquid's d1, pinned: leaves OpenRouter on 2026-11-20 | all | yes |
| `~liquid/d1-ultra-latest` | Liquid's largest, d1-ultra, whichever version is the latest | all | yes |
| `liquid/d1-ultra` | Liquid's d1-ultra, pinned | all | yes |
| `~perplexity/pplx-decider-latest` | Perplexity's Decider, whichever version is the latest | all | yes |
| `perplexity/pplx-decider-v1.2` | Perplexity's Decider V1.2, pinned | all | yes |
| `~perplexity/pplx-decider-flash-latest` | Decider Flash, the smaller Decider, whichever version is the latest | all | yes |
| `perplexity/pplx-decider-flash-v1.2` | Decider Flash V1.2, pinned | all | yes |
| `parallel/decisions-fast:free` | Parallel's Decisions Fast, which also researches the live web; free | all | no |
| `baseten/mammoth-1` | Mammoth 1, Baseten's largest, 27B | all | yes |
| `baseten/elephant-1` | Elephant 1, from Baseten, 9B | all | yes |
| `baseten/joey-1` | Joey 1, Baseten's smallest, 2B | all | yes |
| `cloudflare/clef` | Cloudflare's Clef, 27B | all | yes |
| `cloudflare/clef-flash` | Clef Flash, the fast Clef, 9B | all | yes |
| `cloudflare/clef-omni` | Clef Omni, the mixture-of-experts Clef | all | yes |
| `nace-ai/drex-max` | Drex Max, Nace.AI's largest | all | yes |
| `nace-ai/drex-v2.0` | Drex v2.0, from Nace.AI | all | no |
| `nace-ai/drex-v1.5` | Drex v1.5, from Nace.AI | all | no |
| `openai/gpt-6-luna-decisions` | GPT-6 Luna, served for decisions | all | yes |
| `inception/mercury-decide` | Inception's Mercury Decide | all | yes |
| `morph/reflex` | Morph's Reflex | all | yes |
| `microsoft/microsoft-decision-1` | Microsoft's Decision 1 | all | no |
| `upstage/solar-decide` | Upstage's Solar Decide | all | no |
| `upstage/solar-decide-flash` | Solar Decide Flash, the low-latency Solar | all | no |
| `togethercomputer/tev1-4b-experimental` | Tev1 4B, Together AI's experiment | all | no |
| `scaledown/classify-1` | ScaleDown's Classify 1, and every Choice option needs a description | all | no |

```sh
jev 'It is raining.' --noul 'raining=Is it raining?' -m respan/span-01-lite
```

- **Pin a version** for answers that don't change under you: the `~` id follows TypeSafe's latest
  Jev (today `typesafe/jev-1.14`), and the reply's `model` names the one that answered, with its
  date, such as `typesafe/jev-1.14-20261014`. Liquid's and Perplexity's `~` ids work the same way.
- **The Span 01 models answer yes/no questions only**, with plain-text instructions and criteria. A
  Score, a Choice, or structured instructions asked of one is refused before any call is made:
  ``question `anger`: respan/span-01 answers yes/no questions only, and this is a score``.
  Span 02 answers every type.
- **The Span 01 models read about 3,000 tokens of a state**, and don't say so when there is more:
  they take a state of any length, read its start and end, and skip the middle. A fact in the
  middle of a long one scores as if it weren't there. OpenRouter lists no context window for them,
  so that figure is measured. Keep what matters short, or use another model.
- **Three models take fewer questions a call** than the endpoint does: `nace-ai/drex-max` 16,
  `parallel/decisions-fast:free` 100 and `morph/reflex` 128. A request with more is refused before
  it's sent; split the questions over two calls.
- **`parallel/decisions-fast:free` also researches the live web**, by Parallel's own account, so
  its answers can rest on more than the state. It is free, and it reads text only.
- **`scaledown/classify-1` needs a description on every Choice option**: `--choice
  'c=Which?|a:the first|b:the second'`, not `--choice 'c=Which?|a|b'`.
- **`liquid/d1` leaves OpenRouter on 2026-11-20.** `~liquid/d1-latest` keeps working.
- **Any other id** the endpoint serves works too, sent as it is and not checked. `jev --help`
  lists the supported ones, and so do `jev::MODELS` and `jev.MODELS`.

`jev --list-models` prints every model with its context window, its price and what it reads, as
OpenRouter lists them now, so it needs a connection but no key. `--json` gives a script the same
list, and `Client::list_models` gives it to Rust and `Client.list_models` to Python.

```text
┌──────────────────────┬───────────┬────────┬───────────────────────────┐
│ model                │   context │   $/1M │ input                     │
├──────────────────────┼───────────┼────────┼───────────────────────────┤
│ ~typesafe/jev-latest │   262,144 │ $0.042 │ text, image               │
│ respan/span-01-lite  │    ~3,000 │   free │ text                      │
│ cloudflare/clef-omni │    65,536 │ $0.150 │ text, image, audio, video │
│ scaledown/classify-1 │ 1,000,000 │ $0.040 │ text, file                │
└──────────────────────┴───────────┴────────┴───────────────────────────┘
```

Context is in tokens, and the price is per million input tokens. `--image` sends images; audio,
video and files go in a `--state-json` state, in the shape the model reads.

### A structured state

`--state-json` sends the state as JSON rather than as text, so nested fields, numbers and lists
reach the model as what they are. A support ticket, `ticket.json`:

```json
{
  "subject": "Charged twice for the October invoice",
  "plan": "enterprise",
  "opened_days_ago": 6,
  "prior_escalations": 2,
  "messages": [
    {"from": "customer", "text": "We were billed EUR 4,800 twice on 3 October. Please refund one."},
    {"from": "support", "text": "Thanks, looking into it."},
    {"from": "customer", "text": "Six days now and no answer. Our finance team is escalating."}
  ]
}
```

The three question types, in one `questions.json`. A noul's criteria are keyed `true` and `false`,
which is what the endpoint calls them — the `--noul` flag spells the same thing
`id=INSTRUCTIONS|WHAT YES MEANS|WHAT NO MEANS`:

```json
{
  "team": {
    "type": "choice",
    "instructions": "Which team should own this ticket?",
    "criteria": {
      "billing": "charges, invoices, refunds",
      "support": "the product itself, bugs, outages",
      "success": "the relationship, renewals, escalations"
    }
  },
  "urgency": {
    "type": "score",
    "instructions": "How urgently does this need a human today?",
    "criteria": ["Can wait a week", "This week", "Today", "Now"]
  },
  "churn_risk": {
    "type": "noul",
    "instructions": "Is this account at risk of leaving?",
    "criteria": {"true": "Threats, repeated escalation, money at stake", "false": "Routine, patient, one-off"}
  }
}
```

```sh
cat ticket.json | jev --state-json -q questions.json
```

The same questions in TOML, as `questions.toml`, ask the same thing: a `[[question]]` table per
question, in the order asked, with its id as `name` and its instructions as `instruction`. TOML has
no `null`, so a choice's options without descriptions are a list of names,
`criteria = ["billing", "support"]`; JSON takes that list too. A name ending in `.json` or `.toml`
says which a file is; otherwise (standard input, say) one that starts with `{` is JSON.

```toml
[[question]]
name = "team"
type = "choice"
instruction = "Which team should own this ticket?"
criteria = { billing = "charges, invoices, refunds", support = "the product itself, bugs, outages", success = "the relationship, renewals, escalations" }

[[question]]
name = "urgency"
type = "score"
instruction = "How urgently does this need a human today?"
criteria = ["Can wait a week", "This week", "Today", "Now"]

[[question]]
name = "churn_risk"
type = "noul"
instruction = "Is this account at risk of leaving?"
criteria = { true = "Threats, repeated escalation, money at stake", false = "Routine, patient, one-off" }
```

```text
team        billing  confidence 0.97  (billing 0.98, success 0.02, support 0.00)
urgency     2.58 of 3, nearest "Now"  confidence 0.59
churn_risk  0.86 yes
571 tokens in, 72 out, $0.000024, typesafe/jev-1.14-20261014
```

The numbers move a little from call to call, so a threshold is worth setting with a margin rather
than at the value one run happened to give.

With `--json`, each answer is named after its type — `.noul`, `.choice` (with `.confidence` and
`.probabilities`), `.score` — which is what a script gates on:

```sh
cat ticket.json | jev --state-json -q questions.json --json > answers.json
jq -r 'if .answers.churn_risk.noul > 0.8 then "page the account team" else "queue normally" end' answers.json
```

### Images

`--image` (`-i`) puts a PNG or JPEG in the state, after any text:

```sh
jev 'Returned by a customer.' -i parcel.jpg --noul 'damaged=Is the parcel damaged?'
jev -i shelf.jpg --score 'stock=How full is the shelf?|Empty|Half full|Full' -m '~liquid/d1-latest'
```

#### Several images

Give each image its own `-i`. They go to the model in the order written, and every question sees
all of them, so a question should say what to look for across them:

```sh
jev 'One parcel, from three sides.' -i front.jpg -i side.jpg -i back.jpg \
  --noul 'damaged=Is the parcel damaged in any of these photos?'
```

For a folder of images, build the flags in a loop:

```sh
args=(); for photo in photos/*.jpg; do args+=(-i "$photo"); done
jev 'One parcel, from every side.' "${args[@]}" --noul 'damaged=Is the parcel damaged in any of these photos?'
```

`-i photos/*.jpg` doesn't work: `-i` takes one path, so the shell's second file becomes the text of
the state, and only one image is sent. A third is an error.

To get an answer for each image rather than one for them all, make one call per image:

```sh
ls photos/*.jpg | xargs -P 8 -I{} sh -c '
  jev -i "$1" --noul "damaged=Is the parcel damaged?" --json |
    jq -r --arg photo "$1" "[\$photo, .answers.damaged.noul] | @tsv"
' _ {}
```

How many fit in one request depends on the model. `jev` doesn't count them: over the limit, the
endpoint refuses the whole request, and there are no partial answers.

| Images in a request | Models |
| --- | --- |
| 4 | `respan/span-02`, `nace-ai/drex-max`, the three `cloudflare/clef` models |
| 8 | Liquid's and Perplexity's |
| 16 | Baseten's, `inception/mercury-decide`, `morph/reflex` |
| 64 or more | Jev, `openai/gpt-6-luna-decisions` |

These come from calling each model with small images, and the models don't all state them:
`cloudflare/clef` and `cloudflare/clef-flash` failed at 5 without saying why, Mercury took 16 and
refused 32, Morph says 16 itself when it refuses 17, and 64 is the most that was tried. Large photos hit other limits first. Jev takes 16 MiB
per image and 32 MiB per request, base64 included, so about 24 MiB of files, and every image also
uses up the model's context.

#### What to know about images

- **23 of the models read images**, marked in the [table](#models). One that doesn't is refused
  before any call is made: `nace-ai/drex-v2.0 reads text only, and this state has images in it`.
  Any other id is sent as it is.
- **With an image, the text comes only from `STATE` or `--state-file`.** Standard input isn't read
  unless `-f -` asks for it. `--state-json` doesn't combine with `--image`.
- **Sizes differ by model**, so they are left to the endpoint: Jev refuses a side under 280 pixels
  or over 1568, and the others took a 64-pixel image. The format is checked before the call.
- **Ask what is in an image, not where it comes in the list.** Given two, `typesafe/jev-1.13` and
  Decider Flash mixed up which one was second.

In Rust, a state with images is an array of `jev::text_part` and `jev::image_part` items, with an
`image_part` for each image:

```rust
let state = serde_json::json!([jev::text_part("One parcel, from two sides."), jev::image_part(&front)?, jev::image_part(&back)?]);
```

These are OpenRouter's own `image_url` parts, not the `assets` map TypeSafe documents. Only Jev reads
`assets` everywhere: most other models refuse a reference inside text or an object, and Liquid's
take a reference that is the whole state as text and answer without having seen the image.

## Fuzzy rules

> **The complete reference is [`docs/rules.md`](docs/rules.md).** This section is the short tour.

A decision is often several answers combined: "a raincoat when it rains, unless it's hot". A
**rules file** says that directly. It names the answers it needs as **terms**, combines them with
fuzzy logic, and gives each outcome a **support score**. The rules are your policy; the model supplies the
evidence. Reading a probability as a degree is a modelling choice, so a score says how strongly the
policy supports an outcome, not how likely it is.

The questions, [`examples/rules/weather.json`](examples/rules/weather.json):

```json
{
  "temp":     {"type": "score",
               "instructions": "How warm is it outside? Cold is under 10 °C, Mild 10 to 22 °C, Hot over 22 °C; …",
               "criteria": ["Cold", "Mild", "Hot"]},
  "humidity": {"type": "score",
               "instructions": "How humid is the air? Dry is under 40% relative humidity, Normal 40 to 70%, Humid over 70%; …",
               "criteria": ["Dry", "Normal", "Humid"]},
  "raining":  {"type": "noul",
               "instructions": "Is it raining, or about to?",
               "criteria": {"true": "Rain is falling, or the state says it is about to start", "false": "No rain, and none expected soon"}}
}
```

The rules, [`examples/rules/wear.toml`](examples/rules/wear.toml):

```toml
[logic]                  # optional
and = "min"              # min | product | lukasiewicz
or  = "max"              # max | probsum | bounded

[decide]                 # optional
threshold = 0.5

[terms]
cold    = "temp.Cold"    # a Score level, by its exact text
mild    = "temp.Mild"
hot     = "temp.Hot"
humid   = "humidity.Humid"
raining = "raining"      # a Noul, by its id
# stormy = "sky.storm"   (a Choice option, from a "sky" choice: clear, cloudy, storm)

[[rule]]
if   = "cold"
then = "coat"

[[rule]]
if   = "mild AND NOT raining"
then = "light jacket"

[[rule]]
if   = "raining AND NOT hot"
then = "raincoat"

[[rule]]
if   = "raining"
then = "umbrella"

[[rule]]
if   = "hot AND humid"
then = "t-shirt"

[[rule]]
if     = "humid OR hot"
then   = "breathable fabric"
weight = 1.0             # optional, 0 to 1; the rule's score is multiplied by it
```

```sh
jev '16°C, the air feels sticky, and a light drizzle has started.' \
  -q examples/rules/weather.json -r examples/rules/wear.toml
```

```text
coat               0.01
light jacket       0.03
raincoat           0.97  yes
umbrella           0.97  yes
t-shirt            0.01
breathable fabric  1.00  yes
threshold 0.50
475 tokens in, 47 out, $0.000020, typesafe/jev-1.14-20261014
```

- **Terms** are lowercase names, and every word in a rule is a term or an operator. A term points
  at a Noul by its id, at a Score level by the question id, a `.`, and the level's exact text, or at
  a Choice option by its name.
- **Operators** are uppercase: `AND`, `OR`, `NOT`, parentheses, and the hedges `VERY` (x²),
  `SOMEWHAT` (√x), `EXTREMELY` (x³) and `INDEED` (pushed toward 0 or 1). Hedges and `NOT` bind
  tightest, then `AND`, then `OR`. A hedge reshapes a score and moves where the threshold falls;
  it doesn't make "angry" mean "very angry".
- **`[logic]`** picks the AND and the OR for the whole file. `min` and `max`, the defaults, let the
  weakest condition and the strongest reason decide. `product` and `probsum` make every doubt lower
  the score and every reason raise it, and `probsum` counts a reason written twice twice. None of
  them is the probability of a compound event; levels of one question exclude each other, so their
  probabilities add (`bounded`).
- **Rules with the same `then`** are joined by the file's OR, and an item is a yes at or over the
  threshold.
- **Checked before the call.** Every term is resolved against the questions first, so a typo costs
  nothing, and `--dry-run` catches it too. The error names what does exist:
  ``[terms] hot: `temp` has no level `hot`; its levels are `temp.Cold`, `temp.Mild`, `temp.Hot` ``.
  An answer or a probability missing from the reply is an error, never a silent zero, and so is a
  number that isn't a probability or a distribution that can't be one rounded to two places.
- **`--table`** adds the rules behind each score, and **`--json`** prints
  `{"reply": …, "outcome": …}`, so a script keeps every answer.

### Outputs: a crisp amount

When the answer is an amount ("how long to water?") rather than a yes, a rule can conclude in an
**output**: a crisp axis with named fuzzy sets. A set's rules are joined into its score, each set is
clipped at its score, the clipped shapes are merged with the file's OR, and the value is the centre
of the merged shape, worked out analytically rather than sampled. This is Mamdani inference with centroid defuzzification.

[`examples/rules/irrigation.toml`](examples/rules/irrigation.toml), with
[`examples/rules/rain.json`](examples/rules/rain.json) asking how much it rained:

```toml
[terms]
scarce  = "rainfall.Scarce"
regular = "rainfall.Regular"
large   = "rainfall.Large"

[output.irrigation]          # minutes of watering this week
range  = [0, 100]
short  = [0, 0, 20, 40]      # a trapezoid: rises a→b, flat b→c, falls c→d
medium = [30, 50, 70]        # a triangle: a, peak, c
long   = [60, 80, 100, 100]  # a shoulder, held up to the end of the range

[[rule]]
if   = "scarce"
then = "irrigation IS long"

[[rule]]
if   = "regular"
then = "irrigation IS medium"

[[rule]]
if   = "large"
then = "irrigation IS short"
```

```sh
jev 'A fairly normal week: two moderate showers, and the soil is damp but drying at the surface.' \
  -q examples/rules/rain.json -r examples/rules/irrigation.toml
```

```text
irrigation  49.49  (short 0.02, medium 0.97, long 0.01)
```

`then = "OUTPUT IS SET"` always concludes in an output, and a misspelt one is an error that
suggests the closest name; any other `then` is an item, and one file can have both. The value says
where the support lies, not how much of it there is, so read it with its sets' scores, printed
beside it. When no rule for an output scores above zero, its value is `-` (`null` in JSON) rather
than a made-up number. The file has no units: say them in a comment, as above.

For everything together (a Choice, hedges, parentheses, a weight, `probsum`, a threshold, and items
alongside an output), see the support-triage example,
[`examples/rules/triage.toml`](examples/rules/triage.toml), which
[`docs/rules.md`](docs/rules.md#a-complete-example-worked-by-hand) works through by hand.

## Drawing the rules

`--svg PATH` draws the rules as an image, and `--graph` draws them in the terminal. Without a
state (no argument, no `-f`, nothing on standard input) or with `--dry-run`, either one draws the
structure alone and makes no call, which is a free way to check a rules file. With a state, every
part carries its number.

```sh
jev '16°C, the air feels sticky, and a light drizzle has started.' \
  -q examples/rules/weather.json -r examples/rules/wear.toml --svg wear.svg
```

That is the drawing at the top of this page. The image is laid out the way a fuzzy rule base is usually drawn, with one row per rule:

- **Premises:** a column per question, with a bar per level, option, or no and yes, filled to its
  probability; the one a rule reads is in bold. Under a Score, the red arrow is its expected level,
  drawn apart because the rules don't read it. A probability the reply leaves out is marked
  `missing`.
- **Rules:** the `if` as a tree of its operators, each with what it came to.
- **Conclusions:** an output's sets, with the rule's own set clipped at its score, or an item's bar
  against the threshold.
- **Final:** each output's merged shape with an arrow at its centroid, and every item's score.

With an output, the conclusions are its sets clipped at each rule's score, and the final column is
their merged shape with an arrow at its centroid. The irrigation rules:

```sh
jev 'A fairly normal week: two moderate showers, and the soil is damp but drying at the surface.' \
  -q examples/rules/rain.json -r examples/rules/irrigation.toml --svg irrigation.svg
```

![The irrigation rules drawn as a fuzzy rule base: each rule's set clipped, and the merged shape with its centroid](docs/irrigation.svg)

In the terminal, `--graph` prints each rule as a tree, and each term with every level's
probability (the term's own in brackets):

```text
R3  raining AND NOT hot  ⇒  raincoat
    AND (min)  0.97
    ├─ raining  0.97  ← raining: no 0.03 · [yes 0.97]
    └─ NOT (1 − x)  0.99
       └─ hot  0.01  ← temp: Cold 0.01 · Mild 0.98 · [Hot 0.01]
    ⇒ raincoat  0.97  ███████████████████▍
```

An output is drawn as a plot of its merged shape, with `↑` at its centre:

```text
  irrigation = 49.49
  1.0 ┤░░░░░░░░░░░░░               ▃▇▇▃               ░░░░░░░░░░░░░
      │░░░░░░░░░░░░░░░░         ▁▄██████▄▁         ░░░░░░░░░░░░░░░░
      │░░░░░░░░░░░░░░░░░░░    ▃▅██████████▅▄    ░░░░░░░░░░░░░░░░░░░
      │░░░░░░░░░░░░░░░░░░░░░▃▇██████████████▇▃░░░░░░░░░░░░░░░░░░░░░
      │▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▄████████████████████▄▁░░░░░░░░░░░░░░░░░░
  0.0 └─────────────────────────────┬──────────────────────────────
       0                            ↑ 49.49                     100
           short                  medium                  long
```

## Exploring in the browser

`jev --explore` opens a page in the browser for writing a state, questions and rules, and seeing
what they come to. It is the command, in a page: the same questions, the same rules engine, and the
same drawing. It takes no images yet, so `--explore` doesn't combine with `--image`.

```sh
jev --explore
jev --explore -q examples/rules/weather.json -r examples/rules/wear.toml   # start from these files
jev --explore 'Help! My payouts have been failing for 3 days.' -q examples/rules/triage.json -r examples/rules/triage.toml
```

On the left, a card each for the **state** (text, or JSON), the **questions** and the **rules**:

- **Questions** as a form (a card per question: its id, yes/no, choice or score, the instructions,
  and its options or levels) or as JSON, the same as a `-q` file. The two stay in step, and the
  JSON is pretty-printed.
- **Rules** in TOML, the same as an `-r` file. The ⓘ on the card opens a guide to writing them:
  terms, `if` / `then` / `weight`, `AND` / `OR` / `NOT` and the hedges, the `[logic]` functions,
  the threshold, and outputs.
- JSON and TOML are highlighted as they are typed, and a mistake shows under the field it is in,
  before anything is asked.
- Each card folds to a line that says what is in it. **Load an example** fills all three from
  [`examples/rules`](examples/rules), and **Reset** clears them, with an undo.

On the right, what they come to:

- **The rules diagram**, the `--svg` drawing, redrawn as the questions or the rules change, at no
  cost. It zooms, fits, pans, and can take the whole width.
- **Run** (<kbd>⌘</kbd>/<kbd>Ctrl</kbd> <kbd>↵</kbd>) asks the model. Then the **decision**, a card per
  item with its score against the threshold and each output's value, sits above the diagram:
  hovering a card lights up the rules behind it, and hovering a rule its card. The model's **answers**
  follow as probability bars, with the model, tokens and cost.
- The **text and JSON** the command prints: the outcome, the `--graph` tree, the answers, what
  `--json` prints and the request `--dry-run` shows, each with a button to save it, as is the SVG.

The page is served on 127.0.0.1 only, on any free port (`--port` picks one), and `--no-open`
prints the address without opening a browser. The key stays in the `jev` process, read from
`OPENROUTER_API_KEY` as always. The address carries a random token for this run after its `#`, and
nothing that reads your inputs or spends the key answers without it, so neither another site open in
the same browser nor another user of the machine can use it.
Without a key, the page still checks and draws the rules. It loads Fira Code from Google Fonts,
and falls back to the system's monospace font offline.

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
| `jev add skill --tool` | writes the skill for an agent whose jev is a tool it calls with JSON, rather than this command |
| `jev add skill --force` | replaces files that are there and differ |

The two files are compiled into the binary, so an installed `jev` carries its own skill and needs
no source tree to hand it over. Nothing is overwritten without `--force`: a file that is already
what would be written is left alone, so running it twice says `unchanged` rather than churning
your diff.

What it teaches is when the tool fits — classifying, routing, triaging, rating, flagging, and
anything where a confidence number beats an opinion — how to write the three question types, the
patterns for putting many items through one call, and how to design fuzzy rules over the answers. Read
[`skills/jev/SKILL.md`](skills/jev/SKILL.md) and
[`skills/jev/references/patterns.md`](skills/jev/references/patterns.md) before installing it, as
you would any instruction you're adding to a project.

## As a library

```toml
[dependencies]
jev = { package = "fuzzy-jev", version = "0.7", default-features = false }
```

```rust
let client = jev::Client::new(&std::env::var("OPENROUTER_API_KEY")?);
let reply = client
    .decide("I was charged twice this month", [("urgent", jev::Question::noul("Is this urgent?"))])
    .await?;
```

`default-features = false` leaves out the CLI's dependencies; the library then builds for
`wasm32-unknown-unknown` too, where requests go through the host's `fetch`. The `command` feature
adds question specs (`jev::spec`), the command's own printing (`jev::print`) and the rules engine
(`jev::rules`) without the command, for another program that wants to offer `jev` the way the
terminal does:

```rust
let rules = jev::rules::Rules::parse(&std::fs::read_to_string("wear.toml")?, questions.iter().map(|(id, q)| (id.as_str(), q)))?;
let outcome = rules.evaluate(&reply)?;       // items and outputs, with their scores
let svg = rules.graph_svg(Some(&reply))?;    // or graph_text, for a terminal
```

## From Python

```sh
pip install fuzzy-jev
```

```python
import jev

client = jev.Client()  # the key from OPENROUTER_API_KEY
questions = jev.load_questions(open("examples/rules/triage.json").read())
rules = jev.Rules(open("examples/rules/triage.toml").read(), questions)  # checked now, before a call

reply = client.decide("I can't log in and payroll is due today", questions)
reply.choice("team").choice, reply.noul("blocked")
outcome = rules.evaluate(reply)
outcome.yes                                        # the items at or over the threshold
open("triage.svg", "w").write(rules.graph_svg(reply))
```

The package is the Rust library underneath, so questions, replies and rules files mean the same in
both. `decide_async` is the awaitable form, and `decide` releases the GIL while it waits.
**[`docs/python.md`](docs/python.md)** is the whole guide: questions, the reply, rules, drawings,
errors, threads and asyncio, and releasing.

## Development

```sh
cargo test                          # offline
cargo test --tests -- --ignored     # a few real calls; passes without calling when the key isn't set
cargo run --example triage -- "Could you tell me what the enterprise plan costs?"
```

CI runs `cargo fmt --check`, clippy for the host and for `wasm32-unknown-unknown`, and the tests.
Pushing a tag such as `v0.2.0` builds the four binaries and puts them on a Release; the same build
can be started by hand from the Actions tab, which leaves them as artifacts.
The same tag publishes the Python package to PyPI; [building and releasing it](docs/python.md#building-and-releasing).

Extracted from [wasm-agent](https://github.com/dsaad68/wasm-agent), where this began as a client
for Jev alone, in `crates/jev`, and where dx's shell offers the same command to an agent. This repository was called
`jev-cli` until the fuzzy rules arrived; GitHub redirects the old URLs.

## License

MIT — see [LICENSE](LICENSE). The bundled Agent Skill says the same in its own frontmatter, so a
project that runs `jev add skill` carries it with the files.
