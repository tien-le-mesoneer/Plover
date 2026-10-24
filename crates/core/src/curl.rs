//! curl commands to and from requests.
//!
//! Parsing follows the Bruno app's paste handler (`bruno-app/src/utils/curl/parse-curl.js`
//! and `index.js`) for which flags fill which part of the request, with two differences:
//! an option Plover cannot represent is reported as a [`CurlNotice`] instead of being
//! skipped, and the shell words are split by a POSIX tokenizer here instead of
//! `shell-quote`.

use crate::request::{Auth, Body, HttpRequest, KeyValue, Method, MultipartField, MultipartValue};

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCurl {
    pub request: HttpRequest,
    pub notices: Vec<CurlNotice>,
}

/// An option of the pasted command that is not part of the request Plover fills in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlNotice {
    /// The option as written, with its value (`--proxy http://p:3128`).
    pub option: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CurlError {
    #[error("not a curl command")]
    NotCurl,
    #[error("the curl command has no URL")]
    MissingUrl,
    #[error("{0}")]
    Syntax(String),
}

/// The exact text the UI shows for `-k`/`--insecure`.
pub const INSECURE_NOTICE: &str = "`-k` not applied, TLS checking is a global setting";

/// Parses a bash-style curl command (as Chrome's "Copy as cURL (bash)" writes it).
pub fn parse_curl(text: &str) -> Result<ParsedCurl, CurlError> {
    if is_cmd_style(text) {
        return Err(CurlError::Syntax(
            "Windows cmd syntax (^) is not supported; copy the command as cURL (bash)".into(),
        ));
    }
    let tokens = tokenize(text)?;
    let mut notices = Vec::new();
    let mut words = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Word(w) => words.push(w.clone()),
            Token::Op(op) => {
                let rest: Vec<String> = tokens[i..]
                    .iter()
                    .map(|t| match t {
                        Token::Word(w) | Token::Op(w) => w.clone(),
                    })
                    .collect();
                notices.push(CurlNotice {
                    option: rest.join(" "),
                    reason: format!("shell text after `{op}` is not part of the request"),
                });
                break;
            }
        }
    }
    match words.first() {
        Some(first) if is_curl_program(first) => {}
        _ => return Err(CurlError::NotCurl),
    }
    let mut builder = Builder {
        notices,
        ..Builder::default()
    };
    builder.read_args(&words[1..])?;
    builder.finish()
}

/// Splits a URL's query string into name/value pairs without decoding, as Bruno does
/// (`parseQueryParams(query, { decode: false })`).
pub fn query_from_url(url: &str) -> Vec<KeyValue> {
    let Some((_, rest)) = url.split_once('?') else {
        return Vec::new();
    };
    let query = rest.split('#').next().unwrap_or_default();
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => KeyValue::new(name, value),
            None => KeyValue::new(pair, ""),
        })
        .collect()
}

/// The body mode Bruno picks for raw data from its content type (`utils/curl/index.js`).
/// `None` content type gives form-urlencoded, or JSON when the data is a JSON object or
/// array (Plover's call: Bruno would make one form field out of the whole JSON text).
pub(crate) fn body_from_text(content_type: Option<&str>, text: &str) -> Body {
    let Some(ct) = content_type.map(str::to_ascii_lowercase) else {
        let trimmed = text.trim_start();
        if (trimmed.starts_with('{') || trimmed.starts_with('['))
            && serde_json::from_str::<serde_json::Value>(text).is_ok()
        {
            return Body::Json(text.to_owned());
        }
        return Body::FormUrlEncoded(decode_form(text));
    };
    if ct.contains("application/x-ndjson") || ct.contains("application/ndjson") {
        Body::Text(text.to_owned())
    } else if ct.contains("application/json") || ct.contains("+json") {
        Body::Json(text.to_owned())
    } else if ct.contains("xml") {
        Body::Xml(text.to_owned())
    } else if ct.contains("application/x-www-form-urlencoded") {
        Body::FormUrlEncoded(decode_form(text))
    } else {
        Body::Text(text.to_owned())
    }
}

/// Splits `a=1&b=x%20y` into decoded fields (`+` is a space), keeping their order.
pub(crate) fn decode_form(text: &str) -> Vec<KeyValue> {
    text.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => KeyValue::new(form_decode(name), form_decode(value)),
            None => KeyValue::new(form_decode(pair), ""),
        })
        .collect()
}

fn form_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match (bytes.get(i + 1), bytes.get(i + 2)) {
                (Some(&a), Some(&b)) if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => {
                    out.push(hex(a) * 16 + hex(b));
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    }
}

/// curl's `--data-urlencode` escaping (`curl_easy_escape`): everything but unreserved.
fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_curl_program(word: &str) -> bool {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    name.eq_ignore_ascii_case("curl") || name.eq_ignore_ascii_case("curl.exe")
}

/// Windows `cmd` copies quote with `^"` and continue lines with a trailing `^`.
fn is_cmd_style(text: &str) -> bool {
    text.contains("^\"")
        || text
            .lines()
            .any(|line| line.trim_end().ends_with('^') && !line.trim_end().ends_with("\\^"))
}

// ---- shell tokenizer

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    /// An unquoted shell control operator (`|`, `;`, `&&`, `>`, `<`).
    Op(String),
}

fn tokenize(text: &str) -> Result<Vec<Token>, CurlError> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    macro_rules! end_word {
        () => {
            if in_word {
                tokens.push(Token::Word(std::mem::take(&mut word)));
                in_word = false;
            }
        };
    }
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => end_word!(),
            '\\' => match chars.next() {
                Some('\n') => {}
                Some('\r') if chars.peek() == Some(&'\n') => {
                    chars.next();
                }
                Some(next) => {
                    word.push(next);
                    in_word = true;
                }
                None => {
                    word.push('\\');
                    in_word = true;
                }
            },
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => word.push(ch),
                        None => return Err(CurlError::Syntax("unterminated `'` quote".into())),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(ch @ ('"' | '\\' | '$' | '`')) => word.push(ch),
                            Some('\n') => {}
                            Some(ch) => {
                                word.push('\\');
                                word.push(ch);
                            }
                            None => {
                                return Err(CurlError::Syntax("unterminated `\"` quote".into()));
                            }
                        },
                        Some(ch) => word.push(ch),
                        None => return Err(CurlError::Syntax("unterminated `\"` quote".into())),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                chars.next();
                in_word = true;
                read_ansi_c(&mut chars, &mut word)?;
            }
            '#' if !in_word => {
                for ch in chars.by_ref() {
                    if ch == '\n' {
                        break;
                    }
                }
            }
            '|' | ';' | '<' | '>' => {
                end_word!();
                let mut op = c.to_string();
                if c == '|' && chars.peek() == Some(&'|') {
                    op.push(chars.next().unwrap_or('|'));
                }
                tokens.push(Token::Op(op));
            }
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                end_word!();
                tokens.push(Token::Op("&&".into()));
            }
            other => {
                word.push(other);
                in_word = true;
            }
        }
    }
    if in_word {
        tokens.push(Token::Word(word));
    }
    Ok(tokens)
}

/// Bash `$'…'` quoting: backslash escapes are decoded, up to the closing `'`.
fn read_ansi_c(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
) -> Result<(), CurlError> {
    let unterminated = || CurlError::Syntax("unterminated `$'` quote".into());
    loop {
        let c = chars.next().ok_or_else(unterminated)?;
        match c {
            '\'' => return Ok(()),
            '\\' => {
                let e = chars.next().ok_or_else(unterminated)?;
                match e {
                    'n' => word.push('\n'),
                    'r' => word.push('\r'),
                    't' => word.push('\t'),
                    'v' => word.push('\u{0b}'),
                    'f' => word.push('\u{0c}'),
                    'a' => word.push('\u{07}'),
                    'b' => word.push('\u{08}'),
                    'e' | 'E' => word.push('\u{1b}'),
                    '\\' | '\'' | '"' | '?' => word.push(e),
                    'x' => push_code(chars, word, 16, 2, "\\x"),
                    'u' => push_code(chars, word, 16, 4, "\\u"),
                    'U' => push_code(chars, word, 16, 8, "\\U"),
                    '0'..='7' => {
                        let mut digits = e.to_string();
                        while digits.len() < 3 && chars.peek().is_some_and(|d| d.is_digit(8)) {
                            digits.push(chars.next().unwrap_or('0'));
                        }
                        let code = u32::from_str_radix(&digits, 8).unwrap_or(0);
                        word.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => {
                        word.push('\\');
                        word.push(other);
                    }
                }
            }
            other => word.push(other),
        }
    }
}

fn push_code(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    word: &mut String,
    radix: u32,
    max: usize,
    literal: &str,
) {
    let mut digits = String::new();
    while digits.len() < max && chars.peek().is_some_and(|d| d.is_digit(radix)) {
        digits.push(chars.next().unwrap_or('0'));
    }
    match u32::from_str_radix(&digits, radix)
        .ok()
        .and_then(char::from_u32)
    {
        Some(ch) if !digits.is_empty() => word.push(ch),
        _ => {
            word.push_str(literal);
            word.push_str(&digits);
        }
    }
}

// ---- flags

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opt {
    Request,
    Header,
    UserAgent,
    Referer,
    Cookie,
    Data,
    DataRaw,
    DataBinary,
    DataUrlencode,
    Json,
    Form,
    FormString,
    User,
    Url,
    OAuth2Bearer,
    Get,
    Head,
    Insecure,
    Digest,
    /// Changes nothing about the request (output, progress, redirects, compression).
    Ignored,
    /// Changes how curl sends the request in a way Plover cannot copy.
    Unsupported(&'static str),
}

struct Flag {
    short: Option<char>,
    long: &'static str,
    takes_value: bool,
    opt: Opt,
}

const fn flag(short: Option<char>, long: &'static str, takes_value: bool, opt: Opt) -> Flag {
    Flag {
        short,
        long,
        takes_value,
        opt,
    }
}

const PROXY: &str = "proxies are not supported per request";
const TLS: &str = "client certificates and custom CA settings are not supported per request";
const DNS: &str = "custom name resolution is not supported";
const TIMEOUT: &str = "timeouts are not imported; set one in the request settings";
const HTTP_VERSION: &str = "Plover picks the HTTP version itself";
const AUTH_MODE: &str = "Plover cannot send this auth mode";
const NOT_SUPPORTED: &str = "this curl option is not supported";

const FLAGS: &[Flag] = &[
    flag(Some('X'), "--request", true, Opt::Request),
    flag(Some('H'), "--header", true, Opt::Header),
    flag(Some('A'), "--user-agent", true, Opt::UserAgent),
    flag(Some('e'), "--referer", true, Opt::Referer),
    flag(Some('b'), "--cookie", true, Opt::Cookie),
    flag(Some('d'), "--data", true, Opt::Data),
    flag(None, "--data-ascii", true, Opt::Data),
    flag(None, "--data-raw", true, Opt::DataRaw),
    flag(None, "--data-binary", true, Opt::DataBinary),
    flag(None, "--data-urlencode", true, Opt::DataUrlencode),
    flag(None, "--json", true, Opt::Json),
    flag(Some('F'), "--form", true, Opt::Form),
    flag(None, "--form-string", true, Opt::FormString),
    flag(Some('u'), "--user", true, Opt::User),
    flag(None, "--url", true, Opt::Url),
    flag(None, "--oauth2-bearer", true, Opt::OAuth2Bearer),
    flag(Some('G'), "--get", false, Opt::Get),
    flag(Some('I'), "--head", false, Opt::Head),
    flag(Some('k'), "--insecure", false, Opt::Insecure),
    flag(None, "--digest", false, Opt::Digest),
    flag(None, "--ntlm", false, Opt::Digest),
    flag(None, "--negotiate", false, Opt::Digest),
    flag(None, "--anyauth", false, Opt::Digest),
    flag(None, "--basic", false, Opt::Ignored),
    flag(None, "--compressed", false, Opt::Ignored),
    flag(Some('s'), "--silent", false, Opt::Ignored),
    flag(Some('S'), "--show-error", false, Opt::Ignored),
    flag(Some('v'), "--verbose", false, Opt::Ignored),
    flag(Some('i'), "--include", false, Opt::Ignored),
    flag(Some('L'), "--location", false, Opt::Ignored),
    flag(Some('f'), "--fail", false, Opt::Ignored),
    flag(None, "--fail-with-body", false, Opt::Ignored),
    flag(Some('#'), "--progress-bar", false, Opt::Ignored),
    flag(None, "--no-progress-meter", false, Opt::Ignored),
    flag(Some('N'), "--no-buffer", false, Opt::Ignored),
    flag(Some('g'), "--globoff", false, Opt::Ignored),
    flag(Some('O'), "--remote-name", false, Opt::Ignored),
    flag(Some('J'), "--remote-header-name", false, Opt::Ignored),
    flag(None, "--tcp-nodelay", false, Opt::Ignored),
    flag(Some('o'), "--output", true, Opt::Ignored),
    flag(Some('w'), "--write-out", true, Opt::Ignored),
    flag(Some('c'), "--cookie-jar", true, Opt::Ignored),
    flag(Some('D'), "--dump-header", true, Opt::Ignored),
    flag(None, "--stderr", true, Opt::Ignored),
    flag(None, "--trace", true, Opt::Ignored),
    flag(None, "--trace-ascii", true, Opt::Ignored),
    flag(Some('x'), "--proxy", true, Opt::Unsupported(PROXY)),
    flag(Some('U'), "--proxy-user", true, Opt::Unsupported(PROXY)),
    flag(None, "--noproxy", true, Opt::Unsupported(PROXY)),
    flag(None, "--preproxy", true, Opt::Unsupported(PROXY)),
    flag(Some('p'), "--proxytunnel", false, Opt::Unsupported(PROXY)),
    flag(Some('E'), "--cert", true, Opt::Unsupported(TLS)),
    flag(None, "--key", true, Opt::Unsupported(TLS)),
    flag(None, "--cacert", true, Opt::Unsupported(TLS)),
    flag(None, "--capath", true, Opt::Unsupported(TLS)),
    flag(None, "--cert-type", true, Opt::Unsupported(TLS)),
    flag(None, "--key-type", true, Opt::Unsupported(TLS)),
    flag(None, "--pass", true, Opt::Unsupported(TLS)),
    flag(None, "--ciphers", true, Opt::Unsupported(TLS)),
    flag(None, "--tlsv1", false, Opt::Unsupported(TLS)),
    flag(None, "--tlsv1.0", false, Opt::Unsupported(TLS)),
    flag(None, "--tlsv1.1", false, Opt::Unsupported(TLS)),
    flag(None, "--tlsv1.2", false, Opt::Unsupported(TLS)),
    flag(None, "--tlsv1.3", false, Opt::Unsupported(TLS)),
    flag(None, "--resolve", true, Opt::Unsupported(DNS)),
    flag(None, "--connect-to", true, Opt::Unsupported(DNS)),
    flag(None, "--dns-servers", true, Opt::Unsupported(DNS)),
    flag(Some('4'), "--ipv4", false, Opt::Unsupported(DNS)),
    flag(Some('6'), "--ipv6", false, Opt::Unsupported(DNS)),
    flag(Some('m'), "--max-time", true, Opt::Unsupported(TIMEOUT)),
    flag(None, "--connect-timeout", true, Opt::Unsupported(TIMEOUT)),
    flag(
        Some('0'),
        "--http1.0",
        false,
        Opt::Unsupported(HTTP_VERSION),
    ),
    flag(None, "--http1.1", false, Opt::Unsupported(HTTP_VERSION)),
    flag(None, "--http2", false, Opt::Unsupported(HTTP_VERSION)),
    flag(
        None,
        "--http2-prior-knowledge",
        false,
        Opt::Unsupported(HTTP_VERSION),
    ),
    flag(None, "--http3", false, Opt::Unsupported(HTTP_VERSION)),
    flag(None, "--aws-sigv4", true, Opt::Unsupported(AUTH_MODE)),
    flag(
        Some('T'),
        "--upload-file",
        true,
        Opt::Unsupported("uploading a file with `-T` is not supported"),
    ),
    flag(
        Some('K'),
        "--config",
        true,
        Opt::Unsupported("curl config files are not read"),
    ),
    flag(Some('r'), "--range", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(
        Some('z'),
        "--time-cond",
        true,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(None, "--retry", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(None, "--retry-delay", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(
        None,
        "--retry-max-time",
        true,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(None, "--max-redirs", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(
        None,
        "--max-filesize",
        true,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(None, "--limit-rate", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(None, "--interface", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(None, "--local-port", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(None, "--unix-socket", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(
        None,
        "--abstract-unix-socket",
        true,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(
        None,
        "--request-target",
        true,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(None, "--variable", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(Some('Q'), "--quote", true, Opt::Unsupported(NOT_SUPPORTED)),
    flag(
        Some('t'),
        "--telnet-option",
        true,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(None, "--path-as-is", false, Opt::Unsupported(NOT_SUPPORTED)),
    flag(
        None,
        "--location-trusted",
        false,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
    flag(
        Some('Z'),
        "--parallel",
        false,
        Opt::Unsupported(NOT_SUPPORTED),
    ),
];

fn long_flag(name: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.long == name)
}

fn short_flag(c: char) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| f.short == Some(c))
}

#[derive(Default)]
struct Builder {
    notices: Vec<CurlNotice>,
    method: Option<String>,
    head: bool,
    get: bool,
    url: Option<String>,
    headers: Vec<KeyValue>,
    data: Vec<String>,
    form: Vec<MultipartField>,
    user: Option<(String, String)>,
    /// Index of the notice for `--digest` and the like.
    digest: Option<usize>,
}

impl Builder {
    fn notice(&mut self, option: String, reason: impl Into<String>) {
        self.notices.push(CurlNotice {
            option,
            reason: reason.into(),
        });
    }

    fn read_args(&mut self, args: &[String]) -> Result<(), CurlError> {
        let mut i = 0;
        let mut positional_only = false;
        while i < args.len() {
            let arg = &args[i];
            i += 1;
            if positional_only || arg == "-" || !arg.starts_with('-') {
                self.positional(arg);
                continue;
            }
            if arg == "--" {
                positional_only = true;
                continue;
            }
            if arg.starts_with("--") {
                let Some(flag) = long_flag(arg) else {
                    self.notice(arg.clone(), "unknown curl option");
                    continue;
                };
                let value = if flag.takes_value {
                    let v = args
                        .get(i)
                        .ok_or_else(|| CurlError::Syntax(format!("`{arg}` needs a value")))?;
                    i += 1;
                    Some(v.clone())
                } else {
                    None
                };
                self.apply(flag, arg, value);
                continue;
            }
            // A cluster of short options: `-sSL`, `-XPOST`, `-H'x: 1'`.
            let cluster: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < cluster.len() {
                let c = cluster[j];
                j += 1;
                let written = format!("-{c}");
                let Some(flag) = short_flag(c) else {
                    self.notice(written, "unknown curl option");
                    continue;
                };
                if flag.takes_value {
                    let attached: String = cluster[j..].iter().collect();
                    let value = if attached.is_empty() {
                        let v = args.get(i).ok_or_else(|| {
                            CurlError::Syntax(format!("`{written}` needs a value"))
                        })?;
                        i += 1;
                        v.clone()
                    } else {
                        attached
                    };
                    self.apply(flag, &written, Some(value));
                    break;
                }
                self.apply(flag, &written, None);
            }
        }
        Ok(())
    }

    fn positional(&mut self, arg: &str) {
        if self.url.is_none() {
            self.url = Some(arg.to_owned());
        } else {
            self.notice(arg.to_owned(), "only the first URL is imported");
        }
    }

    fn apply(&mut self, flag: &Flag, written: &str, value: Option<String>) {
        let value = value.unwrap_or_default();
        let shown = || {
            if flag.takes_value {
                format!("{written} {value}")
            } else {
                written.to_owned()
            }
        };
        match flag.opt {
            Opt::Request => self.method = Some(value.to_ascii_uppercase()),
            Opt::Header => self.header(&value),
            Opt::UserAgent => self.set_header("User-Agent", &value),
            Opt::Referer => self.set_header("Referer", &value),
            Opt::Cookie => {
                if value.contains('=') {
                    match self
                        .headers
                        .iter_mut()
                        .find(|h| h.name.eq_ignore_ascii_case("cookie"))
                    {
                        Some(existing) => {
                            existing.value = format!("{}; {value}", existing.value);
                        }
                        None => self.headers.push(KeyValue::new("Cookie", value)),
                    }
                } else {
                    let option = shown();
                    self.notice(option, "cookie files are not read");
                }
            }
            Opt::Data => {
                if value.starts_with('@') {
                    let option = shown();
                    self.notice(option, "reading the body from a file is not supported");
                } else {
                    self.data.push(value);
                }
            }
            Opt::DataRaw => self.data.push(value),
            Opt::DataBinary => {
                if value.starts_with('@') {
                    let option = shown();
                    self.notice(option, "reading the body from a file is not supported");
                } else {
                    self.data.push(value);
                }
            }
            Opt::DataUrlencode => self.data_urlencode(&value, shown()),
            Opt::Json => {
                if value.starts_with('@') {
                    let option = shown();
                    self.notice(option, "reading the body from a file is not supported");
                    return;
                }
                if !self.has_header("content-type") {
                    self.headers
                        .push(KeyValue::new("Content-Type", "application/json"));
                }
                if !self.has_header("accept") {
                    self.headers
                        .push(KeyValue::new("Accept", "application/json"));
                }
                self.data.push(value);
            }
            Opt::Form => self.form_field(&value, shown(), false),
            Opt::FormString => self.form_field(&value, shown(), true),
            Opt::User => {
                let (user, pass) = value.split_once(':').unwrap_or((&value, ""));
                self.user = Some((user.to_owned(), pass.to_owned()));
            }
            Opt::Url => {
                if self.url.is_none() {
                    self.url = Some(value);
                } else {
                    self.notice(value, "only the first URL is imported");
                }
            }
            Opt::OAuth2Bearer => self.set_header("Authorization", &format!("Bearer {value}")),
            Opt::Get => self.get = true,
            Opt::Head => self.head = true,
            Opt::Insecure => self.notice(written.to_owned(), INSECURE_NOTICE),
            Opt::Digest => {
                self.digest = Some(self.notices.len());
                self.notice(
                    written.to_owned(),
                    format!("Plover cannot send `{written}` auth"),
                );
            }
            Opt::Ignored => {}
            Opt::Unsupported(reason) => {
                let option = shown();
                self.notice(option, reason);
            }
        }
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    }

    fn set_header(&mut self, name: &str, value: &str) {
        self.headers.retain(|h| !h.name.eq_ignore_ascii_case(name));
        self.headers.push(KeyValue::new(name, value));
    }

    fn header(&mut self, value: &str) {
        match value.split_once(':') {
            // `-H 'Name:'` tells curl not to send that header: nothing to add.
            Some((name, rest)) if !name.trim().is_empty() => {
                let rest = rest.trim_start();
                if !rest.is_empty() {
                    self.headers.push(KeyValue::new(name.trim(), rest));
                }
            }
            // `-H 'Name;'` sends the header with an empty value.
            None if value.ends_with(';') && value.len() > 1 => {
                self.headers
                    .push(KeyValue::new(value[..value.len() - 1].trim(), ""));
            }
            _ => self.notice(format!("-H {value}"), "not a `Name: value` header"),
        }
    }

    fn data_urlencode(&mut self, value: &str, shown: String) {
        // curl: `content`, `=content`, `name=content`, `@file`, `name@file`.
        if let Some((name, content)) = value.split_once('=') {
            if name.contains('@') {
                self.notice(shown, "reading the body from a file is not supported");
                return;
            }
            self.data.push(if name.is_empty() {
                url_encode(content)
            } else {
                format!("{name}={}", url_encode(content))
            });
        } else if value.contains('@') {
            self.notice(shown, "reading the body from a file is not supported");
        } else {
            self.data.push(url_encode(value));
        }
    }

    fn form_field(&mut self, value: &str, shown: String, literal: bool) {
        let Some((name, raw)) = value.split_once('=') else {
            self.notice(shown, "not a `name=value` form field");
            return;
        };
        let unquote = |v: &str| -> String {
            v.strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v)
                .to_owned()
        };
        let field = |value: MultipartValue, content_type: Option<String>| MultipartField {
            name: name.to_owned(),
            value,
            enabled: true,
            content_type,
        };
        if literal {
            self.form
                .push(field(MultipartValue::Text(raw.to_owned()), None));
        } else if let Some(file) = raw.strip_prefix('@') {
            let mut parts = file.split(';');
            let path = unquote(parts.next().unwrap_or_default());
            let content_type = parts
                .find_map(|p| p.trim().strip_prefix("type="))
                .map(str::to_owned);
            self.form
                .push(field(MultipartValue::Files(vec![path]), content_type));
        } else if raw.starts_with('<') {
            self.notice(shown, "reading a form field from a file is not supported");
        } else {
            self.form
                .push(field(MultipartValue::Text(unquote(raw)), None));
        }
    }

    fn finish(mut self) -> Result<ParsedCurl, CurlError> {
        let mut url = self.url.take().ok_or(CurlError::MissingUrl)?;
        if url.contains('^') && !url.contains("{{") {
            return Err(CurlError::MissingUrl);
        }
        let has_data = !self.data.is_empty();
        let mut body = Body::None;
        if !self.form.is_empty() {
            if has_data {
                self.notice(
                    "-d".into(),
                    "a command with both -F and -d is not valid; the -d data was left out",
                );
            }
            body = Body::Multipart(std::mem::take(&mut self.form));
        } else if has_data {
            let data = self.data.join("&");
            if self.get {
                url.push(if url.contains('?') { '&' } else { '?' });
                url.push_str(&data);
            } else {
                let content_type = self
                    .headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                    .map(|h| h.value.clone());
                body = body_from_text(content_type.as_deref(), &data);
            }
        }
        let sends_body = !matches!(body, Body::None);
        let method = match self.method.take() {
            Some(m) => {
                Method::from_block_name(&m.to_ascii_lowercase()).unwrap_or(Method::Custom(m))
            }
            None if sends_body => Method::Post,
            None if self.head => Method::Head,
            None => Method::Get,
        };
        let auth = match (self.user.take(), self.digest) {
            (Some(_), Some(index)) => {
                self.notices[index]
                    .reason
                    .push_str("; the -u credentials were not imported");
                Auth::None
            }
            (Some((username, password)), None) => Auth::Basic { username, password },
            (None, _) => Auth::None,
        };
        let mut request = HttpRequest::blank();
        request.method = method;
        request.query = query_from_url(&url);
        request.url = url;
        request.headers = self.headers;
        request.body = body;
        request.auth = auth;
        Ok(ParsedCurl {
            request,
            notices: self.notices,
        })
    }
}
