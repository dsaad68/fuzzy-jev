# The five patterns

How Jev calls get composed into something useful. From TypeSafe's own docs
([fan-out](https://docs.typesafe.ai/patterns/fan-out),
[confidence routing](https://docs.typesafe.ai/patterns/confidence-routing),
[intent routing](https://docs.typesafe.ai/patterns/intent-routing),
[composite scoring](https://docs.typesafe.ai/patterns/composite-scoring)), worked through with the
`jev` command, and fuzzy rules over the answers with `-r`.

They compose: intent routing is usually fan-out plus confidence routing, and a ranking step is
composite scoring over one fan-out call.

## Fan-out

**Ask everything you might need in one call, including questions whose relevance depends on the
answer to another. Let your code decide what mattered.**

Questions are answered in parallel, so a call with six questions costs barely more time than one
with two — only tokens. Branching *after* the call beats a round trip per branch.

```json
{
  "category":     {"type": "choice", "instructions": "What kind of ticket is this?",
                   "criteria": {"bug": "something is broken", "billing": "a charge or invoice",
                                "feature": "asking for something that doesn't exist", "account": "access or settings"}},
  "severity":     {"type": "score",  "instructions": "If this is a bug, how bad is it?",
                   "criteria": ["cosmetic", "annoying", "blocking work"]},
  "has_repro":    {"type": "noul",   "instructions": "Did they give steps to reproduce it?"},
  "wants_refund": {"type": "noul",   "instructions": "Are they asking for money back?"},
  "frustration":  {"type": "score",  "instructions": "How frustrated does the writer sound?",
                   "criteria": ["calm", "irritated", "very angry"]}
}
```

```sh
jev -f ticket.txt -q triage.json --json > reply.json

case "$(jq -r '.answers.category.choice' reply.json)" in
  bug)     jq -r '"severity \(.answers.severity.score), repro \(.answers.has_repro.noul)"' reply.json ;;
  billing) jq -r '"refund wanted: \(.answers.wants_refund.noul > 0.5)"' reply.json ;;
  feature) echo "backlog" ;;                       # severity and repro were asked and ignored
esac
jq -r '"frustration \(.answers.frustration.score)"' reply.json   # applies whatever the category
```

**Avoid it** when the branches are genuinely sequential (the second question can't be written until
the first is answered), when a truly single question will do, or when the wasted tokens actually
matter to you.

## Confidence routing

**The answer tells you what; confidence tells you whether to act on it.** Pick a threshold per
action, scaled to what that action costs if it's wrong.

TypeSafe's rough bands: act autonomously above ~0.85 for anything consequential, ask the user to
confirm between ~0.6 and ~0.85, and escalate to a person below ~0.6. These are starting points —
what a low confidence *means* depends on your stakes, so tune them against real traffic.

```sh
jev -f message.txt -q intent.json --json > reply.json
intent=$(jq -r '.answers.intent.choice' reply.json)
sure=$(jq -r '.answers.intent.confidence' reply.json)

if   awk "BEGIN{exit !($sure < 0.6)}";  then handoff_to_a_person
elif [ "$intent" = check_balance ];     then show_balance            # low stakes, 0.6 is plenty
elif [ "$intent" = approve_transfer ]; then
       awk "BEGIN{exit !($sure > 0.85)}" && approve || confirm_with_user
fi
```

A Noul carries no separate confidence — its probability *is* the confidence, so gate on the
probability itself and treat the middle band (say 0.3–0.7) as "unsure".

**Avoid it** when every decision warrants the same threshold, or when there's no fallback to fall
back to: a gate with nowhere to escalate just stalls.

## Intent routing

**Classify cheaply first, then hand off.** One small call decides which handler gets the request —
deterministic code, a specialist model, or a person — instead of sending everything through the
expensive path.

A Choice for the category, plus a Score for how hard it looks, covers most of it:

```json
{
  "intent":     {"type": "choice", "instructions": "What does the customer want? Route by what they're asking you to do.",
                 "criteria": {"order_status": "where is my order", "product_question": "how does it work",
                              "return_exchange": "sending it back", "complaint": "unhappy, wants it made right",
                              "other": "none of these"}},
  "complexity": {"type": "score",  "instructions": "How much work is answering this?",
                 "criteria": ["a lookup", "needs judgment", "needs an exception or a manager"]}
}
```

Route on the intent, then let complexity override: a `product_question` scoring at the top level
goes to a person even though its category has a cheap handler. Confidence first, as above — a
misrouted request costs more than the model call you saved.

**Avoid it** when every request ends up at the same handler anyway, when the routing is a single
branch, or when a misroute is unrecoverable and accuracy has to be near-perfect.

## Composite scoring

**Score independent dimensions separately, then combine them yourself.** One "how good is this
overall?" question hides its reasoning and can't be retuned. Several narrow ones stay legible, and
the weights become a dial you can turn without touching the criteria.

Ask them in one call (this is fan-out again):

```json
{
  "depth":      {"type": "score", "instructions": "How deep is the candidate's Python experience?",
                 "criteria": ["none", "scripts", "production services", "libraries others depend on", "language internals"]},
  "leadership": {"type": "score", "instructions": "How much have they led other engineers?",
                 "criteria": ["none", "mentored one", "led a project", "led a team", "led an org"]}
}
```

Normalize each to 0–1 by dividing by its top level, then weight and sum:

```sh
jev -f cv.txt -q dimensions.json --json \
  | jq -r '(.answers.depth.score / 4) as $d | (.answers.leadership.score / 4) as $l
           | "ic \(0.8 * $d + 0.2 * $l | .*100 | round / 100)  lead \(0.3 * $d + 0.7 * $l | .*100 | round / 100)"'
```

Two roles, two weightings, one call. The top level is `(levels - 1)`, since levels are numbered from
0 — with five levels, divide by 4.

**Avoid it** when the dimensions aren't actually independent (scoring them apart then adding them
double-counts), or when the judgment is genuinely holistic and the weights would be invented.

## Fuzzy rules

**Let Jev read the situation and let rules, written by whoever owns the decision, say what to do.**
Every answer is already a fuzzy degree: a Noul's probability, each Score level's probability, each
Choice option's. A rules file combines them, and each outcome gets a score.

Ask the inputs as Scores with named levels, 3–5 each: `Cold|Mild|Hot`, not a number. Graded ideas
belong in a Score; keep Nouls for things that are true or false.

```json
{"temp":     {"type": "score", "instructions": "How warm does it feel outside?", "criteria": ["Cold", "Mild", "Hot"]},
 "humidity": {"type": "score", "instructions": "How humid is the air?", "criteria": ["Dry", "Normal", "Humid"]},
 "raining":  {"type": "noul",  "instructions": "Is it raining, or about to?"}}
```

```toml
[logic]                  # optional
and = "min"              # min (default: safe when answers are related) | product | lukasiewicz
or  = "max"              # max (default: the strongest reason decides) | probsum | bounded

[decide]
threshold = 0.5          # an item is a yes at or over this

[terms]
cold = "temp.Cold"
mild = "temp.Mild"
hot = "temp.Hot"
humid = "humidity.Humid"
raining = "raining"

[[rule]]
if = "cold"
then = "coat"

[[rule]]
if = "mild AND NOT raining"
then = "light jacket"

[[rule]]
if = "raining AND NOT hot"
then = "raincoat"

[[rule]]
if = "hot"
then = "t-shirt"
```

```sh
jev -f weather.txt -q weather.json -r wear.toml --table
```

Designing them:

1. **One rule per piece of know-how**, in the owner's words: "a raincoat when it rains, unless it's
   hot" is `raining AND NOT hot`.
2. **Check coverage.** Walk every combination of levels (cold/mild/hot × raining or not) and make sure
   some rule speaks for each; a gap means no outcome in that weather.
3. **Look for conflicts** — two rules saying opposite things for one situation — and decide which
   wins, or make a third outcome.
4. **Tune with real states.** Run ten or twenty through `-r` and fix a rule, a weight or the
   threshold, never the answers. `--table` shows which rule gave each score, and `--graph` (or
   `--svg rules.svg`) draws every rule with its numbers.

Operators: `AND`, `OR`, `NOT`, parentheses, and hedges `VERY` (a²), `SOMEWHAT` (√a), `EXTREMELY`
(a³), `INDEED` (pushed toward 0 or 1). Hedges and `NOT` bind tightest, then `AND`, then `OR`. Use
`probsum` for OR when independent reasons should reinforce each other, `product` for AND when
conditions really are independent.

### Outputs: an amount, not a yes

When the decision is an amount (how much to water, how many minutes to wait), declare an output: a
crisp axis with named fuzzy sets. Rules conclude `OUTPUT IS SET`. Each such rule clips its set at its
score, the clipped sets are merged with OR, and the value is the centre of the merged shape
(Mamdani inference, centroid defuzzification).

```toml
[output.irrigation]
range  = [0, 100]
drops  = [0, 0, 20, 40]       # trapezoid: a, b, c, d
liter  = [30, 50, 70]         # triangle: a, peak, c
gallon = [60, 80, 100, 100]

[[rule]]
if = "scarce"
then = "irrigation IS gallon"

[[rule]]
if = "regular"
then = "irrigation IS liter"
```

Overlap neighbouring sets, as the levels of a Score overlap, so that an answer between two levels
gives a value between their sets. Gaps between the sets make the value jump.

**Avoid it** when one answer decides alone (read it directly), or when you'd be inventing rules
nobody holds: then a weighted sum is at least honest about being a guess.
