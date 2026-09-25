//! `jev --explore`: a page in the browser to try a state, questions and rules, and see what Jev
//! answers, what the rules make of it as text, and their drawing as SVG.
//!
//! The page is served from this process, on the loopback address only. The key stays here, in
//! OPENROUTER_API_KEY: the page sends the state, the questions and the rules, and this process asks.
//!
//! What reads the command line's inputs or spends the key (`/start` and `/run`) needs this run's
//! token, a random one in the address's `#` fragment, which the page sends back in `X-Jev`. So
//! another user of the machine, who can reach the port but wasn't given the address, can't use
//! them. Another site's page can't either: it can't set that header without a preflight this server
//! never answers, and a request must name this server as its `Host` (and `Origin`, when it has one),
//! which a DNS rebinding doesn't.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use jev::print::{self, Format};
use jev::rules::Rules;
use jev::{Client, DecisionRequest};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

use super::{parse_state, read, Args};

const PAGE: &str = include_str!("explore.html");

/// The examples the page offers, as the files in `examples/rules`: a name, what it shows, a state,
/// the questions and the rules.
const EXAMPLES: [(&str, &str, &str, &str, &str); 3] = [
    (
        "wear",
        "What to wear for the weather: items decided at a threshold",
        "16°C, the air feels sticky, and a light drizzle has started.",
        include_str!("../../examples/rules/weather.json"),
        include_str!("../../examples/rules/wear.toml"),
    ),
    (
        "irrigation",
        "How long to water: a crisp output from fuzzy sets",
        "A fairly normal week: two moderate showers, and the soil is damp but drying at the surface.",
        include_str!("../../examples/rules/rain.json"),
        include_str!("../../examples/rules/irrigation.toml"),
    ),
    (
        "triage",
        "Routing a support ticket: a Choice, hedges and weights, with an output",
        "Help! My payouts have been failing for 3 days and nobody answers my emails. We are losing customers.",
        include_str!("../../examples/rules/triage.json"),
        include_str!("../../examples/rules/triage.toml"),
    ),
];

/// The most a request may be: its head, and its body.
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 4 * 1024 * 1024;
/// How long a request may take to arrive, and how many connections are served at once: one that
/// sends half a request and waits holds no more than a slot, and not for long.
const READ_TIME: Duration = Duration::from_secs(10);
const CONNECTIONS: usize = 32;

/// What every request needs: where to send the questions, and how.
struct Server {
    key: String,
    url: String,
    timeout: Duration,
    /// `127.0.0.1:PORT` and `localhost:PORT`, the hosts a request may name.
    hosts: [String; 2],
    /// This run's token, which `/start` and `/run` need in `X-Jev`.
    token: String,
    /// What the page starts with: the command line's state, questions and rules, and its model.
    start: Value,
}

/// Serves the page until the process is stopped.
pub async fn serve(args: &Args) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", args.port)).await.with_context(|| format!("listening on 127.0.0.1:{}", args.port))?;
    let port = listener.local_addr()?.port();
    let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default().trim().to_owned();
    if key.is_empty() && args.url == jev::DECISIONS_URL {
        eprintln!("jev: OPENROUTER_API_KEY is not set, so the page can draw rules but not ask");
    }
    let server = Arc::new(Server {
        start: start(args, !key.is_empty() || args.url != jev::DECISIONS_URL)?,
        key,
        url: args.url.clone(),
        timeout: Duration::from_secs_f64(args.timeout),
        hosts: [format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        token: token(),
    });

    let address = format!("http://127.0.0.1:{port}/#{}", server.token);
    eprintln!("jev: exploring at {address} (Ctrl-C to stop)");
    if !args.no_open {
        open(&address);
    }
    let slots = Arc::new(Semaphore::new(CONNECTIONS));
    loop {
        let (stream, _) = listener.accept().await?;
        // With every slot taken, the connection is closed rather than queued.
        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else { continue };
        let server = Arc::clone(&server);
        tokio::spawn(async move {
            // A connection that breaks off has no one left to tell.
            let _ = connection(stream, &server).await;
            drop(slot);
        });
    }
}

/// A random token for this run: 128 bits from two SipHash keys the standard library draws from the
/// system's random source, which is what it has without a crate for it.
fn token() -> String {
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let half = || {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

/// Whether `given` is `token`, taking as long whichever byte differs.
fn same_token(given: &str, token: &str) -> bool {
    given.len() == token.len() && given.bytes().zip(token.bytes()).fold(0, |differ, (a, b)| differ | (a ^ b)) == 0
}

/// The page's starting fields: what the command line gave, if anything; and whether it can ask.
fn start(args: &Args, can_ask: bool) -> anyhow::Result<Value> {
    // Standard input can be read once, so only one of the three may come from it.
    let stdin = |path: &Option<PathBuf>| path.as_deref() == Some(Path::new("-"));
    if [stdin(&args.state_file), stdin(&args.questions), stdin(&args.rules)].into_iter().filter(|from| *from).count() > 1 {
        anyhow::bail!("only one of the state, --questions and --rules can come from standard input");
    }
    let file = |path: &Option<PathBuf>| path.as_deref().map(read).transpose();
    let state = match (&args.state, &args.state_file) {
        (Some(text), _) => Some(text.clone()),
        (None, Some(path)) => Some(read(path)?),
        (None, None) => None,
    };
    let examples: Vec<Value> = EXAMPLES
        .iter()
        .map(|(name, about, state, questions, rules)| json!({"name": name, "about": about, "state": state, "questions": questions, "rules": rules}))
        .collect();
    Ok(json!({
        "state": state,
        "stateJson": args.state_json,
        "questions": file(&args.questions)?,
        "rules": file(&args.rules)?,
        "model": args.model,
        "canAsk": can_ask,
        "examples": examples,
    }))
}

/// Opens `address` in the default browser, or says to. The opener is waited for on a thread of its
/// own: one that stays with the browser it opened would otherwise keep the page from being served.
fn open(address: &str) {
    let address = address.to_owned();
    std::thread::spawn(move || {
        let status = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(&address).status()
        } else if cfg!(windows) {
            std::process::Command::new("cmd").args(["/C", "start", "", &address]).status()
        } else {
            std::process::Command::new("xdg-open").arg(&address).status()
        };
        if !status.is_ok_and(|status| status.success()) {
            eprintln!("jev: open {address} in a browser");
        }
    });
}

/// One request and its response; every connection is closed after one.
async fn connection(mut stream: TcpStream, server: &Server) -> std::io::Result<()> {
    // A request that takes longer than this to arrive is dropped, unanswered.
    let Ok(read) = tokio::time::timeout(READ_TIME, request(&mut stream)).await else { return Ok(()) };
    let (status, kind, body) = match read? {
        Ok(request) => respond(&request, server).await,
        Err(why) => (400, "text/plain; charset=utf-8", why.to_owned().into_bytes()),
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

/// A request as far as this server reads one.
#[derive(Debug, PartialEq)]
struct Request {
    method: String,
    path: String,
    host: Option<String>,
    origin: Option<String>,
    /// The `X-Jev` header: the token, from this server's page.
    jev: Option<String>,
    body: Vec<u8>,
}

/// Reads one request: its head, and a body of `Content-Length` bytes. The outer error is the
/// connection's; the inner one, a request this server won't read.
async fn request(stream: &mut TcpStream) -> std::io::Result<Result<Request, &'static str>> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let end = loop {
        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break at;
        }
        if bytes.len() > MAX_HEAD {
            return Ok(Err("the request's head is too long"));
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(Err("the request ended early"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let (mut request, length) = match head(&bytes[..end]) {
        Ok(read) => read,
        Err(why) => return Ok(Err(why)),
    };
    if length > MAX_BODY {
        return Ok(Err("the request's body is too long"));
    }
    request.body = bytes.split_off(end + 4);
    while request.body.len() < length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(Err("the request ended early"));
        }
        request.body.extend_from_slice(&chunk[..read]);
    }
    request.body.truncate(length);
    Ok(Ok(request))
}

/// The request line and the headers this server looks at, and how long the body is.
fn head(bytes: &[u8]) -> Result<(Request, usize), &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the request's head isn't UTF-8")?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(path)) = (first.next(), first.next()) else { return Err("no request line") };
    let mut request = Request { method: method.to_owned(), path: path.to_owned(), host: None, origin: None, jev: None, body: Vec::new() };
    let mut length = 0;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => request.host = Some(value.to_owned()),
            "origin" => request.origin = Some(value.to_owned()),
            "x-jev" => request.jev = Some(value.to_owned()),
            "content-length" => length = value.parse().map_err(|_| "Content-Length isn't a number")?,
            "transfer-encoding" => return Err("a chunked body isn't read here"),
            _ => {}
        }
    }
    Ok((request, length))
}

/// The status, type and body for `request`.
async fn respond(request: &Request, server: &Server) -> (u16, &'static str, Vec<u8>) {
    // Another name for this address (a rebinding) or another site's page is not the page served here.
    let ours = |host: &str| server.hosts.iter().any(|ours| ours == host);
    if !request.host.as_deref().is_some_and(ours) {
        return (403, "text/plain; charset=utf-8", b"not this server's host".to_vec());
    }
    if let Some(origin) = &request.origin {
        if !origin.strip_prefix("http://").is_some_and(ours) {
            return (403, "text/plain; charset=utf-8", b"not this server's page".to_vec());
        }
    }
    let path = request.path.split('?').next().unwrap_or_default();
    let with_token = request.jev.as_deref().is_some_and(|given| same_token(given, &server.token));
    match (request.method.as_str(), path) {
        // The page itself holds nothing of this run's: it asks for that with the token.
        ("GET", "/") => (200, "text/html; charset=utf-8", PAGE.as_bytes().to_vec()),
        ("GET", "/start") if with_token => (200, "application/json", server.start.to_string().into_bytes()),
        ("POST", "/run") if with_token => {
            let (status, body) = match serde_json::from_slice::<Run>(&request.body) {
                Ok(run) => match run.answer(server).await {
                    Ok(body) => (200, body),
                    Err(error) => (400, json!({"error": format!("{error:#}")})),
                },
                Err(error) => (400, json!({"error": format!("the request isn't what the page sends: {error}")})),
            };
            (status, "application/json", body.to_string().into_bytes())
        }
        ("GET", "/start") | ("POST", "/run") => {
            (403, "text/plain; charset=utf-8", b"no token: open the address jev --explore printed, with its #".to_vec())
        }
        (_, "/" | "/start" | "/run") => (405, "text/plain; charset=utf-8", b"method not allowed".to_vec()),
        _ => (404, "text/plain; charset=utf-8", b"not found".to_vec()),
    }
}

/// What the page asks for: the fields, and whether to ask Jev or only draw the rules.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    #[serde(default)]
    state: String,
    #[serde(default)]
    state_json: bool,
    questions: String,
    #[serde(default)]
    rules: String,
    model: String,
    /// Draw the rules' structure, and ask nothing.
    #[serde(default)]
    draw: bool,
}

impl Run {
    /// What the command prints for these fields, each part as its own string: the request, the
    /// answers, and with rules the outcome, the terminal's tree and the SVG.
    async fn answer(self, server: &Server) -> anyhow::Result<Value> {
        let questions = jev::spec::questions_file(&self.questions).map_err(|error| anyhow::anyhow!("questions: {error}"))?;
        if questions.is_empty() {
            anyhow::bail!("no questions: the questions are a JSON object of ids to questions");
        }
        // The form can give two questions one id, which the request's map would make one.
        for (at, (id, _)) in questions.iter().enumerate() {
            if questions[..at].iter().any(|(earlier, _)| earlier == id) {
                anyhow::bail!("questions: question `{id}` is asked twice");
            }
        }
        let rules = match self.rules.trim() {
            "" => None,
            text => Some(
                Rules::parse(text, questions.iter().map(|(id, question)| (id.as_str(), question)))
                    .map_err(|error| anyhow::anyhow!("rules: {error}"))?,
            ),
        };
        if self.draw {
            let rules = rules.context("no rules to draw")?;
            return Ok(json!({"graph": rules.graph_text(None)?, "svg": rules.graph_svg(None)?}));
        }

        let ids: Vec<String> = questions.iter().map(|(id, _)| id.clone()).collect();
        let state = parse_state(self.state, self.state_json)?;
        let request = DecisionRequest { model: self.model.trim().to_owned(), state, questions: questions.into_iter().collect() };
        if server.key.is_empty() && server.url == jev::DECISIONS_URL {
            anyhow::bail!("OPENROUTER_API_KEY is not set: set it where you start `jev --explore`");
        }
        let reply = Client::new(&server.key).with_url(&server.url).with_timeout(server.timeout).send(&request).await?;
        // `json` is what `--json` prints and `dryRun` what `--dry-run` does, so a saved file is the
        // command's own; `ids` are the questions in the order asked, answered or not.
        let mut body = json!({
            "request": request,
            "dryRun": serde_json::to_string_pretty(&request)? + "\n",
            "reply": reply,
            "ids": ids,
            "answers": print::render(Format::Text, &reply, &ids, 100)?,
            "json": print::render(Format::Json, &reply, &ids, 100)?,
        });
        if let Some(rules) = rules {
            let outcome = rules.evaluate(&reply)?;
            body["outcome"] = print::render_outcome(Format::Text, &reply, &outcome, 100)?.into();
            body["json"] = print::render_outcome(Format::Json, &reply, &outcome, 100)?.into();
            body["decision"] = serde_json::to_value(&outcome)?;
            body["graph"] = rules.graph_text(Some(&reply))?.into();
            body["svg"] = rules.graph_svg(Some(&reply))?.into();
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_request_head() {
        let (request, length) =
            head(b"POST /run HTTP/1.1\r\nHost: 127.0.0.1:7000\r\nContent-Length: 12\r\nX-Jev: 1\r\norigin: http://127.0.0.1:7000").unwrap();
        assert_eq!((request.method.as_str(), request.path.as_str(), length), ("POST", "/run", 12));
        assert_eq!(request.host.as_deref(), Some("127.0.0.1:7000"));
        assert_eq!(request.origin.as_deref(), Some("http://127.0.0.1:7000"));
        assert_eq!(request.jev.as_deref(), Some("1"));
        assert!(head(b"POST /run HTTP/1.1\r\nTransfer-Encoding: chunked").is_err());
        assert!(head(b"POST /run HTTP/1.1\r\nContent-Length: many").is_err());
    }

    fn server() -> Server {
        Server {
            key: String::new(),
            url: jev::DECISIONS_URL.to_owned(),
            timeout: Duration::from_secs(1),
            hosts: ["127.0.0.1:7000".to_owned(), "localhost:7000".to_owned()],
            token: "secret".to_owned(),
            start: json!({}),
        }
    }

    fn post(host: &str, origin: Option<&str>, jev: Option<&str>, body: Value) -> Request {
        Request {
            method: "POST".to_owned(),
            path: "/run".to_owned(),
            host: Some(host.to_owned()),
            origin: origin.map(str::to_owned),
            jev: jev.map(str::to_owned),
            body: body.to_string().into_bytes(),
        }
    }

    #[tokio::test]
    async fn answers_only_its_own_page() {
        let draw = json!({"questions": "{\"n\": {\"type\": \"noul\", \"instructions\": \"?\"}}", "rules": "[terms]\nn = \"n\"\n[[rule]]\nif = \"n\"\nthen = \"x\"\n", "model": "m", "draw": true});
        let status = |request: Request| async move { respond(&request, &server()).await.0 };
        let ours = Some("secret");
        assert_eq!(status(post("127.0.0.1:7000", Some("http://127.0.0.1:7000"), ours, draw.clone())).await, 200);
        assert_eq!(status(post("localhost:7000", None, ours, draw.clone())).await, 200);
        // Without this run's token (another site's form, or another user of the machine), a rebound
        // name or another origin, it is refused.
        assert_eq!(status(post("127.0.0.1:7000", None, None, draw.clone())).await, 403);
        assert_eq!(status(post("127.0.0.1:7000", None, Some("guess"), draw.clone())).await, 403);
        assert_eq!(status(post("127.0.0.1:7000", None, Some("secreT"), draw.clone())).await, 403);
        assert_eq!(status(post("evil.example:7000", None, ours, draw.clone())).await, 403);
        assert_eq!(status(post("127.0.0.1:7000", Some("http://evil.example"), ours, draw)).await, 403);
        // The command line's inputs need the token too; the page itself doesn't.
        let get = |path: &str, jev: Option<&str>| Request {
            method: "GET".to_owned(),
            path: path.to_owned(),
            host: Some("127.0.0.1:7000".to_owned()),
            origin: None,
            jev: jev.map(str::to_owned),
            body: Vec::new(),
        };
        assert_eq!(status(get("/start", None)).await, 403);
        assert_eq!(status(get("/start", ours)).await, 200);
        assert_eq!(status(get("/", None)).await, 200);
    }

    #[test]
    fn makes_a_new_token_each_run() {
        let (one, two) = (token(), token());
        assert_eq!(one.len(), 32);
        assert!(one.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(one, two);
        assert!(same_token(&one, &one) && !same_token(&one, &two) && !same_token(&one, &one[..31]));
    }

    #[test]
    fn reads_standard_input_for_one_input_only() {
        use clap::{CommandFactory, FromArgMatches};
        let args = |argv: &[&str]| {
            Args::from_arg_matches(&Args::command().try_get_matches_from(std::iter::once("jev").chain(argv.iter().copied())).unwrap())
                .unwrap()
        };
        let error = start(&args(&["--explore", "-q", "-", "-r", "-"]), true).unwrap_err();
        assert!(error.to_string().contains("only one of"), "{error}");
        assert!(start(&args(&["--explore", "-f", "-", "-q", "-"]), true).is_err());
    }

    #[tokio::test]
    async fn draws_the_structure_without_a_key() {
        let run = Run {
            state: String::new(),
            state_json: false,
            questions: include_str!("../../examples/rules/weather.json").to_owned(),
            rules: include_str!("../../examples/rules/wear.toml").to_owned(),
            model: "m".to_owned(),
            draw: true,
        };
        let body = run.answer(&server()).await.unwrap();
        assert!(body["svg"].as_str().unwrap().starts_with("<svg"));
        assert!(body["graph"].as_str().unwrap().contains("umbrella"));
    }

    #[tokio::test]
    async fn says_what_is_wrong_before_asking() {
        let run = |questions: &str, rules: &str| Run {
            state: "state".to_owned(),
            state_json: false,
            questions: questions.to_owned(),
            rules: rules.to_owned(),
            model: "m".to_owned(),
            draw: false,
        };
        let error = |run: Run| async move { format!("{:#}", run.answer(&server()).await.unwrap_err()) };
        assert!(error(run("not json", "")).await.starts_with("questions:"));
        let noul = r#"{"n": {"type": "noul", "instructions": "?"}}"#;
        assert!(error(run(noul, "[[rule]]\nif = \"m\"\nthen = \"x\"\n")).await.starts_with("rules:"));
        assert!(error(run(noul, "")).await.contains("OPENROUTER_API_KEY"));
        let twice = r#"{"n": {"type": "noul", "instructions": "?"}, "n": {"type": "noul", "instructions": "Again?"}}"#;
        assert!(error(run(twice, "")).await.contains("asked twice"));
    }
}
