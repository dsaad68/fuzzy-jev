"""Triage a support ticket with the rules in examples/rules/, and draw them as triage.svg:

    python examples/triage.py "I can't log in and payroll is due today"

With OPENROUTER_API_KEY set, Jev answers the questions, the rules decide, and the drawing carries
every number. Without it, the drawing is the rules alone, which costs no call.
"""

import os
import pathlib
import sys

import jev

RULES = pathlib.Path(__file__).resolve().parents[2] / "examples" / "rules"
ticket = sys.argv[1] if len(sys.argv) > 1 else "I can't log in to pay our invoice, and it is due today. This is the third time!"

questions = jev.load_questions((RULES / "triage.json").read_text())
# Checked against the questions now, so a rule naming a level they don't have costs no call.
rules = jev.Rules((RULES / "triage.toml").read_text(), questions)

reply = None
if os.environ.get("OPENROUTER_API_KEY"):
    reply = jev.Client().decide(ticket, questions)
    print(reply.render("text"), end="")
    print()
    print(rules.evaluate(reply).text(), end="")
else:
    print("OPENROUTER_API_KEY is not set: drawing the rules without answers")

svg = pathlib.Path("triage.svg")
svg.write_text(rules.graph_svg(reply))
print(f"wrote {svg.resolve()}")
