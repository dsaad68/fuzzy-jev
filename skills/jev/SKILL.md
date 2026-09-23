---
name: jev
description: >-
  Ask Jev (TypeSafe's System One model, on OpenRouter's decisions endpoint) typed questions about a
  piece of text and get calibrated probabilities back instead of prose — a Choice between named
  options, a Score on an ordered scale, or a Noul, the probability that something is true. Use when
  classifying, routing, triaging, labelling, rating, scoring or flagging text, especially many items
  against one set of criteria; when a judgment needs a confidence number to gate on rather than an
  opinion; when building a filter, moderation or triage step; or whenever the `jev` command, the
  `jev` crate or OpenRouter's decisions endpoint comes up — including when the user just says "sort
  these tickets", "score these reviews" or "which of these need a human?". Not for writing,
  summarizing or rewriting text — Jev only answers questions you define.
license: MIT
compatibility: >-
  Needs OPENROUTER_API_KEY and network access to openrouter.ai. `jev` is a native binary
  (`cargo install fuzzy-jev`); jq is handy for reading `--json`.
metadata:
  source: https://github.com/dsaad68/fuzzy-jev
---

# Jev: typed questions, probabilities back

Jev doesn't write text. It reads a **state** and answers **questions** about it with probability
distributions your code can act on. Every question sees the same state and is answered
independently, so ask all of them in one call — an extra question costs a few tokens and almost no
time.

| Type | Asks | Answer |
| --- | --- | --- |
| `noul` | Is this true? | `noul`: the probability of yes. **No separate confidence** — that is the answer. |
| `choice` | Which one of these? | `choice`, `confidence`, and `probabilities` for every option. 2–255 options. |
| `score` | Which level? | `score`: the **expected** level, so it can fall between two; plus `confidence` and each level's probability. 2–10 levels, lowest first. |

## Ask

A question is `ID=INSTRUCTIONS` then `|`-separated criteria. Each flag repeats; answers print in the
order written.

```sh
jev 'Our card was declined and the account is now suspended — nobody can get in.' \
  --choice 'team=Which team should handle this? Route by what the sender is asking for, not by what happened to them.|billing:how the customer is charged or pays us|support:the product not working or not understood|security:who can get into the account' \
  --noul 'blocked=Is the sender blocked from working right now?' \
  --score 'urgency=How urgent is this?|No deadline|This week|Today'
```

```text
team     billing  confidence 0.79  (billing 0.86, security 0.12, support 0.02)
blocked  0.90 yes
urgency  1.98 of 2, nearest "Today"  confidence 0.97
420 tokens in, 69 out, $0.000018, typesafe/jev-1.13-20260917
```

| Flag | |
| --- | --- |
| `STATE` / `-f PATH` | The state as an argument or from a file (`-` for stdin); stdin is read when it isn't a terminal. |
| `--state-json` | Parse the state as JSON, so questions can point at named parts. |
| `--noul` / `--choice` / `--score` | A question, repeatable. Noul criteria: none, or `\|YES MEANS\|NO MEANS`. Choice: two or more `NAME[:DESCRIPTION]`. Score: two to ten levels, lowest first. |
| `-q FILE` | Questions from a JSON file — the only way to use text containing `\|`, or structured criteria. |
| `-r FILE` | Fuzzy rules over the answers (TOML, [below](#decide-with-rules)): prints each outcome's score instead of the answers. |
| `--graph` / `--svg PATH` | With `-r`: draw the rules as a tree per rule in the terminal, or as an SVG rule-base diagram. Without a state, the structure alone and no call. |
| `--json` / `--table` | The reply as sent, for `jq`; or a table. The default is one line per question. |
| `-m ID` | Another model. Default `typesafe/jev-1.13`. |
| `--dry-run` | Print the request instead of sending it. **Needs no key** — check a question before paying for it. |

A `-q` file is a JSON object of ids to questions, kept in order. A Choice's `criteria` maps option
to description, a Score's is an array lowest first, a Noul's is optional `{"true":…, "false":…}`:

```json
{"team":    {"type": "choice", "instructions": "Which team?", "criteria": {"billing": "charges and invoices", "support": "the product misbehaving", "other": "neither"}},
 "urgency": {"type": "score",  "instructions": "How soon does this need an answer?", "criteria": ["No deadline", "This week", "Today"]}}
```

## Read

```json
{"type": "noul",   "noul": 0.9}
{"type": "choice", "choice": "billing", "confidence": 0.79, "probabilities": {"billing": 0.86, "security": 0.12}}
{"type": "score",  "score": 1.98, "confidence": 0.97, "probabilities": {"0": 0, "1": 0.02, "2": 0.98},
                   "legend": {"0": "No deadline", "1": "This week", "2": "Today"}}
```

A Score's `probabilities` and `legend` are keyed by level number **as strings**; the reply also
carries `usage.cost` in US dollars.

```sh
jev -f ticket.txt -q questions.json --json > reply.json
jq -r '.answers.team | select(.confidence > 0.6) | .choice'      reply.json
jq -r '.answers.urgency | .legend[(.score | round | tostring)]'  reply.json
```

## Decide with rules

When the decision is several answers combined ("raining and not hot → raincoat"), write it as rules
rather than as `jq` arithmetic. A rules file names answers as **terms** and combines them with fuzzy
logic; every answer is already a degree from 0 to 1:

```toml
[terms]                       # lowercase names for answers
hot     = "temp.Hot"          # a Score level, by its exact text
humid   = "humidity.Humid"
raining = "raining"           # a Noul, by its id
# stormy  = "sky.storm"       # a Choice option, from a "sky" choice: clear, cloudy, storm

[[rule]]
if   = "raining AND NOT hot"  # AND OR NOT ( ), hedges VERY SOMEWHAT EXTREMELY INDEED — uppercase
then = "raincoat"

[[rule]]
if     = "VERY humid OR hot"
then   = "breathable fabric"
weight = 0.8                  # optional, 0–1
```

```sh
jev -f weather.txt -q weather.json -r wear.toml
```

```text
raincoat           0.96  yes
breathable fabric  0.80  yes
threshold 0.50
```

AND is `min`, OR is `max`, NOT is `1 − x`; rules with the same `then` are joined by OR. An item is
a yes at or over `[decide] threshold` (0.5). `--table` adds the rules behind each score, and `--json`
prints `{"reply", "outcome"}`. The file is checked against the questions before the call, so a term
naming a level that doesn't exist fails for free, `--dry-run` included. Read
`references/patterns.md` for `[logic]` (other ANDs and ORs), outputs, and how to design the rules.

For an **amount** rather than a yes/no ("how much to water"), declare an output and conclude in its
sets. The value is the centroid of the clipped sets:

```toml
[output.irrigation]
range = [0, 100]
drops = [0, 0, 20, 40]       # trapezoid a, b, c, d
liter = [30, 50, 70]         # triangle a, peak, c

[[rule]]
if   = "regular"
then = "irrigation IS liter"
```

To check the rules before trusting them, `jev -q q.json -r rules.toml --graph` shows every rule's
tree with no call. With a state, it shows the number at every node, so you can see which rule
decided.

## Write criteria that decide something

Criteria exist for the cases that are nearly one thing and nearly another; the obvious ones answer
themselves. Say what puts an item on each side of the line:

```sh
--score 'quality=How good is this review?|Bad|OK|Good'                       # the boundary is anyone's guess
--score 'quality=How useful is this to someone deciding whether to buy?|Says nothing specific — "great product"|Names one concrete thing|Names several, with the trade-offs between them'
```

Put the tie-break rule in the instructions ("route by what the sender is asking for, not by what
happened to them"), and add an `other` option when the list may not cover every input.

## Patterns

Read `references/patterns.md` when building a routing, triage or ranking step rather than asking a
one-off question; it works each of these through with `jev` and `jq`.

- **Fan-out** — ask every question you might need in one call, speculative ones included, and let
  your code pick what matters. They run in parallel, so extra questions barely cost anything.
- **Confidence routing** — the answer says what, confidence says whether to act. Gate each action on
  a threshold matched to its stakes, and send the rest to a person.
- **Intent routing** — classify cheaply first, then hand off to the right handler, instead of
  putting every request through an expensive model.
- **Composite scoring** — score independent dimensions separately, normalize each to 0–1, weight
  them and sum. Adjustable, and the individual scores stay visible.
- **Fuzzy rules** — `-r`: IF–THEN rules over the answers decide what to do, readable and editable
  by the person who owns the decision.

## Many items

One call per invocation, so a batch is `xargs`:

```sh
ls emails/*.txt | xargs -P 8 -I{} sh -c '
  id=$(basename "$1" .txt)
  jev -f "$1" -q questions.json --json |
    jq -r --arg id "$id" "[\$id, .answers.team.choice, (.answers.team.confidence|tostring)] | @tsv"
' _ {}
```

Rows arrive as they finish, so sort if order matters.

## Gotchas

- **Jev sees only the state** — not your files, not the conversation, not the question ids. Quote
  into the state everything it needs.
- **A Noul has no `confidence` field.** Its probability is the answer.
- **A Score's `score` is an expectation, not an index.** `1.98 of 2` is nearly the top level;
  `1.2 of 2` is genuinely between two. Round only when you need a discrete label.
- **Refused before anything is sent** (so it costs nothing): a repeated question id, a Choice with
  under two options, a Score outside two to ten levels, an empty state, the state and `-q` both on
  stdin. Anything else is OpenRouter's own error.
- **Nothing else is validated.** Jev will answer a badly-posed question with a confident-looking
  distribution. Garbage criteria, garbage answer.
- **Deciding one thing, once, with the context already in front of you? Just decide it.** In this
  repo's evals, routing every label of a 20-email classification through Jev cost 4–6× and did not
  classify better than the agent deciding for itself.
