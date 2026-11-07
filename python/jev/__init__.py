"""A client for OpenRouter's decision models, TypeSafe's Jev by default.

A model reads a state (a string, a dict, or a list, which can hold images) and answers typed
questions about it: a Choice between options, a Score along levels, or a Noul, the probability that something is true. Rules
combine those answers, by fuzzy logic, into decisions and amounts.

    import jev

    client = jev.Client()  # the key from OPENROUTER_API_KEY
    reply = client.decide(
        "Help! My payouts have been failing for 3 days.",
        {
            "is_urgent": jev.Question.noul("Does this message convey urgency?"),
            "department": jev.Question.choice("Which team should handle this?", {"billing": "Payments", "technical": "Bugs"}),
            "frustration": jev.Question.score("How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"]),
        },
    )
    if reply.noul("is_urgent") > 0.8 and reply.choice("department").confidence > 0.6:
        print("page", reply.choice("department").choice)
"""

from ._jev import (
    DECISIONS_URL,
    DEFAULT_MODEL,
    DEFAULT_TIMEOUT,
    MODELS,
    MODELS_URL,
    BadAnswerError,
    NumericalError,
    ChoiceAnswer,
    Client,
    DecisionResponse,
    DecodeError,
    HttpError,
    ImageError,
    InvalidQuestionError,
    Item,
    JevError,
    MissingAnswerError,
    NoulAnswer,
    Outcome,
    OutputValue,
    Question,
    Rules,
    RulesError,
    ScoreAnswer,
    StatusError,
    Usage,
    WrongTypeError,
    __version__,
    image_part,
    load_questions,
    text_part,
)

__all__ = [
    "DECISIONS_URL",
    "DEFAULT_MODEL",
    "DEFAULT_TIMEOUT",
    "MODELS",
    "MODELS_URL",
    "BadAnswerError",
    "NumericalError",
    "ChoiceAnswer",
    "Client",
    "DecisionResponse",
    "DecodeError",
    "HttpError",
    "ImageError",
    "InvalidQuestionError",
    "Item",
    "JevError",
    "MissingAnswerError",
    "NoulAnswer",
    "Outcome",
    "OutputValue",
    "Question",
    "Rules",
    "RulesError",
    "ScoreAnswer",
    "StatusError",
    "Usage",
    "WrongTypeError",
    "__version__",
    "image_part",
    "load_questions",
    "text_part",
]
