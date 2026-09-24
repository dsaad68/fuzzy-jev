# fuzzy-jev for Python

Python bindings for [fuzzy-jev](https://github.com/dsaad68/fuzzy-jev): ask TypeSafe's **Jev**
typed questions about a piece of text through OpenRouter's decisions endpoint, and get
probabilities back — a **Choice** between named options, a **Score** on an ordered scale, or a
**Noul**, the probability that something is true. [Rules](https://github.com/dsaad68/fuzzy-jev/blob/main/docs/rules.md) then combine those
answers by fuzzy logic into decisions and amounts, and draw themselves as SVG.

The package is `fuzzy-jev`; it is imported as `jev`. It is the Rust library underneath, so a
question, a reply and a rules file mean exactly what they mean to the `jev` command.

## Install

```sh
pip install fuzzy-jev
```

Wheels are built for Linux and macOS (x86_64 and arm64), for CPython 3.10 and later. Anywhere else pip
builds from source, which needs a [Rust toolchain](https://rustup.rs). From a checkout of this
repository:

```sh
pip install ./python          # or: uv pip install ./python
```

For development, `maturin develop` builds it into the active virtual environment.

## Ask

```python
import jev

client = jev.Client()  # the key from OPENROUTER_API_KEY; or jev.Client("sk-or-...")
reply = client.decide(
    "Help! My payouts have been failing for 3 days.",
    {
        "is_urgent": jev.Question.noul(
            "Does this message convey urgency?", yes="Explicitly time-sensitive", no="No urgency expressed"
        ),
        "department": jev.Question.choice(
            "Which team should handle this?",
            {"billing": "Payments, invoicing, refunds", "technical": "Bugs, outages, integrations", "sales": "Pricing"},
        ),
        "frustration": jev.Question.score("How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"]),
    },
)

reply.noul("is_urgent")                  # 0.95
department = reply.choice("department")  # ChoiceAnswer: .choice, .confidence, .probabilities
reply.score("frustration").score         # 1.04, between levels 1 and 2
print(reply.render("table"))             # as `jev --table` prints it
reply.usage.cost                         # in US dollars
```

The state can be a string, a dict or a list; instructions and criteria can be any JSON value too.
Questions are a dict of id to `Question`, or `(id, question)` pairs; a question can also be a dict
in the endpoint's shape (`{"type": "noul", "instructions": ...}`), and `jev.load_questions(text)`
reads a `jev -q` questions file.

`decide` releases the GIL while it waits, so threads ask in parallel; `await
client.decide_async(...)` is the same for asyncio. `client.request(state, questions)` is the
request body `decide` would send, without sending it.

Errors are all `jev.JevError`: `StatusError` (with `.status`), `HttpError`, `DecodeError`,
`InvalidQuestionError` (raised before any call), `MissingAnswerError`, `WrongTypeError`,
`BadAnswerError` and `RulesError`.

## Rules

```python
questions = jev.load_questions(open("examples/rules/triage.json").read())
rules = jev.Rules(open("examples/rules/triage.toml").read(), questions)  # checked now, before a call

reply = client.decide(ticket, questions)
outcome = rules.evaluate(reply)
outcome.yes                              # ["page on-call", ...]: the items at or over the threshold
outcome["reply_within"].value            # an output's crisp value, in hours here
print(outcome.text())

open("triage.svg", "w").write(rules.graph_svg(reply))
```

See [`docs/rules.md`](https://github.com/dsaad68/fuzzy-jev/blob/main/docs/rules.md) for the whole of the rules file.

## Tests

```sh
maturin develop && pytest
```
