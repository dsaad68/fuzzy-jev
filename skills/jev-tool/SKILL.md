---
name: jev
description: >-
  Ask Jev, a small model that writes no text, typed questions about a piece of text and get
  calibrated probabilities back instead of prose — a Choice between named options, a Score on an
  ordered scale, or a Noul, the probability that something is true. Use when classifying, routing,
  triaging, labelling, rating, scoring or flagging text, especially many items against one set of
  criteria; when a judgment needs a confidence number to act on rather than an opinion; or when
  building a filter, moderation or triage step. Not for writing, summarizing or rewriting text —
  Jev only answers questions you define.
license: MIT
metadata:
  source: https://github.com/dsaad68/fuzzy-jev (skills/jev), rewritten for the jev tool rather than the jev command
---

# Jev: typed questions, probabilities back

Jev doesn't write text. It reads a **state** and answers **questions** about it with probability
distributions you can act on. Every question sees the same state and is answered independently, so
ask all of them in one call — an extra question costs a few tokens and almost no time.

| Type | Asks | Answer |
| --- | --- | --- |
| `noul` | Is this true? | `noul`: the probability of yes. **No separate confidence** — that is the answer. |
| `choice` | Which one of these? | the option chosen, its `confidence`, and every option's probability. 2–255 options. |
| `score` | Which level? | the **expected** level, so it can fall between two; plus confidence and each level's probability. 2–10 levels, lowest first. |

One call is `{"state": what to judge, "questions": {"<your id>": {…}}}`. A question is
`{"type": …, "instructions": …, "criteria": …}`: every question needs a type and instructions, and
`criteria` is an object of option to description for a choice, an array of levels lowest first for a
score, and optional for a noul (`{"true": …, "false": …}`). The ids are yours and Jev never sees
them, so each question's instructions have to say everything.

## Write the questions once, then ask them of everything

The questions are the work; the calls are cheap. When a task puts many items through the same
judgment, don't rewrite the criteria for each one — write them down once, in `questions.json`, and
send that same object with every item:

1. **Write `questions.json` first.** One question per judgment the task asks for, with the criteria
   the task gives you, close to word for word. It holds exactly the object a call passes under
   `questions`.
2. **Try it on one item.** Ask about a single item and read the answer: a confident answer on an
   item you'd have called the same way means the criteria are working, and a confidence near the
   middle on an easy item means they aren't. Fix the file before spending twenty calls on it.
3. **Fan out.** One call per item: that item's text as the state, the questions from the file in
   each call. Put four or five calls in a turn so they run in parallel, rather than a turn per item.
4. **Write the results out.** Take the most probable option for each question, and look at the item
   yourself where the confidence is low.

Keeping the questions in a file is what makes them worth improving: every item is judged against the
same words, a fix to the criteria is one edit rather than twenty, and afterwards the file says what
was actually asked.

```json
{"team":    {"type": "choice", "instructions": "Which team should handle this? Route by what the sender is asking you to do, not by what happened to them.",
             "criteria": {"billing": "money that has already moved: invoices, charges, refunds",
                          "support": "the product not working or not understood",
                          "security": "who can get into the account and how they are kept out"}},
 "blocked": {"type": "noul",   "instructions": "Is the sender unable to work right now?"},
 "urgency": {"type": "score",  "instructions": "How soon does this need an answer?",
             "criteria": ["No deadline given", "Wants it this week", "Blocked until it is answered"]}}
```

## Write criteria that decide something

Criteria exist for the items that are nearly one thing and nearly another; the obvious ones answer
themselves. An option's description says what puts an item on its side of the line, and a line no
description draws is a line Jev guesses at:

```jsonc
"criteria": {"billing": "billing", "support": "support"}                       // decides nothing
"criteria": {"billing": "money that has already moved or is owed — invoices, charges, refunds",
             "support": "the product not working or not understood"}           // decides
```

Put a rule that settles the overlaps in the question's `instructions` ("route by what the sender is
asking you to do, not by what happened to them"), where it applies to every option, and add an
`other` option when the list may not cover every item.

## Read the answer

One line per question, in the order you asked them:

```text
team     billing  confidence 0.79  (billing 0.86, security 0.12, support 0.02)
blocked  0.90 yes
urgency  1.98 of 2  confidence 0.97  (No deadline given 0.00, Wants it this week 0.02, Blocked 0.98)
420 tokens in, 69 out, $0.000018, typesafe/jev-1.13-20260917
```

The answer says **what**; the confidence says **whether to act on it**. Take the answer where it is
confident, and read the item yourself where it isn't — that is the whole point of getting a number
back rather than a sentence.

## Decide with rules

When the decision is several answers combined ("raining and not hot → raincoat"), pass `rules`: TOML
text naming answers as terms and combining them with fuzzy logic. Every answer is already a degree
from 0 to 1.

```toml
[terms]                       # lowercase names for answers
hot     = "temp.Hot"          # a Score level, by its exact text
raining = "raining"           # a Noul, by its id
stormy  = "sky.storm"         # a Choice option, from a "sky" choice: clear, cloudy, storm

[[rule]]
if   = "raining AND NOT hot"  # AND OR NOT ( ), hedges VERY SOMEWHAT EXTREMELY INDEED — uppercase
then = "raincoat"
```

For an amount rather than a yes/no, declare `[output.NAME]` with a `range = [low, high]` and sets
(`liter = [30, 50, 70]` is a triangle, four numbers a trapezoid), and conclude `then = "NAME IS
liter"`. The value is the centroid of the sets, each clipped at its rule's score.

The reply adds each outcome's score after the answers, `yes` at or over `[decide] threshold` (0.5).
AND is `min`, OR is `max`, NOT is `1 − x`. The rules are checked against the questions before the
call, so a term naming a level that doesn't exist is handed back to you for free.

## Patterns

Read `references/patterns.md` when you are building a routing, triage or ranking step rather than
asking one question: fan-out, confidence routing, intent routing, composite scoring and fuzzy rules,
each worked through.

## Gotchas

- **Jev sees only the state** — not your files, not this conversation, not the question ids. Quote
  into the state everything it needs, including the item's own text.
- **A Noul has no confidence field.** Its probability is the answer: 0.9 is a confident yes, 0.5 is
  no answer at all.
- **A Score is an expectation, not an index.** `1.98 of 2` is nearly the top level; `1.2 of 2` is
  genuinely between two. Round only when you need a discrete label.
- **Ask every question about one state in one call.** Two calls for two questions about the same
  item costs twice and answers no better.
- **Nothing validates your criteria.** Jev answers a badly posed question with a confident-looking
  distribution. Garbage criteria, garbage answer.
- **Deciding one thing, once, with the text already in front of you? Just decide it.** Jev earns its
  keep on many items against one set of criteria, not on a single judgment you are already making.
