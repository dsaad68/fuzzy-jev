//! Python bindings for fuzzy-jev: `import jev`. A client for Jev's decisions endpoint, the questions it
//! asks and the reply, and the fuzzy rules over a reply, with their drawings. The shapes are the
//! library's; this file only moves values across. `jev/__init__.py` re-exports the module, and
//! `jev/__init__.pyi` types it.
//!
//! Instructions, criteria and a state are any JSON value, so they cross as Python values through
//! `pythonize`: a `str` usually, but a `dict`, a `list` or `None` works too.

use std::collections::BTreeMap;
use std::time::Duration;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString};
use pythonize::{depythonize, pythonize};
use serde_json::Value;

create_exception!(jev, JevError, PyException, "Every error this package raises.");
create_exception!(jev, HttpError, JevError, "The request could not be sent, or its reply could not be read.");
create_exception!(jev, StatusError, JevError, "The endpoint answered with an error status; `status` is the HTTP status.");
create_exception!(jev, DecodeError, JevError, "A request could not be encoded, or a reply could not be decoded.");
create_exception!(
    jev,
    InvalidQuestionError,
    JevError,
    "A question the endpoint can't answer as asked, an id asked twice, or a question a yes/no-only model can't take."
);
create_exception!(jev, MissingAnswerError, JevError, "The reply has no answer under this question id.");
create_exception!(jev, WrongTypeError, JevError, "The answer under this id is of another type than the one asked for.");
create_exception!(
    jev,
    BadAnswerError,
    JevError,
    "An answer that isn't well formed: a probability missing, out of range, or not a distribution."
);
create_exception!(jev, NumericalError, JevError, "An output whose sets had support, but whose value the arithmetic couldn't give.");
create_exception!(jev, RulesError, JevError, "A rules file that doesn't parse, or doesn't fit the questions.");

/// The library's error as the exception of its kind, with its message.
fn error(e: jev::Error) -> PyErr {
    let message = e.to_string();
    match e {
        jev::Error::Http(_) => HttpError::new_err(message),
        jev::Error::Status { status, .. } => {
            let err = StatusError::new_err(message);
            Python::attach(|py| {
                let _ = err.value(py).setattr("status", status);
            });
            err
        }
        jev::Error::Decode(_) => DecodeError::new_err(message),
        jev::Error::DuplicateQuestion(_) | jev::Error::Invalid { .. } => InvalidQuestionError::new_err(message),
        jev::Error::MissingAnswer(_) => MissingAnswerError::new_err(message),
        jev::Error::WrongType { .. } => WrongTypeError::new_err(message),
        jev::Error::MissingProbability { .. } | jev::Error::BadAnswer { .. } => BadAnswerError::new_err(message),
        jev::Error::Numerical { .. } => NumericalError::new_err(message),
    }
}

fn value(object: &Bound<'_, PyAny>) -> PyResult<Value> {
    Ok(depythonize(object)?)
}

fn object<'py>(py: Python<'py>, value: &impl serde::Serialize) -> PyResult<Bound<'py, PyAny>> {
    Ok(pythonize(py, value)?)
}

/// A question: a Choice between options, a Score over levels, or a Noul. Made with
/// `Question.choice`, `Question.score` or `Question.noul`, or read from the endpoint's own JSON shape
/// with `Question.from_dict`.
#[pyclass(module = "jev", frozen, eq, skip_from_py_object)]
#[derive(Clone, PartialEq)]
struct Question(jev::Question);

#[pymethods]
impl Question {
    /// A Choice between `options`, kept in the order given: a dict of name to description, or an
    /// iterable of `(name, description)` pairs or of bare names.
    #[staticmethod]
    fn choice(instructions: &Bound<'_, PyAny>, options: &Bound<'_, PyAny>) -> PyResult<Question> {
        let options = match options.cast::<PyDict>() {
            Ok(dict) => dict.items().into_any(),
            Err(_) => options.clone(),
        };
        let mut named = Vec::new();
        for option in options.try_iter()? {
            let option = option?;
            named.push(match option.cast::<PyString>() {
                Ok(name) => (name.to_string(), Value::Null),
                Err(_) => {
                    let (name, description): (String, Bound<'_, PyAny>) =
                        option.extract().map_err(|_| PyValueError::new_err("an option is a name, or a (name, description) pair"))?;
                    (name, value(&description)?)
                }
            });
        }
        Ok(Question(jev::Question::choice(value(instructions)?, named)))
    }

    /// A Score over `levels`, from the lowest (level 0) to the highest.
    #[staticmethod]
    fn score(instructions: &Bound<'_, PyAny>, levels: &Bound<'_, PyAny>) -> PyResult<Question> {
        let levels = levels.try_iter()?.map(|level| value(&level?)).collect::<PyResult<Vec<_>>>()?;
        Ok(Question(jev::Question::score(value(instructions)?, levels)))
    }

    /// A Noul: the probability of yes. `yes` and `no` say what each means, for when the boundary is
    /// subtle; give both or neither.
    #[staticmethod]
    #[pyo3(signature = (instructions, yes = None, no = None))]
    fn noul(instructions: &Bound<'_, PyAny>, yes: Option<&Bound<'_, PyAny>>, no: Option<&Bound<'_, PyAny>>) -> PyResult<Question> {
        let instructions = value(instructions)?;
        Ok(Question(match (yes, no) {
            (None, None) => jev::Question::noul(instructions),
            (Some(yes), Some(no)) => jev::Question::noul_with_criteria(instructions, value(yes)?, value(no)?),
            _ => return Err(PyValueError::new_err("a noul takes both `yes` and `no`, or neither")),
        }))
    }

    /// A question in the endpoint's shape: `{"type": "choice", "instructions": ..., "criteria": ...}`.
    #[staticmethod]
    fn from_dict(question: &Bound<'_, PyAny>) -> PyResult<Question> {
        Ok(Question(depythonize(question)?))
    }

    /// `"choice"`, `"score"` or `"noul"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            jev::Question::Choice { .. } => "choice",
            jev::Question::Score { .. } => "score",
            jev::Question::Noul { .. } => "noul",
        }
    }

    #[getter]
    fn instructions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match &self.0 {
            jev::Question::Choice { instructions, .. }
            | jev::Question::Score { instructions, .. }
            | jev::Question::Noul { instructions, .. } => object(py, instructions),
        }
    }

    /// The question in the endpoint's shape, as `Question.from_dict` reads it.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        object(py, &self.0)
    }

    /// Raises `InvalidQuestionError` when the endpoint can't answer it as asked: no options, an
    /// option twice, too many levels.
    fn check(&self) -> PyResult<()> {
        self.0.check().map_err(InvalidQuestionError::new_err)
    }

    fn __repr__(&self) -> String {
        format!("Question({})", serde_json::to_string(&self.0).unwrap_or_default())
    }
}

/// Questions under their ids: a dict, or an iterable of `(id, question)` pairs, where a question is
/// a `Question` or a dict in the endpoint's shape. Pairs keep an id given twice, so that the client
/// can refuse it rather than keep one of the two.
fn questions(questions: &Bound<'_, PyAny>) -> PyResult<Vec<(String, jev::Question)>> {
    let pairs = match questions.cast::<PyDict>() {
        Ok(dict) => dict.items().into_any(),
        Err(_) => questions.clone(),
    };
    let mut asked = Vec::new();
    for pair in pairs.try_iter()? {
        let (id, question): (String, Bound<'_, PyAny>) =
            pair?.extract().map_err(|_| PyValueError::new_err("questions are a dict of id to question, or (id, question) pairs"))?;
        let question = match question.cast::<Question>() {
            Ok(question) => question.get().0.clone(),
            Err(_) => depythonize(&question)?,
        };
        asked.push((id, question));
    }
    Ok(asked)
}

/// Reads a questions file's text (a JSON object of ids to questions, as `jev -q` reads it) into a
/// dict of `Question`s, in the file's order. Every question is checked.
#[pyfunction]
fn load_questions<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (id, question) in jev::spec::questions_file(text).map_err(InvalidQuestionError::new_err)? {
        dict.set_item(id, Question(question))?;
    }
    Ok(dict)
}

/// Sends questions about a state to Jev. `key` is an OpenRouter API key; without one it is read
/// from `OPENROUTER_API_KEY`, and an empty key sends no `Authorization` header, for an endpoint
/// (`url`) that adds its own. `timeout` is in seconds, for the whole request.
#[pyclass(module = "jev", frozen)]
struct Client {
    inner: jev::Client,
    model: String,
    url: String,
}

#[pymethods]
impl Client {
    #[new]
    #[pyo3(signature = (key = None, *, model = None, url = None, timeout = None))]
    fn new(key: Option<String>, model: Option<String>, url: Option<String>, timeout: Option<f64>) -> PyResult<Client> {
        let key = key.unwrap_or_else(|| std::env::var("OPENROUTER_API_KEY").unwrap_or_default());
        let model = model.unwrap_or_else(|| jev::DEFAULT_MODEL.to_owned());
        let url = url.unwrap_or_else(|| jev::DECISIONS_URL.to_owned());
        let mut inner = jev::Client::new(&key).with_model(&model).with_url(&url);
        if let Some(timeout) = timeout {
            let timeout = Duration::try_from_secs_f64(timeout)
                .ok()
                .filter(|timeout| !timeout.is_zero())
                .ok_or_else(|| PyValueError::new_err(format!("timeout is {timeout}; it is a number of seconds over 0")))?;
            inner = inner.with_timeout(timeout);
        }
        Ok(Client { inner, model, url })
    }

    #[getter]
    fn model(&self) -> &str {
        &self.model
    }

    #[getter]
    fn url(&self) -> &str {
        &self.url
    }

    /// The request `decide` sends, as a dict, without sending it. Costs no call.
    fn request<'py>(&self, py: Python<'py>, state: &Bound<'py, PyAny>, questions: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        object(py, &self.build(state, questions)?)
    }

    /// Asks `questions` about `state` and waits for the reply. Other Python threads run meanwhile.
    fn decide(&self, py: Python<'_>, state: &Bound<'_, PyAny>, questions: &Bound<'_, PyAny>) -> PyResult<DecisionResponse> {
        let request = self.build(state, questions)?;
        let client = self.inner.clone();
        let reply = py.detach(|| pyo3_async_runtimes::tokio::get_runtime().block_on(async move { client.send(&request).await }));
        reply.map(DecisionResponse).map_err(error)
    }

    /// `decide`, awaitable.
    fn decide_async<'py>(&self, py: Python<'py>, state: &Bound<'py, PyAny>, questions: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        let request = self.build(state, questions)?;
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move { client.send(&request).await.map(DecisionResponse).map_err(error) })
    }

    fn __repr__(&self) -> String {
        // Never the key.
        format!("Client(model={:?}, url={:?})", self.model, self.url)
    }
}

impl Client {
    fn build(&self, state: &Bound<'_, PyAny>, asked: &Bound<'_, PyAny>) -> PyResult<jev::DecisionRequest> {
        self.inner.request(value(state)?, questions(asked)?).map_err(error)
    }
}

/// The reply: one answer per question, under the ids from the request. `reply["id"]` is the answer
/// to question `id`; `choice`, `score` and `noul` also check its type.
#[pyclass(module = "jev", frozen)]
struct DecisionResponse(jev::DecisionResponse);

#[pymethods]
impl DecisionResponse {
    /// A reply from its JSON text, as the endpoint (or `to_json`) writes it.
    #[staticmethod]
    fn from_json(text: &str) -> PyResult<DecisionResponse> {
        serde_json::from_str(text).map(DecisionResponse).map_err(|e| DecodeError::new_err(e.to_string()))
    }

    /// A reply from a dict in the endpoint's shape.
    #[staticmethod]
    fn from_dict(reply: &Bound<'_, PyAny>) -> PyResult<DecisionResponse> {
        Ok(DecisionResponse(depythonize(reply).map_err(|e| DecodeError::new_err(e.to_string()))?))
    }

    /// The model that answered, with its date, such as `typesafe/jev-1.13-20260917`.
    #[getter]
    fn model(&self) -> &str {
        &self.0.model
    }

    /// OpenRouter's id for the call.
    #[getter]
    fn id(&self) -> Option<&str> {
        self.0.id.as_deref()
    }

    #[getter]
    fn provider(&self) -> Option<&str> {
        self.0.provider.as_deref()
    }

    #[getter]
    fn usage(&self) -> Usage {
        let usage = &self.0.usage;
        Usage { input_tokens: usage.input_tokens, output_tokens: usage.output_tokens, cost: usage.cost }
    }

    /// Every answer by question id.
    #[getter]
    fn answers<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (id, answer) in &self.0.answers {
            dict.set_item(id, answer_object(py, answer)?)?;
        }
        Ok(dict)
    }

    /// The answer to question `id`, of whichever type.
    fn answer<'py>(&self, py: Python<'py>, id: &str) -> PyResult<Bound<'py, PyAny>> {
        answer_object(py, self.0.answer(id).map_err(error)?)
    }

    /// The Choice answer to question `id`.
    fn choice(&self, id: &str) -> PyResult<ChoiceAnswer> {
        self.0.choice(id).cloned().map(ChoiceAnswer).map_err(error)
    }

    /// The Score answer to question `id`.
    fn score(&self, id: &str) -> PyResult<ScoreAnswer> {
        self.0.score(id).cloned().map(ScoreAnswer).map_err(error)
    }

    /// The probability of yes for Noul question `id`.
    fn noul(&self, id: &str) -> PyResult<f64> {
        self.0.noul(id).map_err(error)
    }

    /// The reply as the endpoint's JSON shape, in a dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        object(py, &self.0)
    }

    /// The reply as JSON text, which `from_json` reads back.
    #[pyo3(signature = (indent = false))]
    fn to_json(&self, indent: bool) -> PyResult<String> {
        let text = if indent { serde_json::to_string_pretty(&self.0) } else { serde_json::to_string(&self.0) };
        text.map_err(|e| DecodeError::new_err(e.to_string()))
    }

    /// The reply as `jev` prints it: `format` is `"text"` (a line per question), `"table"` (kept
    /// within `width` columns) or `"json"`. `ids` picks the questions and their order; all of them,
    /// by id, without it. Ends with a newline.
    #[pyo3(signature = (format = "text", ids = None, width = 100))]
    fn render(&self, format: &str, ids: Option<Vec<String>>, width: usize) -> PyResult<String> {
        let ids = ids.unwrap_or_else(|| self.0.answers.keys().cloned().collect());
        jev::print::render(print_format(format)?, &self.0, &ids, width).map_err(|e| DecodeError::new_err(e.to_string()))
    }

    fn __getitem__<'py>(&self, py: Python<'py>, id: &str) -> PyResult<Bound<'py, PyAny>> {
        self.answer(py, id)
    }

    fn __contains__(&self, id: &str) -> bool {
        self.0.answers.contains_key(id)
    }

    fn __len__(&self) -> usize {
        self.0.answers.len()
    }

    fn __repr__(&self) -> String {
        let ids: Vec<&str> = self.0.answers.keys().map(String::as_str).collect();
        format!("DecisionResponse(model={:?}, answers={ids:?})", self.0.model)
    }
}

fn print_format(format: &str) -> PyResult<jev::print::Format> {
    Ok(match format {
        "text" => jev::print::Format::Text,
        "table" => jev::print::Format::Table,
        "json" => jev::print::Format::Json,
        _ => return Err(PyValueError::new_err(format!("format is {format:?}; it is \"text\", \"table\" or \"json\""))),
    })
}

/// An answer as its class, or a dict for a type this package doesn't know yet.
fn answer_object<'py>(py: Python<'py>, answer: &jev::Answer) -> PyResult<Bound<'py, PyAny>> {
    Ok(match answer {
        jev::Answer::Choice(answer) => ChoiceAnswer(answer.clone()).into_pyobject(py)?.into_any(),
        jev::Answer::Score(answer) => ScoreAnswer(answer.clone()).into_pyobject(py)?.into_any(),
        jev::Answer::Noul(answer) => NoulAnswer(answer.clone()).into_pyobject(py)?.into_any(),
        jev::Answer::Other(answer) => object(py, answer)?,
    })
}

/// What a call used.
#[pyclass(module = "jev", frozen, get_all)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
    /// What the call cost, in US dollars, when OpenRouter says.
    cost: Option<f64>,
}

#[pymethods]
impl Usage {
    fn __repr__(&self) -> String {
        format!("Usage(input_tokens={}, output_tokens={}, cost={:?})", self.input_tokens, self.output_tokens, self.cost)
    }
}

/// A Choice's answer: the most probable option, how peaked the distribution is, and every option's
/// probability.
#[pyclass(module = "jev", frozen)]
struct ChoiceAnswer(jev::ChoiceAnswer);

#[pymethods]
impl ChoiceAnswer {
    #[getter]
    fn kind(&self) -> &'static str {
        "choice"
    }

    /// The most probable option.
    #[getter]
    fn choice(&self) -> &str {
        &self.0.choice
    }

    /// From 0 to 1: how peaked `probabilities` is.
    #[getter]
    fn confidence(&self) -> f64 {
        self.0.confidence
    }

    /// Every option's probability; they add up to 1.
    #[getter]
    fn probabilities(&self) -> BTreeMap<String, f64> {
        self.0.probabilities.clone()
    }

    fn __repr__(&self) -> String {
        format!("ChoiceAnswer(choice={:?}, confidence={}, probabilities={:?})", self.0.choice, self.0.confidence, self.0.probabilities)
    }
}

/// A Score's answer: the expected level, which can fall between two, how peaked the distribution
/// is, and each level's probability and description by level number.
#[pyclass(module = "jev", frozen)]
struct ScoreAnswer(jev::ScoreAnswer);

#[pymethods]
impl ScoreAnswer {
    #[getter]
    fn kind(&self) -> &'static str {
        "score"
    }

    /// The expected level, from 0 to the top level.
    #[getter]
    fn score(&self) -> f64 {
        self.0.score
    }

    /// From 0 to 1: how peaked `probabilities` is.
    #[getter]
    fn confidence(&self) -> f64 {
        self.0.confidence
    }

    /// Each level's probability, by level number; they add up to 1.
    #[getter]
    fn probabilities(&self) -> BTreeMap<u8, f64> {
        self.0.probabilities.clone()
    }

    /// Each level's description, by level number.
    #[getter]
    fn legend<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for (level, description) in &self.0.legend {
            dict.set_item(level, object(py, description)?)?;
        }
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        format!("ScoreAnswer(score={}, confidence={}, probabilities={:?})", self.0.score, self.0.confidence, self.0.probabilities)
    }
}

/// A Noul's answer: the probability of yes.
#[pyclass(module = "jev", frozen)]
struct NoulAnswer(jev::NoulAnswer);

#[pymethods]
impl NoulAnswer {
    #[getter]
    fn kind(&self) -> &'static str {
        "noul"
    }

    /// The probability that the answer is yes.
    #[getter]
    fn noul(&self) -> f64 {
        self.0.noul
    }

    fn __float__(&self) -> f64 {
        self.0.noul
    }

    fn __repr__(&self) -> String {
        format!("NoulAnswer(noul={})", self.0.noul)
    }
}

/// A rules file (TOML, as `jev -r` reads it), parsed and checked against the questions it will read,
/// so a term naming a level a question doesn't have fails here, before any call.
#[pyclass(module = "jev", frozen)]
struct Rules(jev::rules::Rules);

#[pymethods]
impl Rules {
    #[new]
    fn new(text: &str, questions: &Bound<'_, PyAny>) -> PyResult<Rules> {
        let asked = self::questions(questions)?;
        jev::rules::Rules::parse(text, asked.iter().map(|(id, question)| (id.as_str(), question))).map(Rules).map_err(RulesError::new_err)
    }

    /// What the rules make of `reply`: each item's score and whether it reaches the threshold, and
    /// each output's value.
    fn evaluate(&self, reply: &DecisionResponse) -> PyResult<Outcome> {
        self.0.evaluate(&reply.0).map(Outcome).map_err(error)
    }

    /// The rules drawn for a terminal; with `reply`, every part carries its number.
    #[pyo3(signature = (reply = None))]
    fn graph_text(&self, reply: Option<&DecisionResponse>) -> PyResult<String> {
        self.0.graph_text(reply.map(|reply| &reply.0)).map_err(error)
    }

    /// The rules drawn as an SVG image; with `reply`, every part carries its number.
    #[pyo3(signature = (reply = None))]
    fn graph_svg(&self, reply: Option<&DecisionResponse>) -> PyResult<String> {
        self.0.graph_svg(reply.map(|reply| &reply.0)).map_err(error)
    }
}

/// What rules made of a reply. A support score is the policy's, not a probability.
#[pyclass(module = "jev", frozen)]
struct Outcome(jev::rules::Outcome);

#[pymethods]
impl Outcome {
    /// The score at or over which an item is a yes.
    #[getter]
    fn threshold(&self) -> f64 {
        self.0.threshold
    }

    /// Every item a `then` names, in the order the file first names it.
    #[getter]
    fn items(&self) -> Vec<Item> {
        self.0.items.iter().map(|item| Item(item.clone())).collect()
    }

    /// Every `[output]`, with its crisp value.
    #[getter]
    fn outputs(&self) -> Vec<OutputValue> {
        self.0.outputs.iter().map(|output| OutputValue(output.clone())).collect()
    }

    /// The items that reach the threshold, by name.
    #[getter]
    fn yes(&self) -> Vec<String> {
        self.0.items.iter().filter(|item| item.yes).map(|item| item.item.clone()).collect()
    }

    /// The item named `name`, or the output named `name`.
    fn __getitem__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        if let Some(item) = self.0.items.iter().find(|item| item.item == name) {
            return Ok(Item(item.clone()).into_pyobject(py)?.into_any().unbind());
        }
        if let Some(output) = self.0.outputs.iter().find(|output| output.output == name) {
            return Ok(OutputValue(output.clone()).into_pyobject(py)?.into_any().unbind());
        }
        Err(pyo3::exceptions::PyKeyError::new_err(name.to_owned()))
    }

    /// One line per item, then the threshold, then a line per output. Ends with a newline.
    fn text(&self) -> String {
        self.0.text()
    }

    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        object(py, &self.0)
    }

    fn __repr__(&self) -> String {
        let yes = self.yes();
        format!("Outcome(threshold={}, yes={yes:?})", self.0.threshold)
    }
}

/// One `then`: its score, whether that reaches the threshold, and the rules that gave it.
#[pyclass(module = "jev", frozen)]
struct Item(jev::rules::Item);

#[pymethods]
impl Item {
    #[getter]
    fn name(&self) -> &str {
        &self.0.item
    }

    #[getter]
    fn score(&self) -> f64 {
        self.0.score
    }

    #[getter]
    fn yes(&self) -> bool {
        self.0.yes
    }

    /// Each rule's part: its `if`, its weight and its score.
    #[getter]
    fn rules<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        fired(py, &self.0.rules)
    }

    fn __bool__(&self) -> bool {
        self.0.yes
    }

    fn __repr__(&self) -> String {
        format!("Item(name={:?}, score={}, yes={})", self.0.item, self.0.score, if self.0.yes { "True" } else { "False" })
    }
}

/// An output's crisp value, and each set's score. The value says where the support lies, not how much
/// there is: read the sets' scores before acting on it.
#[pyclass(module = "jev", frozen)]
struct OutputValue(jev::rules::OutputValue);

#[pymethods]
impl OutputValue {
    #[getter]
    fn name(&self) -> &str {
        &self.0.output
    }

    /// `None` when no rule concluding it scored above zero.
    #[getter]
    fn value(&self) -> Option<f64> {
        self.0.value
    }

    /// Each set's score, in order along the output's range.
    #[getter]
    fn sets(&self) -> Vec<(String, f64)> {
        self.0.sets.iter().map(|set| (set.set.clone(), set.score)).collect()
    }

    /// Each set's rules: set name to a list of `{"if", "weight", "score"}`.
    #[getter]
    fn rules<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for set in &self.0.sets {
            dict.set_item(&set.set, fired(py, &set.rules)?)?;
        }
        Ok(dict)
    }

    fn __repr__(&self) -> String {
        format!("OutputValue(name={:?}, value={}, sets=[{}])", self.0.output, self.0.value_text(), self.0.sets_text())
    }
}

fn fired<'py>(py: Python<'py>, rules: &[jev::rules::Fired]) -> PyResult<Bound<'py, PyList>> {
    let rules = rules.iter().map(|rule| object(py, rule)).collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, rules)
}

#[pymodule]
fn _jev(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("DEFAULT_MODEL", jev::DEFAULT_MODEL)?;
    // The supported models, the default first: each an `id`, what it is (`about`), and whether it
    // answers yes/no questions only (`noul_only`), which a request is checked against before it's sent.
    let models = jev::MODELS
        .iter()
        .map(|model| {
            let dict = PyDict::new(py);
            dict.set_item("id", model.id)?;
            dict.set_item("about", model.about)?;
            dict.set_item("noul_only", model.noul_only)?;
            Ok(dict)
        })
        .collect::<PyResult<Vec<_>>>()?;
    m.add("MODELS", PyList::new(py, models)?)?;
    m.add("DECISIONS_URL", jev::DECISIONS_URL)?;
    m.add("DEFAULT_TIMEOUT", jev::DEFAULT_TIMEOUT.as_secs_f64())?;
    m.add_class::<Client>()?;
    m.add_class::<Question>()?;
    m.add_class::<DecisionResponse>()?;
    m.add_class::<Usage>()?;
    m.add_class::<ChoiceAnswer>()?;
    m.add_class::<ScoreAnswer>()?;
    m.add_class::<NoulAnswer>()?;
    m.add_class::<Rules>()?;
    m.add_class::<Outcome>()?;
    m.add_class::<Item>()?;
    m.add_class::<OutputValue>()?;
    m.add_function(wrap_pyfunction!(load_questions, m)?)?;
    m.add("JevError", py.get_type::<JevError>())?;
    m.add("HttpError", py.get_type::<HttpError>())?;
    m.add("StatusError", py.get_type::<StatusError>())?;
    m.add("DecodeError", py.get_type::<DecodeError>())?;
    m.add("InvalidQuestionError", py.get_type::<InvalidQuestionError>())?;
    m.add("MissingAnswerError", py.get_type::<MissingAnswerError>())?;
    m.add("WrongTypeError", py.get_type::<WrongTypeError>())?;
    m.add("BadAnswerError", py.get_type::<BadAnswerError>())?;
    m.add("NumericalError", py.get_type::<NumericalError>())?;
    m.add("RulesError", py.get_type::<RulesError>())?;
    Ok(())
}
