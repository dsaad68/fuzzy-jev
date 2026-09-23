# Rules: how `rules.toml` works

Jev answers typed questions with probabilities. A **rules file** turns those probabilities into
decisions. You write the knowledge as `IF … THEN …` rules, and fuzzy logic works out how strongly
each rule holds for this particular state. This page covers the whole file, every operator, how
each number is computed, what the command prints, and the errors you can get.

- [How it fits together](#how-it-fits-together)
- [A first file](#a-first-file)
- [The file at a glance](#the-file-at-a-glance)
- [`[terms]`: naming the answers](#terms-naming-the-answers)
- [`if`: the rule language](#if-the-rule-language)
- [`[logic]`: which AND and which OR](#logic-which-and-and-which-or)
- [`then`, `weight` and `[decide]`: items](#then-weight-and-decide-items)
- [`[output.NAME]`: a crisp amount](#outputname-a-crisp-amount)
- [Running it](#running-it)
- [Drawing it](#drawing-it)
- [A complete example, worked by hand](#a-complete-example-worked-by-hand)
- [Errors](#errors)
- [From Rust](#from-rust)
- [Designing rules](#designing-rules)

The examples are in [`examples/rules/`](../examples/rules):

| Questions | Rules | Shows |
| --- | --- | --- |
| [`weather.json`](../examples/rules/weather.json) | [`wear.toml`](../examples/rules/wear.toml) | Items, Scores, a Noul, `AND` / `OR` / `NOT` |
| [`rain.json`](../examples/rules/rain.json) | [`irrigation.toml`](../examples/rules/irrigation.toml) | An output: a crisp amount from fuzzy sets |
| [`triage.json`](../examples/rules/triage.json) | [`triage.toml`](../examples/rules/triage.toml) | All of it: a Choice, hedges, parentheses, a weight, `probsum`, a threshold, items and an output together |

## How it fits together

```text
 state ──▶ Jev ──▶ answers ──▶ [terms] ──▶ degrees ──▶ [[rule]] if … ──▶ rule scores ──▶ items    (yes at the threshold)
 (text)     │     (probabilities)          (0 to 1)                                  └─▶ outputs  (a crisp value)
            └── questions.json
```

1. **Jev reads the state** and answers the questions in `questions.json`.
2. **`[terms]`** gives a short name to each answer the rules need. Each term is a **degree** from 0
   to 1.
3. **Each `[[rule]]`** combines degrees in its `if` with `AND`, `OR`, `NOT` and hedges into a
   **rule score**.
4. **Each `then`** is either an **item**, which is a yes when its score reaches the threshold, or a
   set of an **output**, which becomes a crisp value such as "2.17 hours".

The file is checked against the questions **before** Jev is called, so a mistake in it costs
nothing.

## A first file

The questions (`weather.json`):

```json
{
  "temp":     {"type": "score", "instructions": "How warm does it feel outside?", "criteria": ["Cold", "Mild", "Hot"]},
  "humidity": {"type": "score", "instructions": "How humid is the air?", "criteria": ["Dry", "Normal", "Humid"]},
  "raining":  {"type": "noul",  "instructions": "Is it raining, or about to?"}
}
```

The rules (a shortened `wear.toml`):

```toml
[terms]
hot     = "temp.Hot"
raining = "raining"

[[rule]]
if   = "raining AND NOT hot"
then = "raincoat"
```

```sh
jev '16°C, the air feels sticky, and a light drizzle has started.' -q weather.json -r wear.toml
```

```text
raincoat  0.95  yes
threshold 0.50
```

Jev gave `raining` 0.95 and `hot` 0.02, so `raining AND NOT hot` = min(0.95, 1 − 0.02) = **0.95**.
That is at or over the default threshold of 0.5, so the raincoat is a yes.

## The file at a glance

| Table | Required | Keys | Default |
| --- | --- | --- | --- |
| `[logic]` | no | `and` = `"min"`, `"product"` or `"lukasiewicz"`; `or` = `"max"`, `"probsum"` or `"bounded"` | `min`, `max` |
| `[decide]` | no | `threshold`: a number from 0 to 1 | `0.5` |
| `[terms]` | yes, for any rule to use a term | `name = "target"`, one per answer the rules read | |
| `[output.NAME]` | no | `range = [low, high]`, then one set per key: `set = [a, b, c]` or `[a, b, c, d]` | |
| `[[rule]]` | at least one | `if` (text), `then` (text), `weight` (0 to 1) | `weight = 1.0` |

Any other key is an error, so a typo such as `treshold` is caught rather than ignored.

## `[terms]`: naming the answers

A term is a **lowercase name** made of `a`–`z`, `0`–`9` and `_`, starting with a letter or `_`. It
points at one answer. Because terms are lowercase and operators are uppercase, neither can be
mistaken for the other.

| Question type | Target | The term's degree | Example |
| --- | --- | --- | --- |
| Noul | `id` | the probability of yes | `raining = "raining"` |
| Score | `id.Level` | that level's probability | `hot = "temp.Hot"` |
| Choice | `id.option` | that option's probability | `billing = "team.billing"` |

- **Exact text.** The level or option must be written exactly as in the questions, including case
  and spaces: `this_week = "urgency.This week"`. `temp.hot` doesn't match `Hot`, and the error lists
  the ones that exist.
- **Only what a question has.** A Score or Choice needs a level or option (`temp` alone is an
  error), and a Noul takes none (`raining.yes` is an error).
- **Dots in ids.** When one question id is a prefix of another (`weather` and `weather.temp`), the
  longer one wins, so `weather.temp.Hot` is `weather.temp`'s level.
- **Structured levels.** A level written as a JSON object has no text, so no term can name it.
- **Unused terms** are allowed, which helps while editing, but they are still checked.

Where the degree comes from, for the ticket in the [worked example](#a-complete-example-worked-by-hand):

```text
urgency: No deadline 0.00 · This week 0.00 · [Today 1.00]      today = "urgency.Today"        → 1.00
team:    billing 0.11 · support 0.09 · [security 0.79] · other 0.01    security = "team.security" → 0.79
blocked: no 0.14 · [yes 0.86]                                  blocked = "blocked"            → 0.86
```

If Jev's reply is missing an answer or a probability that a term needs, that is an error, never a
zero: a zero would read as a confident "no" that nothing said.

## `if`: the rule language

### Operators

| Operator | Meaning | Degree | `a = 0.8`, `b = 0.6` |
| --- | --- | --- | --- |
| `a AND b` | both | set by `[logic] and` (default `min(a, b)`) | 0.6 |
| `a OR b` | either | set by `[logic] or` (default `max(a, b)`) | 0.8 |
| `NOT a` | not | `1 − a` | 0.2 |
| `( … )` | grouping | the inside's degree | |

### Hedges

A hedge changes how strongly a term, or a group in parentheses, holds.

| Hedge | Degree | `a = 0.7` | `a = 0.3` | Use it for |
| --- | --- | --- | --- | --- |
| `VERY a` | a² | 0.49 | 0.09 | "very hot": only strong answers stay strong |
| `EXTREMELY a` | a³ | 0.34 | 0.03 | stricter still |
| `SOMEWHAT a` | √a | 0.84 | 0.55 | "somewhat mild": weak answers still count |
| `INDEED a` | 2a² when a ≤ 0.5, else 1 − 2(1 − a)² | 0.82 | 0.18 | pushes toward 0 or 1, away from the middle |

### Precedence

From tightest to loosest:

1. **Hedges and `NOT`** apply to the next term or group: `NOT VERY hot` is `NOT (VERY hot)`.
2. **`AND`**.
3. **`OR`**: `cold OR raining AND humid` is `cold OR (raining AND humid)`.

Use parentheses whenever you mean something else: `(cold OR raining) AND humid`.

Keep in mind that `AND` binds tighter than `OR`. The triage rule
`VERY angry OR INDEED blocked AND billing` reads as `(VERY angry) OR ((INDEED blocked) AND billing)`.

### Grammar

```text
rule   = or
or     = and { "OR" and }
and    = unary { "AND" unary }
unary  = ( "NOT" | "VERY" | "SOMEWHAT" | "EXTREMELY" | "INDEED" ) unary | atom
atom   = term | "(" or ")"
term   = [a-z_] [a-z0-9_]*          (defined in [terms])
```

- Words are separated by spaces and brackets: `(hot)` and `( hot )` are the same.
- Operators are **uppercase only**. A lowercase `and` gets an error that points you to `AND`,
  rather than being read as a term.
- An `if` may be up to **256 words and brackets** long. Beyond that, split it into several rules
  with the same `then`, which are joined by OR anyway.

## `[logic]`: which AND and which OR

`[logic]` picks one AND and one OR for the whole file. The OR is used inside rules, to join rules
with the same `then`, and to merge an output's sets.

| `and` | Formula | 0.8 AND 0.6 | Use it when |
| --- | --- | --- | --- |
| `min` (default) | `min(a, b)` | 0.60 | Answers are related, as answers about one state usually are. The weakest condition decides. |
| `product` | `a × b` | 0.48 | The conditions are independent. Every doubt lowers the result. |
| `lukasiewicz` | `max(0, a + b − 1)` | 0.40 | Strict: both must clearly hold. |

| `or` | Formula | 0.8 OR 0.6 | Use it when |
| --- | --- | --- | --- |
| `max` (default) | `max(a, b)` | 0.80 | The strongest reason decides, and a second reason adds nothing. |
| `probsum` | `a + b − ab` | 0.92 | Independent reasons reinforce each other. |
| `bounded` | `min(1, a + b)` | 1.00 | Reasons add up, capped at 1. |

`min` and `max` are the safe defaults. Change them when you know why.

## `then`, `weight` and `[decide]`: items

A `then` that isn't `OUTPUT IS SET` names an **item**: any text, such as `raincoat`,
`page on-call`, or even `request IS urgent`.

```toml
[decide]
threshold = 0.6            # an item is a yes at or over this; default 0.5

[[rule]]
if     = "VERY angry OR INDEED blocked AND billing"
then   = "escalate to a lead"
weight = 0.9               # the rule's score is its if × 0.9; default 1.0
```

- **A rule's score** is what its `if` comes to, times its `weight`. Use a weight below 1 for a rule
  you trust less than the others.
- **Several rules with the same `then`** are joined by the file's OR. With `max`, the strongest
  rule decides; with `probsum`, they add up.
- **An item is a yes** when its score is at or over `threshold`.
- **Items print in the order** the file first names them.

## `[output.NAME]`: a crisp amount

An item answers yes or no. When the answer is an **amount** ("how many hours until we reply?",
"how much to water?"), conclude in an **output** instead.

```toml
[output.reply_within]            # the name: lowercase, as a term's
range    = [0, 72]               # the axis: low, high
soon     = [0, 0, 2, 6]          # a set: a trapezoid a, b, c, d
same_day = [4, 12, 24]           # a set: a triangle a, peak, c
later    = [20, 48, 72, 72]      # a set: a shoulder that holds to the end

[[rule]]
if   = "today OR blocked"
then = "reply_within IS soon"    # OUTPUT IS SET
```

### Sets

A set is a shape on the output's axis that says how much each point belongs to it.

| Written | Shape | Belongs fully | |
| --- | --- | --- | --- |
| `[a, peak, c]` | triangle | at `peak` | rises from `a`, falls to `c` |
| `[a, b, c, d]` | trapezoid | from `b` to `c` | rises from `a` to `b`, falls from `c` to `d` |
| `[a, a, c, d]` | left shoulder | from `a` | e.g. `soon = [0, 0, 2, 6]` |
| `[a, b, d, d]` | right shoulder | up to `d` | e.g. `later = [20, 48, 72, 72]` |
| `[p, p, p]` | a single point | only at `p` | |

The points must be in order along the axis and inside `range`. Sets are drawn and listed in order
along the axis, whatever order the file has them in. **Overlap neighbouring sets**, as a Score's
levels overlap, so that an answer between two levels gives a value between their sets.

### How the value is computed

This is Mamdani inference with centroid defuzzification:

1. **Score each set.** Every rule that concludes `OUTPUT IS SET` has a score. A set's score is the
   OR of its rules' scores.
2. **Clip each concluded set** at its rule's score: the shape, cut flat at that height.
3. **Merge** the clipped shapes with the file's OR, point by point along the axis.
4. **Take the centroid**, the balance point of the merged shape. That is the output's value.

The integral is taken over a grid across the whole range, plus a fine grid across every concluded
set, so a set that is narrow next to its range still counts.

| Situation | Value |
| --- | --- |
| At least one concluded set scores above 0 | the centroid, e.g. `2.17` |
| No rule for this output scores above 0 | none: `-` in text, `null` in JSON |
| Only single-point sets score | their points, each weighted by its set's score |

`then = "X IS Y"` is an output only when the file declares `[output.X]`. Otherwise the whole text
is an item's name. A misspelt output name therefore shows up as an item, which `--graph` and the
outcome make easy to spot.

## Running it

```sh
jev STATE -q questions.json -r rules.toml [--table | --json | --graph] [--svg FILE] [--dry-run]
```

| Flag | What you get |
| --- | --- |
| *(none)* | one line per item, then the threshold, then one line per output |
| `--table` | a table with every rule behind each score |
| `--json` | `{"reply": …, "outcome": …}`: Jev's full reply and the outcome |
| `--graph` | the rules drawn in the terminal ([below](#drawing-it)) |
| `--svg FILE` | the rules drawn as an image; works alongside any of the others |
| `--dry-run` | print the request instead of sending it; the rules are still checked |

The state comes as an argument, from `-f FILE`, or on standard input. The rules may come from
standard input too (`-r -`), as long as the state and the questions don't.

### Text

```text
page on-call          0.86  yes
escalate to a lead    0.90  yes
weekly billing queue  0.00
threshold 0.60
reply_within          2.17  (soon 1.00, same_day 0.00, later 0.00)
523 tokens in, 89 out, $0.000022, typesafe/jev-1.13-20260917
```

### `--table`

```text
┌──────────────────────┬───────┬──────┬──────────────────────────────────────────────────────────────────────┐
│ item                 │ score │ yes? │ rules                                                                │
├──────────────────────┼───────┼──────┼──────────────────────────────────────────────────────────────────────┤
│ page on-call         │ 0.88  │ yes  │ blocked AND (security OR today) = 0.88                               │
│ escalate to a lead   │ 0.90  │ yes  │ VERY angry OR INDEED blocked AND billing = 0.90                      │
│ weekly billing queue │ 0.00  │      │ billing AND NOT (today OR this_week) = 0.00                          │
│ reply_within         │ 2.17  │      │ soon: today OR blocked = 1.00; same_day: this_week = 0.00; later:    │
│                      │       │      │ relaxed = 0.00                                                       │
└──────────────────────┴───────┴──────┴──────────────────────────────────────────────────────────────────────┘
threshold 0.60
```

These are separate calls, so the numbers move slightly between them (0.86 here, 0.88 there). Set
thresholds with a margin.

### `--json`

The `outcome` part (`reply` is Jev's answer, exactly as the endpoint sent it):

```json
{
  "threshold": 0.6,
  "items": [
    {
      "item": "page on-call",
      "score": 0.88,
      "yes": true,
      "rules": [{"if": "blocked AND (security OR today)", "weight": 1.0, "score": 0.88}]
    }
  ],
  "outputs": [
    {
      "output": "reply_within",
      "value": 2.1666368100000004,
      "sets": [
        {"set": "soon", "score": 1.0, "rules": [{"if": "today OR blocked", "weight": 1.0, "score": 1.0}]},
        {"set": "same_day", "score": 0.0, "rules": [{"if": "this_week", "weight": 1.0, "score": 0.0}]}
      ]
    }
  ]
}
```

| Field | |
| --- | --- |
| `threshold` | from `[decide]` |
| `items[]` | `item`, `score`, `yes`, and `rules[]`: each rule's `if`, `weight` and score |
| `outputs[]` | only when the file has outputs: `output`, `value` (a number, or `null`), and `sets[]` in axis order, each with its `score` and `rules[]` |

Scores aren't rounded in JSON, so round them yourself when you print them:
`jq '.outcome.items[] | {item, score: (.score * 100 | round / 100), yes}'`.

## Drawing it

Without a state (or with `--dry-run`), both drawings show the structure only and make no call. That
is a free way to review a rules file.

```sh
jev -q examples/rules/triage.json -r examples/rules/triage.toml --graph           # structure, no call
jev -f ticket.txt -q examples/rules/triage.json -r examples/rules/triage.toml --graph --svg triage.svg
```

### `--svg`

![The triage rules drawn as a rule base](triage.svg)

- **Premises** (left): one column per question.
  - A Score's levels are drawn as overlapping curves. The level a rule reads is in bold, shaded up to
    Jev's degree, and cut there.
  - The red arrow is the question's expected level (`score 2.00`).
  - A Choice's options and a Noul's no and yes are drawn as bars.
- **Rules**: the `if` as a tree of its operators, each with its value. A tree too tall for its box
  ends in `…`; `--graph` always shows the whole tree.
- **Conclusions**: an output's sets with the rule's own set clipped at its score, or an item's bar
  with the threshold as a dashed line.
- **Final** (right): each output's merged shape with an arrow at its centroid, and every item's
  score against the threshold.

### `--graph`

Every node shows its value, and every term shows where its degree came from, with every level's or
option's probability and the term's own in brackets:

```text
R2  VERY angry OR INDEED blocked AND billing  ⇒  escalate to a lead
    OR (a + b − ab)  1.00
    ├─ VERY (x²)  1.00
    │  └─ angry  1.00  ← anger: Calm 0.00 · Annoyed 0.00 · [Angry 1.00]
    └─ AND (min)  0.11
       ├─ INDEED (toward 0 or 1)  0.96
       │  └─ blocked  0.86  ← blocked: no 0.14 · [yes 0.86]
       └─ billing  0.11  ← team: [billing 0.11] · support 0.09 · security 0.79 · other 0.01
    ⇒ escalate to a lead  0.90 (× weight 0.9)  ██████████████████
```

An output is drawn as a plot. `░` is every set's outline, the blocks are the merged shape, and `↑`
marks the centroid:

```text
  reply_within = 2.17
  1.0 ┤██▄       ░                           ░░░░░░░░░░░░░░░░░░░░░░
      │███     ░░░░░                    ░░░░░░░░░░░░░░░░░░░░░░░░░░░
      │███▇   ░░░░░░░░             ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
      │████▃ ░░░░░░░░░░░       ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
      │█████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
  0.0 └──┬─────────────────────────────────────────────────────────
       0 ↑ 2.17                                                  72
       soon  same_day                                 later
```

## A complete example, worked by hand

[`triage.toml`](../examples/rules/triage.toml) against this ticket:

> Our card was declined twice this morning and now the whole account is locked. Nobody on the team
> can log in and payroll runs at 5pm today. This is the third time this month. Fix it NOW.

```toml
[logic]
and = "min"
or  = "probsum"            # independent reasons to act add up

[decide]
threshold = 0.6            # act only on a fairly clear yes

[terms]
billing   = "team.billing"          # a Choice option
security  = "team.security"
relaxed   = "urgency.No deadline"   # a Score level whose text has a space
this_week = "urgency.This week"
today     = "urgency.Today"
angry     = "anger.Angry"
blocked   = "blocked"               # a Noul

[[rule]]
if   = "blocked AND (security OR today)"
then = "page on-call"

[[rule]]
if     = "VERY angry OR INDEED blocked AND billing"
then   = "escalate to a lead"
weight = 0.9

[[rule]]
if   = "billing AND NOT (today OR this_week)"
then = "weekly billing queue"

[output.reply_within]
range    = [0, 72]
soon     = [0, 0, 2, 6]
same_day = [4, 12, 24]
later    = [20, 48, 72, 72]

[[rule]]
if   = "today OR blocked"
then = "reply_within IS soon"

[[rule]]
if   = "this_week"
then = "reply_within IS same_day"

[[rule]]
if   = "relaxed"
then = "reply_within IS later"
```

**What Jev said** (the degrees): `blocked` 0.86, `security` 0.79, `billing` 0.11, `today` 1.00,
`this_week` 0.00, `relaxed` 0.00, `angry` 1.00.

**R1, page on-call.** `security OR today` with probsum = 0.79 + 1.00 − 0.79 × 1.00 = 1.00. Then
`blocked AND 1.00` = min(0.86, 1.00) = **0.86**. That is ≥ 0.6, so **yes**.

**R2, escalate to a lead.**
- `VERY angry` = 1.00² = 1.00.
- `INDEED blocked` = 1 − 2 × (1 − 0.86)² = 0.96.
- `INDEED blocked AND billing` = min(0.96, 0.11) = 0.11. `AND` binds tighter than `OR`, so it is
  worked out first.
- `1.00 OR 0.11` with probsum = 1.00.
- Times the weight: 1.00 × 0.9 = **0.90**, so **yes**.

**R3, weekly billing queue.** `today OR this_week` = 1.00, and `NOT` that is 0.00.
`billing AND 0.00` = **0.00**, so **no**. That's right: this ticket is urgent, not one for the weekly
queue.

**R4–R6, reply_within.**
- `soon` is clipped at `today OR blocked` = 1.00 + 0.86 − 0.86 = **1.00**.
- `same_day` is clipped at 0.00, and so is `later`.
- The merged shape is therefore `soon` itself: 1 from 0 to 2, falling to 0 at 6.
- Its centroid: the flat part (area 2, centre 1) plus the slope (area 2, centre 2 + 4/3 ≈ 3.33)
  gives (2 × 1 + 2 × 3.33) / 4 = **2.17 hours**.

## Errors

Every error is found before the call, except the last two in the table, which come from Jev's
reply. Each message names the rule by number and quotes its `if`.

| Mistake | Message |
| --- | --- |
| a level that doesn't exist | ``[terms] hot: `temp` has no level `hot`; its levels are `temp.Cold`, `temp.Mild`, `temp.Hot` `` |
| an option that doesn't exist | ``[terms] s: `team` has no option `Billing`; its options are `team.billing`, … `` |
| a Score or Choice without a level | ``[terms] t: `temp` is a score: name one of its levels, as `temp.Cold`, … `` |
| a Noul with a level | ``[terms] r: `raining` is a noul, which has no levels or options: write `raining` alone `` |
| a question that isn't asked | ``[terms] w: no question `wind`; the questions are humidity, raining, temp `` |
| a capital in a term's name | ``[terms] `Hot`: a term is a lowercase word of letters, digits and `_` … `` |
| a term not in `[terms]` | ``rule 1 (`wet`): `wet` isn't in [terms], which names hot, raining `` |
| a lowercase operator | ``rule 1 (`raining and hot`): `and` isn't in [terms]; the operator is written AND `` |
| an unknown operator | ``rule 1 (`raining XOR hot`): `XOR` isn't an operator: operators are AND, OR, NOT, VERY, SOMEWHAT, EXTREMELY, INDEED `` |
| a missing operator | ``rule 1 (`raining hot`): `hot` where AND or OR should be `` |
| a missing term | ``rule 1 (`raining AND`): it ends where a term should be `` |
| brackets | `` a `(` that isn't closed ``, `` a `)` with no `(` before it `` |
| an `if` that is too long | ``it is 399 words and brackets long, over the 256 a rule may have; split it …`` |
| an empty `if` or `then` | `` the `if` is empty ``, `` the `then` is empty `` |
| a weight or threshold outside 0–1 | `the weight is 2; it goes from 0 to 1`, `[decide] threshold is 1.5; it goes from 0 to 1` |
| no rules | ``no rules: add a [[rule]] with an `if` and a `then` `` |
| an unknown key or `[logic]` value | TOML's own message, naming the key and the values it accepts |
| an output's set that doesn't exist | ``[output.water] has no set `lots`; its sets are some `` |
| an output's range | `[output.water] range is [10.0, 0.0]; it is two numbers, lowest first: range = [0, 100]` |
| an output's set | `liter has 2 points; a set is [a, b, c] …`, `goes outside the range`, `points go down somewhere` |
| *from the reply:* an answer is missing | ``no answer for question `raining` `` |
| *from the reply:* a probability is missing | ``question `team` gives no probability for `storm` `` |

## From Rust

The engine is `jev::rules`, behind the `command` feature (the CLI's default).

```toml
[dependencies]
jev = { package = "fuzzy-jev", version = "0.2", default-features = false, features = ["command"] }
```

```rust
use jev::rules::Rules;

let questions = jev::spec::questions_file(&std::fs::read_to_string("triage.json")?)?;
let rules = Rules::parse(&std::fs::read_to_string("triage.toml")?, questions.iter().map(|(id, q)| (id.as_str(), q)))?;

let reply = jev::Client::new(&key).decide(ticket, questions).await?;
let outcome = rules.evaluate(&reply)?;

for item in &outcome.items {
    if item.yes {
        println!("{}: {:.2}", item.item, item.score);
    }
}
if let Some(hours) = outcome.outputs[0].value {
    println!("reply within {hours:.1} hours");
}
std::fs::write("triage.svg", rules.graph_svg(Some(&reply))?)?;
```

| Function | Returns |
| --- | --- |
| `Rules::parse(text, questions)` | the checked rules, or the error message as a `String` |
| `rules.evaluate(&reply)` | an `Outcome`, or `jev::Error` when the reply lacks an answer or probability |
| `rules.graph_text(Some(&reply))` / `graph_text(None)` | the `--graph` drawing, with numbers or the structure only |
| `rules.graph_svg(Some(&reply))` / `graph_svg(None)` | the `--svg` drawing |
| `outcome.text()` | the text the command prints |
| `jev::print::render_outcome(format, &reply, &outcome, width)` | the command's text, table or JSON |

`Outcome` has `threshold`, `items: Vec<Item>` (`item`, `score`, `yes`, `rules`) and
`outputs: Vec<OutputValue>` (`output`, `value: Option<f64>`, `sets`). Each rule entry is a `Fired`
(`when`, `weight`, `score`), serialized with the key `if`.

## Designing rules

1. **Ask graded things as Scores** with named levels (`Cold | Mild | Hot`), 3–5 each. Keep Nouls
   for things that are simply true or false.
2. **Write one rule per piece of know-how**, in the owner's words: "a raincoat when it rains, unless
   it's hot" becomes `raining AND NOT hot`.
3. **Check coverage.** Walk the combinations (cold / mild / hot × raining or not) and make sure some
   rule speaks for each. A gap is a situation with no outcome.
4. **Look for conflicts** between two rules that say opposite things about one situation. Decide
   which one wins, or add an outcome for the case where both hold.
5. **Review the structure for free** with `--graph` and no state, then **tune on real states**: run
   ten or twenty, and fix a rule, a weight or the threshold, never the answers.
6. **Leave `[logic]` at `min` and `max`** unless you know the conditions are independent.
