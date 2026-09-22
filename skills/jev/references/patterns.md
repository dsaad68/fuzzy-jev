# The four patterns

How Jev calls get composed into something useful. From TypeSafe's own docs
([fan-out](https://docs.typesafe.ai/patterns/fan-out),
[confidence routing](https://docs.typesafe.ai/patterns/confidence-routing),
[intent routing](https://docs.typesafe.ai/patterns/intent-routing),
[composite scoring](https://docs.typesafe.ai/patterns/composite-scoring)), worked through with the
`jev` command.

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
