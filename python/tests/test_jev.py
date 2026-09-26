"""The bindings, without a key or the network: `decide` goes to a local server."""

import asyncio
import json
import pathlib
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

import pytest

import jev

ROOT = pathlib.Path(__file__).resolve().parents[2]
FIXTURE = (ROOT / "tests" / "fixtures" / "decision.json").read_text()
RULES = ROOT / "examples" / "rules"


def triage():
    return {
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
    }


def test_builds_the_documented_request():
    request = jev.Client("key").request("Help! My payouts have been failing for 3 days.", triage())
    assert request == {
        "model": "~typesafe/jev-latest",
        "state": "Help! My payouts have been failing for 3 days.",
        "questions": {
            "is_urgent": {
                "type": "noul",
                "instructions": "Does this message convey urgency?",
                "criteria": {"true": "Explicitly time-sensitive", "false": "No urgency expressed"},
            },
            "department": {
                "type": "choice",
                "instructions": "Which team should handle this?",
                "criteria": {
                    "billing": "Payments, invoicing, refunds",
                    "technical": "Bugs, outages, integrations",
                    "sales": "Pricing, upgrades, new accounts",
                },
            },
            "frustration": {
                "type": "score",
                "instructions": "How frustrated is the customer?",
                "criteria": ["Calm", "Frustrated", "Very angry"],
            },
        },
    }


def test_questions_in_every_shape():
    options = jev.Question.choice("Which?", [("zeta", "last"), "alpha"])
    assert json.dumps(options.to_dict()) == '{"type": "choice", "instructions": "Which?", "criteria": {"zeta": "last", "alpha": null}}'
    assert options.kind == "choice"
    assert jev.Question.from_dict(options.to_dict()) == options
    structured = jev.Question.noul({"question": "Is it?", "focus": "the request"})
    assert structured.instructions == {"question": "Is it?", "focus": "the request"}
    request = jev.Client("").request({"message": "hi"}, [("q", {"type": "noul", "instructions": "Is it?"})])
    assert request["state"] == {"message": "hi"}
    assert request["questions"]["q"] == {"type": "noul", "instructions": "Is it?"}
    with pytest.raises(ValueError, match="both"):
        jev.Question.noul("Is it?", yes="yes")


def test_refuses_questions_before_a_call():
    client = jev.Client("key")
    with pytest.raises(jev.InvalidQuestionError, match="asked twice"):
        client.request("state", [("a", jev.Question.noul("Is it?")), ("a", jev.Question.noul("Really?"))])
    with pytest.raises(jev.InvalidQuestionError, match="up to 10 levels"):
        client.request("state", {"q": jev.Question.score("How much?", ["level"] * 11)})
    with pytest.raises(jev.InvalidQuestionError, match="twice"):
        jev.Question.choice("Which?", ["a", "a"]).check()


def test_reads_a_reply():
    reply = jev.DecisionResponse.from_json(FIXTURE)
    assert reply.model == "typesafe/jev-1.13-20260917"
    assert reply.provider == "TypeSafe"
    assert reply.noul("is_urgent") == 0.95
    department = reply.choice("department")
    assert (department.choice, department.confidence) == ("billing", 0.82)
    assert department.probabilities["technical"] == 0.12
    frustration = reply.score("frustration")
    assert frustration.score == 1.04
    assert frustration.probabilities[1] == 0.96
    assert frustration.legend[2] == "Very angry"
    assert isinstance(reply["is_urgent"], jev.NoulAnswer)
    assert "department" in reply and len(reply) == 3
    assert reply.usage.input_tokens == 427 and reply.usage.cost == pytest.approx(0.000017934)
    assert jev.DecisionResponse.from_json(reply.to_json()).to_dict() == reply.to_dict()
    assert reply.render("text", ids=["is_urgent"]).startswith("is_urgent")
    assert json.loads(reply.render("json"))["model"] == reply.model


def test_says_which_answer_is_missing_or_of_another_type():
    reply = jev.DecisionResponse.from_json(FIXTURE)
    with pytest.raises(jev.MissingAnswerError, match="nope"):
        reply.noul("nope")
    with pytest.raises(jev.WrongTypeError, match="is a choice, not a score"):
        reply.score("department")
    with pytest.raises(ValueError):
        reply.render("yaml")


def test_keeps_an_unknown_answer_type():
    reply = jev.DecisionResponse.from_dict(
        {"model": "m", "answers": {"q": {"type": "ranking", "order": [1, 2]}}, "usage": {"input_tokens": 1, "output_tokens": 1}}
    )
    assert reply["q"] == {"type": "ranking", "order": [1, 2]}
    assert reply.usage.cost is None


def test_rules_over_a_reply():
    questions = jev.load_questions((RULES / "triage.json").read_text())
    assert list(questions) == ["team", "urgency", "blocked", "anger"]
    rules = jev.Rules((RULES / "triage.toml").read_text(), questions)
    reply = jev.DecisionResponse.from_dict(
        {
            "model": "m",
            "answers": {
                "team": {"type": "choice", "choice": "billing", "confidence": 0.8,
                         "probabilities": {"billing": 0.9, "support": 0.1, "security": 0.0, "other": 0.0}},
                "urgency": {"type": "score", "score": 1.8, "confidence": 0.7, "probabilities": {"0": 0.0, "1": 0.2, "2": 0.8}},
                "blocked": {"type": "noul", "noul": 0.9},
                "anger": {"type": "score", "score": 1.0, "confidence": 0.5, "probabilities": {"0": 0.2, "1": 0.6, "2": 0.2}},
            },
            "usage": {"input_tokens": 1, "output_tokens": 1},
        }
    )
    outcome = rules.evaluate(reply)
    assert outcome.threshold == 0.6
    assert "page on-call" in outcome.yes
    assert outcome["page on-call"].score == pytest.approx(0.8)
    assert outcome["page on-call"].rules[0]["if"] == "blocked AND (security OR today)"
    reply_within = outcome["reply_within"]
    assert reply_within.value is not None and 0 < reply_within.value < 72
    assert [name for name, _ in reply_within.sets] == ["soon", "same_day", "later"]
    assert "threshold 0.60" in outcome.text()
    assert outcome.to_dict()["items"][0]["item"] == "page on-call"
    assert rules.graph_svg(reply).startswith("<svg")
    assert "page on-call" in rules.graph_text()


def test_refuses_rules_that_dont_fit():
    questions = jev.load_questions((RULES / "triage.json").read_text())
    with pytest.raises(jev.RulesError, match="isn't in \\[terms\\]"):
        jev.Rules('[[rule]]\nif = "nothing"\nthen = "x"\n', questions)
    with pytest.raises(jev.RulesError, match="Tomorrow"):
        jev.Rules('[terms]\nt = "urgency.Tomorrow"\n[[rule]]\nif = "t"\nthen = "x"\n', questions)


@pytest.fixture
def server():
    """A decisions endpoint on localhost: the fixture for a request, or a 401 without a key."""
    seen = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            seen.append((self.headers.get("authorization"), json.loads(self.rfile.read(int(self.headers["content-length"])))))
            if self.headers.get("authorization"):
                status, body = 200, FIXTURE
            else:
                status, body = 401, '{"error": {"message": "Missing Authentication header", "code": 401}}'
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.end_headers()
            self.wfile.write(body.encode())

        def log_message(self, *args):
            pass

    httpd = HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{httpd.server_port}/decisions", seen
    httpd.shutdown()


def test_decides(server):
    url, seen = server
    reply = jev.Client("key", url=url, model="typesafe/jev-latest", timeout=5).decide("state", triage())
    assert reply.choice("department").choice == "billing"
    assert seen[0][0] == "Bearer key"
    assert seen[0][1]["model"] == "typesafe/jev-latest"


def test_decides_async(server):
    url, _ = server

    async def ask():
        client = jev.Client("key", url=url)
        return await asyncio.gather(*(client.decide_async(f"state {n}", triage()) for n in range(3)))

    replies = asyncio.run(ask())
    assert [reply.noul("is_urgent") for reply in replies] == [0.95] * 3


def test_raises_the_endpoints_error(server):
    url, _ = server
    with pytest.raises(jev.StatusError, match="Missing Authentication header") as error:
        jev.Client("", url=url).decide("state", triage())
    assert error.value.status == 401
    assert isinstance(error.value, jev.JevError)


def test_refuses_a_bad_timeout():
    with pytest.raises(ValueError, match="timeout"):
        jev.Client("key", timeout=0)


def test_keeps_the_key_out_of_repr():
    assert "secret" not in repr(jev.Client("secret"))


def test_lists_the_supported_models():
    ids = [model["id"] for model in jev.MODELS]
    assert ids[0] == jev.DEFAULT_MODEL == "~typesafe/jev-latest"
    assert {"typesafe/jev-1.13", "jaredpalmer/kev-4b", "respan/span-01", "respan/span-01-lite", "respan/span-01-lite:free"} <= set(ids)
    assert all(model["noul_only"] == model["id"].startswith("respan/") for model in jev.MODELS)


def test_a_yes_no_model_is_refused_other_questions_before_any_call():
    # Nothing listens on the discard port: the refusal comes before a connection is tried.
    client = jev.Client("key", url="http://127.0.0.1:9", model="respan/span-01")
    with pytest.raises(jev.InvalidQuestionError, match="yes/no questions only"):
        client.decide("state", triage())
