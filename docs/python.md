# Python: `import jev`

The Python package is the Rust library underneath, so a question, a reply and a rules file mean in
Python exactly what they mean to the `jev` command. This page covers installing it, asking,
reading the reply, rules and their drawings, errors, and the whole API.

- [Install](#install)
- [A first call](#a-first-call)
- [The client](#the-client)
- [Questions](#questions)
- [The state](#the-state)
- [The reply](#the-reply)
- [Rules](#rules)
- [Drawing the rules](#drawing-the-rules)
- [Errors](#errors)
- [Threads and asyncio](#threads-and-asyncio)
- [Without the network](#without-the-network)
- [API reference](#api-reference)
- [Building and releasing](#building-and-releasing)

## Install

```sh
pip install fuzzy-jev        # or: uv add fuzzy-jev
```

The package on PyPI is `fuzzy-jev`, since `jev` is taken; it is imported as `jev`. Wheels are built
for Linux (glibc 2.28 and up) and macOS, on x86_64 and arm64, and one wheel works for every CPython
from 3.10 on. Anywhere else pip builds from the source distribution, which needs a
[Rust toolchain](https://rustup.rs).

From a checkout:

```sh
pip install ./python
```

Set an [OpenRouter key](https://openrouter.ai/keys): Jev is TypeSafe's model, served by OpenRouter's
decisions endpoint.

```sh
export OPENROUTER_API_KEY=sk-or-...
```

## A first call

```python
import jev

client = jev.Client()  # the key from OPENROUTER_API_KEY
reply = client.decide(
    "Help! My payouts have been failing for 3 days.",
    {
        "is_urgent": jev.Question.noul(
            "Does this message convey urgency?", yes="Explicitly time-sensitive", no="No urgency expressed"
        ),
        "department": jev.Question.choice(
            "Which team should handle this?",
            {
                "billing": "Payments, invoicing, refunds",
                "technical": "Bugs, outages, integrations",
                "sales": "Pricing, upgrades, new accounts",
            },
        ),
        "frustration": jev.Question.score("How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"]),
    },
)

if reply.noul("is_urgent") > 0.8 and reply.choice("department").confidence > 0.6:
    print("page", reply.choice("department").choice)
print(reply.render())
```

```text
department   billing  confidence 0.82  (billing 0.88, technical 0.12, sales 0.00)
frustration  1.04 of 2, nearest "Frustrated"  confidence 0.94
is_urgent    0.95 yes
427 tokens in, 73 out, $0.000018, typesafe/jev-1.13-20260917
```

Every question sees the same state and is answered on its own, so ask them all in one call: an extra
question costs a few tokens and no extra round trip.

## The client

```python
jev.Client(key=None, *, model=None, url=None, timeout=None)
```

| Argument | |
| --- | --- |
| `key` | An OpenRouter API key. Without one it is read from `OPENROUTER_API_KEY`. An empty key sends no `Authorization` header, for an endpoint (`url`) that adds the key itself. |
| `model` | Another model; `jev.DEFAULT_MODEL` (`typesafe/jev-1.13`) otherwise. |
| `url` | Another endpoint; `jev.DECISIONS_URL` otherwise. |
| `timeout` | Seconds for the whole request, from sending it to reading the reply; `jev.DEFAULT_TIMEOUT` (60) otherwise. A request that timed out is not sent again: it may have been answered, and billed, all the same. |

| Method | |
| --- | --- |
| `decide(state, questions)` | Asks, and waits for the `DecisionResponse`. |
| `decide_async(state, questions)` | The same, awaitable. |
| `request(state, questions)` | The request body `decide` would send, as a dict, without sending it. Costs no call and needs no key. |

A client's `repr` never shows the key.

## Questions

Three types, as the endpoint has them:

| Type | Asks | Answer |
| --- | --- | --- |
| `Question.noul(instructions, yes=None, no=None)` | Is this true? | The probability of yes |
| `Question.choice(instructions, options)` | Which one of these options? | The most probable option, its confidence, every option's probability |
| `Question.score(instructions, levels)` | Which level, lowest first? | The expected level (it can fall between two), its confidence, every level's probability |

- A Noul's `yes` and `no` say what each answer means, for when the boundary is subtle. Give both or
  neither.
- A Choice's `options` is a dict of name to description, or an iterable of `(name, description)`
  pairs or bare names (a bare name has no description). Up to 255, kept in the order given.
- A Score takes up to ten levels, lowest first: level 0, level 1, and so on.
- The id a question is asked under is never shown to the model, so `instructions` should say
  everything.

`questions` is a dict of id to question, or an iterable of `(id, question)` pairs. With pairs, an
id given twice is refused (`InvalidQuestionError`) instead of one question silently replacing the
other. A question can also be a dict in the endpoint's own shape:

```python
client.decide(state, {"blocked": {"type": "noul", "instructions": "Is the sender unable to work?"}})
```

Instructions and every criterion are JSON values: a string usually, but a dict, a list or `None`
works too (the endpoint's "Advanced: structure").

```python
jev.Question.choice({"question": "Which team?", "focus": "the primary request"}, ["billing", "support"])
```

A questions file, as `jev -q` reads it, loads into a dict of `Question`s in the file's order:

```python
questions = jev.load_questions(open("examples/rules/triage.json").read())
```

| On a `Question` | |
| --- | --- |
| `kind` | `"choice"`, `"score"` or `"noul"` |
| `instructions` | as given |
| `to_dict()` / `Question.from_dict(d)` | the endpoint's shape |
| `check()` | raises `InvalidQuestionError` if the endpoint can't answer it: no options, an option twice, too many levels |

Questions are checked before any call, so one that can't be answered costs nothing.

## The state

What the questions are about: a string, or anything JSON can hold, such as a dict or a list.

```python
client.decide({"subject": "Refund", "body": "I was charged twice", "plan": "pro"}, questions)
```

## The reply

```python
reply.noul("is_urgent")                  # 0.95: a float
reply.choice("department")               # ChoiceAnswer
reply.score("frustration")               # ScoreAnswer
reply["department"]                      # the answer, of whichever type
"department" in reply, len(reply)
reply.answers                            # every answer by id
reply.model, reply.id, reply.provider    # "typesafe/jev-1.13-20260917", OpenRouter's id, "TypeSafe"
reply.usage                              # input_tokens, output_tokens, cost (US dollars, or None)
```

`choice`, `score` and `noul` check the type as well: asking for a Score under a Choice's id raises
`WrongTypeError`, and an id with no answer raises `MissingAnswerError`.

| Answer | Fields |
| --- | --- |
| `ChoiceAnswer` | `choice`, `confidence`, `probabilities: dict[str, float]` |
| `ScoreAnswer` | `score`, `confidence`, `probabilities: dict[int, float]`, `legend: dict[int, str]` (each level's text) |
| `NoulAnswer` | `noul`; `float(answer)` works too |

Every answer also has `kind`. An answer of a type this package doesn't know yet comes back as the
dict that arrived, so nothing the endpoint sends is lost.

`confidence` is a second axis: act on a confident answer, and send the rest to a person.

```python
department = reply.choice("department")
if department.confidence < 0.6:
    route_to_a_person()
```

A reply prints as the command prints it, and goes to and from JSON:

```python
print(reply.render("text"))                  # a line per question; the default
print(reply.render("table", width=120))      # as `jev --table`
reply.render("json")                         # as `jev --json`
reply.render(ids=["is_urgent", "department"])  # these questions, in this order

text = reply.to_json(indent=True)
jev.DecisionResponse.from_json(text)         # and from_dict / to_dict
```

## Rules

A rules file is a policy over the answers: each term reads a probability as the degree to which a
condition holds, and rules combine them with `AND`, `OR`, `NOT` and hedges into a support score per
item, or a crisp amount per output. The file's format, every operator and every formula are in
[`rules.md`](rules.md); Python reads the same file.

```python
questions = jev.load_questions(open("examples/rules/triage.json").read())
rules = jev.Rules(open("examples/rules/triage.toml").read(), questions)

reply = client.decide(ticket, questions)
outcome = rules.evaluate(reply)
```

The rules are checked against the questions when they are made, so a term naming a level a question
doesn't have raises `RulesError` before any call.

```python
outcome.yes                                  # ["page on-call"]: the items at or over the threshold
outcome.threshold                            # 0.6
outcome["page on-call"].score                # 0.59
outcome["escalate to a lead"].rules          # [{"if": "VERY angry OR ...", "weight": 0.9, "score": 0.48}]
outcome["reply_within"].value                # 2.17: an output's crisp value, or None if no rule fired
outcome["reply_within"].sets                 # [("soon", 1.0), ("same_day", 0.0), ("later", 0.0)]
print(outcome.text())                        # as the command prints it
outcome.to_dict()                            # as `jev -r --json` has it under "outcome"
```

| On an `Outcome` | |
| --- | --- |
| `threshold` | the score at or over which an item is a yes |
| `items` | every `Item`, in the order the file first names it |
| `outputs` | every `OutputValue` |
| `yes` | the names of the items that reach the threshold |
| `outcome[name]` | the item or output named `name`; `KeyError` otherwise |
| `text()`, `to_dict()` | |

An `Item` has `name`, `score`, `yes` and `rules`, and is truthy when it is a yes. An `OutputValue`
has `name`, `value`, `sets` (each set's score, along the output's range) and `rules` (each set's
rules). A support score is the policy's, not a probability; an output's value says where the support
lies, not how much there is, so read its sets' scores before acting on it
([what the numbers mean](rules.md#what-the-numbers-mean)).

`evaluate` checks every answer the terms read: a probability outside 0 to 1, a distribution that
can't be one rounded to two places, or a level the question doesn't have raises `BadAnswerError`,
never a number that only looks like an answer.

## Drawing the rules

```python
open("triage.svg", "w").write(rules.graph_svg(reply))   # every part with its number
open("rules.svg", "w").write(rules.graph_svg())         # the structure alone: no reply, no call
print(rules.graph_text(reply))                          # for a terminal, as `jev --graph`
```

[`python/examples/triage.py`](../python/examples/triage.py) does all of it: it asks the triage
questions about a ticket, prints the answers and the outcome, and writes `triage.svg`; without a key
it draws the rules alone.

```sh
python python/examples/triage.py "I can't log in and payroll is due today"
```

![The triage rules drawn as a fuzzy rule base](triage.svg)

## Errors

Everything this package raises is a `jev.JevError`, apart from a `ValueError` for an argument of the
wrong shape (a Noul with only `yes`, a timeout of 0, an unknown render format).

| Exception | When |
| --- | --- |
| `InvalidQuestionError` | A question the endpoint can't answer as asked, or an id asked twice. Before any call. |
| `RulesError` | A rules file that doesn't parse or doesn't fit the questions. Before any call. |
| `HttpError` | The request couldn't be sent, its reply couldn't be read, or it timed out. |
| `StatusError` | The endpoint answered with an error; `.status` is the HTTP status, and the message is the endpoint's. |
| `DecodeError` | A reply that couldn't be decoded. |
| `MissingAnswerError` | No answer under that id. |
| `WrongTypeError` | The answer under that id is of another type than the one asked for. |
| `BadAnswerError` | An answer that isn't well formed: a probability missing or outside 0 to 1, a distribution no real one rounds to, or a level or option the question doesn't have. `decide` and `send` check every reply this way too. |
| `NumericalError` | An output whose sets had support, but whose value the arithmetic couldn't give (a range or a support too extreme for a float). Never reported as no support. |

```python
try:
    reply = client.decide(ticket, questions)
except jev.StatusError as error:
    if error.status == 401:
        ...
```

## Threads and asyncio

`decide` releases the GIL while it waits, so threads ask in parallel:

```python
from concurrent.futures import ThreadPoolExecutor

with ThreadPoolExecutor(8) as pool:
    replies = list(pool.map(lambda ticket: client.decide(ticket, questions), tickets))
```

`decide_async` is the same for asyncio:

```python
replies = await asyncio.gather(*(client.decide_async(ticket, questions) for ticket in tickets))
```

One client can be shared by every thread and task; it keeps a connection pool.

## Without the network

`client.request(state, questions)` is the body `decide` sends, checked but not sent, which needs no
key. A saved reply reads back with `DecisionResponse.from_json`, so rules can be written and tested
against fixed answers, without a call:

```python
reply = jev.DecisionResponse.from_json(open("tests/fixtures/decision.json").read())
```

## API reference

Everything is importable from `jev`; types are in `jev/_jev.pyi`.

| Name | |
| --- | --- |
| `Client(key=None, *, model=None, url=None, timeout=None)` | `.decide`, `.decide_async`, `.request`, `.model`, `.url` |
| `Question` | `.choice`, `.score`, `.noul`, `.from_dict`; `.kind`, `.instructions`, `.to_dict()`, `.check()` |
| `load_questions(text)` | a `jev -q` file's questions, as a dict |
| `DecisionResponse` | `.choice`, `.score`, `.noul`, `.answer`, `[id]`, `.answers`, `.model`, `.id`, `.provider`, `.usage`, `.render`, `.to_dict`, `.to_json`, `.from_dict`, `.from_json` |
| `ChoiceAnswer`, `ScoreAnswer`, `NoulAnswer`, `Usage` | the fields above |
| `Rules(text, questions)` | `.evaluate`, `.graph_svg`, `.graph_text` |
| `Outcome`, `Item`, `OutputValue` | the fields above |
| `DEFAULT_MODEL`, `DECISIONS_URL`, `DEFAULT_TIMEOUT`, `__version__` | |
| `JevError` and its subclasses | [Errors](#errors) |

## Building and releasing

The bindings are the crate in [`python/`](../python), built by [maturin](https://www.maturin.rs)
with [PyO3](https://pyo3.rs). It is its own crate, with its own `Cargo.lock`, over the library with
the `command` feature.

```sh
cd python
uv venv && uv pip install maturin pytest
.venv/bin/maturin develop        # build into .venv; again after any change to the Rust
.venv/bin/pytest                 # offline: `decide` is tested against a local server
```

`maturin develop` rather than `cargo build`: an extension module doesn't link on its own on macOS.

CI (`ci.yml`) checks formatting, runs clippy and runs the tests. `python.yml` builds the Linux wheels
and the source distribution on every change to the library or the bindings, installs each wheel and
tests it. A tag, or a run started by hand from the Actions tab, builds the macOS wheels too (the
Apple-silicon one tested; the Intel one, cross-compiled, not). Pushing a tag `vX.Y.Z` also publishes
them all to PyPI through trusted publishing, in
the `pypi` environment, once it has checked that the tag, `Cargo.toml` and `python/Cargo.toml` all
say `X.Y.Z`. The same tag builds the command's binaries (`release.yml`).

To release:

1. Raise `version` in both `Cargo.toml` and `python/Cargo.toml`.
2. Merge, then `git tag vX.Y.Z && git push origin vX.Y.Z`.

A version on PyPI can't be replaced, only followed by a newer one.
