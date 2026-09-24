# The five patterns

How Jev calls get composed into something useful. From TypeSafe's own docs
([fan-out](https://docs.typesafe.ai/patterns/fan-out),
[confidence routing](https://docs.typesafe.ai/patterns/confidence-routing),
[intent routing](https://docs.typesafe.ai/patterns/intent-routing),
[composite scoring](https://docs.typesafe.ai/patterns/composite-scoring)), worked through with the
`jev` tool and a `questions.json` you keep beside the work, and fuzzy rules over the answers.

They compose: a labelling run is fan-out over many states plus confidence routing on what comes
back, and a ranking step is composite scoring over one call.

## Fan-out

**Ask everything you might need in one call, including questions whose relevance depends on the
answer to another. Decide what mattered afterwards.**

Questions are answered in parallel, so a call with six questions costs barely more time than one
with two — only tokens. Deciding *after* the call beats a second call per branch.

```json
{"category":     {"type": "choice", "instructions": "What kind of ticket is this?",
                  "criteria": {"bug": "something is broken", "billing": "a charge or an invoice",
                               "feature": "asking for something that doesn't exist yet", "account": "access or settings"}},
 "severity":     {"type": "score",  "instructions": "If this is a bug, how bad is it?",
                  "criteria": ["cosmetic", "annoying", "blocking work"]},
 "has_repro":    {"type": "noul",   "instructions": "Did they give steps to reproduce it?"},
 "wants_refund": {"type": "noul",   "instructions": "Are they asking for money back?"}}
```

The severity and the repro flag are wasted on a billing ticket, and they cost a few tokens; a second
round trip to ask them would cost a turn.

**Avoid it** when the branches are genuinely sequential — when the second question can't be written
until the first is answered — or when a single question really will do.

## Many items, one set of criteria

**The labelling run: `questions.json` once, then a call per item.** This is what Jev is for, and
what the criteria are worth paying attention to.

1. Write `questions.json`: the questions the task asks for, with the task's own words as criteria.
2. Ask about one item. A middling confidence on an easy item means the criteria are unclear; a
   confident answer that matches what you'd have said only means they aren't obviously broken. It
   is a smoke check, not an evaluation.
3. Then every item, four or five calls to the turn so they run in parallel. Each call carries that
   item's text as the state and the same questions.
4. Collect the answers into the file the task asks for, taking the most probable option.

Two things to hold to: the state is the item's own text, quoted in full — Jev opens no files — and
the questions are the file's, unchanged between items. Criteria that drift from item to item make
the labels incomparable, which is the one thing a labelling run is supposed to give you.

## Confidence routing

**The answer tells you what; the confidence tells you whether to act on it.** Pick a threshold per
action, scaled to what that action costs if it's wrong.

TypeSafe's rough bands: act on anything consequential above ~0.85, confirm between ~0.6 and ~0.85,
and hand to a person below ~0.6. They are starting points — what a low confidence *means* depends on
your stakes.

Working on your own, the escalation is you: read the items whose answers came back unsure, decide
those few yourself, and leave the confident ones alone. That is cheaper than reading all of them and
better than taking a 0.4 answer at face value. A Noul carries no separate confidence — its
probability *is* the confidence, so gate on it and treat the middle band (say 0.3–0.7) as unsure.

**Avoid it** when every decision warrants the same threshold, or when there is nothing to escalate
to: a gate with no fallback just stalls.

## Intent routing

**Classify cheaply first, then hand off.** One small call decides which handler gets the request —
deterministic code, a specialist, or a person — instead of putting everything through the expensive
path.

A Choice for the category plus a Score for how hard it looks covers most of it:

```json
{"intent":     {"type": "choice", "instructions": "What does the customer want? Route by what they are asking you to do.",
                "criteria": {"order_status": "where is my order", "product_question": "how does it work",
                             "return_exchange": "sending it back", "complaint": "unhappy, wants it made right",
                             "other": "none of these"}},
 "complexity": {"type": "score",  "instructions": "How much work is answering this?",
                "criteria": ["a lookup", "needs judgment", "needs an exception or a manager"]}}
```

Route on the intent, then let the complexity override it: a `product_question` at the top level goes
to a person even though its category has a cheap handler. Add an `other` option whenever the list
may not cover everything — without one, Jev has to put the leftovers somewhere.

**Avoid it** when every request ends at the same handler anyway, or when a misroute is
unrecoverable.

## Composite scoring

**Score independent dimensions separately, then combine them yourself.** One "how good is this
overall?" question hides its reasoning and can't be retuned; several narrow ones stay legible, and
the weights become a dial you can turn without touching the criteria.

```json
{"depth":      {"type": "score", "instructions": "How deep is the candidate's Python experience?",
                "criteria": ["none", "scripts", "production services", "libraries others depend on", "language internals"]},
 "leadership": {"type": "score", "instructions": "How much have they led other engineers?",
                "criteria": ["none", "mentored one", "led a project", "led a team", "led an org"]}}
```

Normalize each score by its top level — levels are numbered from 0, so five levels divide by 4 —
then weight and sum: `0.8 × depth + 0.2 × leadership` for one role, `0.3 × depth + 0.7 × leadership`
for another. Two roles, two weightings, one call.

**Avoid it** when two dimensions measure the same thing (adding both counts it twice), or when the
judgment is genuinely holistic and the weights would be invented.

## Fuzzy rules

**Let Jev read the situation and let rules, written by whoever owns the decision, say what to do.**
A rules file reads Jev's probabilities (a Noul's, a Score level's, a Choice option's) as the
degrees to which its terms hold, combines them, and gives each outcome a **support score**. That
reading is a modelling choice: a score says how strongly the policy supports an outcome, not how
likely it is, so don't pass it on as a probability. The answers keep the probabilities.

Pass the rules, as TOML text, in the call's `rules` argument; the reply adds an `outcome:` line per
item after the answers. Ask the inputs as Scores with named levels, 3–5 each: `Cold|Mild|Hot`, not a number. Graded ideas
belong in a Score; keep Nouls for things that are true or false.

```json
{"temp":     {"type": "score", "instructions": "How warm is it outside? Cold is under 10 °C, Mild 10 to 22 °C, Hot over 22 °C.", "criteria": ["Cold", "Mild", "Hot"]},
 "humidity": {"type": "score", "instructions": "How humid is the air? Dry is under 40% relative humidity, Normal 40 to 70%, Humid over 70%.", "criteria": ["Dry", "Normal", "Humid"]},
 "raining":  {"type": "noul",  "instructions": "Is it raining, or about to?"}}
```

```toml
[logic]                  # optional
and = "min"              # min (default: the weakest condition decides) | product | lukasiewicz
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

Designing them:

1. **One rule per piece of know-how**, in the owner's words: "a raincoat when it rains, unless it's
   hot" is `raining AND NOT hot`.
2. **Check coverage.** Walk every combination of levels (cold/mild/hot × raining or not) and make sure
   some rule speaks for each; a gap means no outcome in that weather.
3. **Look for conflicts** — two rules saying opposite things for one situation — and decide which
   wins, or make a third outcome.
4. **Tune with real states.** Run ten or twenty to debug the rules — that is exploration, not
   evidence that they work — and fix a rule, a weight or the threshold, never the answers. Before
   relying on them, check them against reviewed cases they weren't tuned on. Keep the rules in `rules.toml` beside `questions.json`, and send the same text each
   call.

Operators: `AND`, `OR`, `NOT`, parentheses, and hedges `VERY` (a²), `SOMEWHAT` (√a), `EXTREMELY`
(a³), `INDEED` (pushed toward 0 or 1). Hedges and `NOT` bind tightest, then `AND`, then `OR`. Use
`probsum` for OR when distinct reasons should reinforce each other (it counts a reason written twice
twice), and `bounded` to add up levels or options of one question, which exclude each other: the
chance of "Today or This week" is their sum. Hedges only reshape a score: `VERY angry` passes 0.5
when `P(Angry)` is 0.71 or more, and it can't tell angry from furious — ask a level for that. None of
the operators gives the probability of a compound event.

Nothing is exclusive unless a rule makes it so: at `raining` 0.5, both `raining` and `NOT raining`
score 0.5 and pass a 0.5 threshold. When two outcomes can't both happen, write what picks between
them, and send a case near the threshold to a person.

### Outputs: an amount, not a yes

When the decision is an amount (how long to water, how many minutes to wait), declare an output: a
crisp axis with named fuzzy sets. Rules conclude `OUTPUT IS SET`, and `X IS Y` always names an
output, so a typo is an error. A set's rules are joined by OR into its score, the set is clipped at
that score, the clipped sets are merged with OR, and the value is the centre of the merged shape
(Mamdani inference, centroid defuzzification).

```toml
[output.irrigation]            # minutes of watering: say the unit, the file has none
range  = [0, 100]
short  = [0, 0, 20, 40]       # trapezoid: a, b, c, d
medium = [30, 50, 70]         # triangle: a, peak, c
long   = [60, 80, 100, 100]

[[rule]]
if = "scarce"
then = "irrigation IS long"

[[rule]]
if = "regular"
then = "irrigation IS medium"
```

Whether neighbouring sets overlap is your policy's choice: overlapping ones let support for two
sets give a value between them. (A Score's levels don't overlap: they exclude each other, even when
several have some probability.) The value says where the support is, not how much: a set clipped
at 0.01 alone gives nearly the same value as one at 1 (exactly the same for a symmetric set), so
check the sets' scores before acting on it. And two supported sets far apart give a value between
them that neither supports; if that compromise is wrong, make the rules choose. Keep hard limits in
code.

**Avoid it** when one answer decides alone (read it directly), or when you'd be inventing rules
nobody holds: then a weighted sum is at least honest about being a guess.
