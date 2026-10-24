//! Parsing curl commands into requests (Bruno `parse-curl.js` behaviour, plus notices).

use plover_core::curl::{CurlError, CurlNotice, parse_curl};
use plover_core::request::{
    Auth, Body, HttpRequest, KeyValue, Method, MultipartField, MultipartValue,
};

fn parse(text: &str) -> (HttpRequest, Vec<CurlNotice>) {
    let parsed = parse_curl(text).unwrap_or_else(|e| panic!("parse failed: {e}"));
    (parsed.request, parsed.notices)
}

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue::new(name, value)
}

fn headers(req: &HttpRequest) -> Vec<(String, String)> {
    req.headers
        .iter()
        .map(|h| (h.name.clone(), h.value.clone()))
        .collect()
}

fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn get_with_query_fills_query() {
    let (req, notices) = parse("curl 'https://example.test/a?x=1&y=2'");
    assert_eq!(req.method, Method::Get);
    assert_eq!(req.url, "https://example.test/a?x=1&y=2");
    assert_eq!(req.query, vec![kv("x", "1"), kv("y", "2")]);
    assert!(req.headers.is_empty());
    assert_eq!(req.body, Body::None);
    assert_eq!(req.auth, Auth::None);
    assert!(notices.is_empty());
}

#[test]
fn query_is_not_decoded() {
    let (req, _) = parse("curl 'https://example.test/a?q=a%20b&flag'");
    assert_eq!(req.query, vec![kv("q", "a%20b"), kv("flag", "")]);
}

#[test]
fn data_defaults_to_post_form_urlencoded() {
    let (req, notices) = parse("curl https://example.test/f -d 'a=1&b=2'");
    assert_eq!(req.method, Method::Post);
    assert_eq!(
        req.body,
        Body::FormUrlEncoded(vec![kv("a", "1"), kv("b", "2")])
    );
    assert!(req.headers.is_empty());
    assert!(notices.is_empty());
}

#[test]
fn form_data_is_decoded_and_joined() {
    let (req, _) =
        parse("curl https://example.test/f -d 'a=x%20y+z' -d b=2 --data-urlencode 'c=1 & 2'");
    assert_eq!(
        req.body,
        Body::FormUrlEncoded(vec![kv("a", "x y z"), kv("b", "2"), kv("c", "1 & 2")])
    );
}

#[test]
fn json_content_type_picks_json() {
    let (req, notices) = parse(
        "curl https://example.test/j -H 'Content-Type: application/json' --data-raw '{\"a\":1}'",
    );
    assert_eq!(req.method, Method::Post);
    assert_eq!(req.body, Body::Json("{\"a\":1}".to_owned()));
    assert_eq!(headers(&req), h(&[("Content-Type", "application/json")]));
    assert!(notices.is_empty());
}

#[test]
fn json_flag_sets_headers() {
    let (req, _) = parse("curl https://example.test/j --json '{\"a\":1}'");
    assert_eq!(req.method, Method::Post);
    assert_eq!(req.body, Body::Json("{\"a\":1}".to_owned()));
    assert_eq!(
        headers(&req),
        h(&[
            ("Content-Type", "application/json"),
            ("Accept", "application/json")
        ])
    );
}

#[test]
fn json_looking_data_without_content_type_is_json() {
    let (req, _) = parse("curl https://example.test/j -d '{\"a\":1}'");
    assert_eq!(req.body, Body::Json("{\"a\":1}".to_owned()));
}

#[test]
fn xml_and_text_modes() {
    let (xml, _) =
        parse("curl https://example.test/x -H 'content-type: application/xml' -d '<a/>'");
    assert_eq!(xml.body, Body::Xml("<a/>".to_owned()));
    let (text, _) = parse("curl https://example.test/x -H 'Content-Type: text/plain' -d 'hello'");
    assert_eq!(text.body, Body::Text("hello".to_owned()));
    let (nd, _) =
        parse("curl https://example.test/x -H 'Content-Type: application/x-ndjson' -d '{\"a\":1}'");
    assert_eq!(nd.body, Body::Text("{\"a\":1}".to_owned()));
}

#[test]
fn get_flag_moves_data_to_query() {
    let (req, _) = parse("curl -G https://example.test/s -d a=1 -d b=2");
    assert_eq!(req.method, Method::Get);
    assert_eq!(req.url, "https://example.test/s?a=1&b=2");
    assert_eq!(req.query, vec![kv("a", "1"), kv("b", "2")]);
    assert_eq!(req.body, Body::None);
}

#[test]
fn form_fields_and_files() {
    let (req, notices) = parse(
        "curl https://example.test/u -F name=x -F 'file=@/tmp/a.png;type=image/png' -F 'quoted=\"a;b\"'",
    );
    assert_eq!(req.method, Method::Post);
    assert_eq!(
        req.body,
        Body::Multipart(vec![
            MultipartField {
                name: "name".into(),
                value: MultipartValue::Text("x".into()),
                enabled: true,
                content_type: None,
            },
            MultipartField {
                name: "file".into(),
                value: MultipartValue::Files(vec!["/tmp/a.png".into()]),
                enabled: true,
                content_type: Some("image/png".into()),
            },
            MultipartField {
                name: "quoted".into(),
                value: MultipartValue::Text("a;b".into()),
                enabled: true,
                content_type: None,
            },
        ])
    );
    assert!(notices.is_empty());
}

#[test]
fn user_flag_is_basic_auth() {
    let (req, _) = parse("curl -u alice:s3cr3t https://example.test/");
    assert_eq!(
        req.auth,
        Auth::Basic {
            username: "alice".into(),
            password: "s3cr3t".into()
        }
    );
    let (colon, _) = parse("curl -u 'bob:a:b' https://example.test/");
    assert_eq!(
        colon.auth,
        Auth::Basic {
            username: "bob".into(),
            password: "a:b".into()
        }
    );
}

#[test]
fn bearer_header_stays_header() {
    let (req, _) = parse("curl https://example.test/ -H 'Authorization: Bearer t'");
    assert_eq!(headers(&req), h(&[("Authorization", "Bearer t")]));
    assert_eq!(req.auth, Auth::None);
}

#[test]
fn head_user_agent_cookie() {
    let (req, notices) = parse("curl -I -A ua -b 'a=1' -b 'b=2' https://example.test/");
    assert_eq!(req.method, Method::Head);
    assert_eq!(
        headers(&req),
        h(&[("User-Agent", "ua"), ("Cookie", "a=1; b=2")])
    );
    assert!(notices.is_empty());
}

#[test]
fn explicit_method_and_attached_values() {
    let (req, _) = parse("curl -XPATCH -H'x: 1' https://example.test/ -d a=1");
    assert_eq!(req.method, Method::Patch);
    assert_eq!(headers(&req), h(&[("x", "1")]));
    let (custom, _) = parse("curl --request purge https://example.test/");
    assert_eq!(custom.method, Method::Custom("PURGE".into()));
}

#[test]
fn compressed_is_silent() {
    let (req, notices) = parse("curl --compressed -sSL https://example.test/");
    assert!(notices.is_empty());
    assert!(req.headers.is_empty());
}

#[test]
fn insecure_gives_exact_notice() {
    let (req, notices) = parse("curl -k https://example.test/");
    assert_eq!(req.url, "https://example.test/");
    assert_eq!(
        notices,
        vec![CurlNotice {
            option: "-k".into(),
            reason: "`-k` not applied, TLS checking is a global setting".into(),
        }]
    );
    let (_, long) = parse("curl --insecure https://example.test/");
    assert_eq!(long[0].option, "--insecure");
    assert_eq!(
        long[0].reason,
        "`-k` not applied, TLS checking is a global setting"
    );
}

#[test]
fn unknown_and_unsupported_flags_are_notices() {
    let (req, notices) = parse(
        "curl --resolve a:443:1.2.3.4 -x http://p:3128 --cert c.pem --digest --data-binary @f.bin -T f --frobnicate https://example.test/a",
    );
    assert_eq!(req.url, "https://example.test/a");
    let options: Vec<&str> = notices.iter().map(|n| n.option.as_str()).collect();
    assert_eq!(
        options,
        vec![
            "--resolve a:443:1.2.3.4",
            "-x http://p:3128",
            "--cert c.pem",
            "--digest",
            "--data-binary @f.bin",
            "-T f",
            "--frobnicate",
        ]
    );
    assert!(notices.iter().all(|n| !n.reason.is_empty()));
    assert_eq!(req.body, Body::None);
}

#[test]
fn digest_user_is_not_sent_as_basic() {
    let (req, notices) = parse("curl --digest -u a:b https://example.test/");
    assert_eq!(req.auth, Auth::None);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].reason.contains("digest"));
}

#[test]
fn ansi_c_quotes_and_continuations() {
    let (req, _) = parse(
        "curl 'https://example.test/' \\\n  -H $'x-a: a\\nb' \\\r\n  -H 'x-b: it'\\''s' --data-raw $'{\"q\":\"a\\'b\"}'",
    );
    assert_eq!(headers(&req), h(&[("x-a", "a\nb"), ("x-b", "it's")]));
    assert_eq!(req.body, Body::Json("{\"q\":\"a'b\"}".to_owned()));
}

#[test]
fn double_quotes_and_backslashes() {
    let (req, _) = parse(r#"curl "https://example.test/" -H "x: \"q\" \$HOME \n""#);
    assert_eq!(headers(&req), h(&[("x", "\"q\" $HOME \\n")]));
}

#[test]
fn url_flag_and_extra_urls() {
    let (req, notices) = parse("curl --url https://example.test/a https://example.test/b");
    assert_eq!(req.url, "https://example.test/a");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].option, "https://example.test/b");
}

#[test]
fn piped_shell_text_is_a_notice() {
    let (req, notices) = parse("curl https://example.test/a | jq .");
    assert_eq!(req.url, "https://example.test/a");
    assert_eq!(notices.len(), 1);
    assert!(notices[0].option.starts_with('|'));
}

#[test]
fn template_urls_are_kept() {
    let (req, _) = parse("curl '{{baseUrl}}/users?id={{id}}'");
    assert_eq!(req.url, "{{baseUrl}}/users?id={{id}}");
    assert_eq!(req.query, vec![kv("id", "{{id}}")]);
}

#[test]
fn missing_url_is_error() {
    assert!(matches!(
        parse_curl("curl -H 'a: b'"),
        Err(CurlError::MissingUrl)
    ));
}

#[test]
fn not_curl_is_error() {
    assert!(matches!(parse_curl("wget x"), Err(CurlError::NotCurl)));
    assert!(matches!(parse_curl("   "), Err(CurlError::NotCurl)));
}

#[test]
fn unterminated_quote_is_syntax_error() {
    assert!(matches!(
        parse_curl("curl 'https://example.test/"),
        Err(CurlError::Syntax(_))
    ));
}

#[test]
fn chrome_devtools_copy_parses_cleanly() {
    let text = r#"curl 'https://example.test/api/search?page=2' \
  -H 'accept: application/json, text/plain, */*' \
  -H 'accept-language: en-US,en;q=0.9,vi;q=0.8' \
  -H 'cache-control: no-cache' \
  -H 'content-type: application/json' \
  -b 'sid=abc123; theme=dark' \
  -H 'origin: https://example.test' \
  -H 'pragma: no-cache' \
  -H 'priority: u=1, i' \
  -H 'referer: https://example.test/search' \
  -H 'sec-ch-ua: "Chromium";v="1", "Not-A.Brand";v="99"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'sec-ch-ua-platform: "macOS"' \
  -H 'sec-fetch-dest: empty' \
  -H 'sec-fetch-mode: cors' \
  -H 'sec-fetch-site: same-origin' \
  -H 'user-agent: Mozilla/5.0 (Macintosh) Chrome/1.0 Safari/537.36' \
  -H 'x-request-id: r-1' \
  --data-raw $'{"q":"a\'b"}' \
  --compressed"#;
    let (req, notices) = parse(text);
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(req.method, Method::Post);
    assert_eq!(req.url, "https://example.test/api/search?page=2");
    assert_eq!(req.query, vec![kv("page", "2")]);
    let names: Vec<&str> = req.headers.iter().map(|h| h.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "accept",
            "accept-language",
            "cache-control",
            "content-type",
            "Cookie",
            "origin",
            "pragma",
            "priority",
            "referer",
            "sec-ch-ua",
            "sec-ch-ua-mobile",
            "sec-ch-ua-platform",
            "sec-fetch-dest",
            "sec-fetch-mode",
            "sec-fetch-site",
            "user-agent",
            "x-request-id",
        ]
    );
    assert_eq!(
        req.headers[9].value,
        r#""Chromium";v="1", "Not-A.Brand";v="99""#
    );
    assert_eq!(req.body, Body::Json(r#"{"q":"a'b"}"#.to_owned()));
}

#[test]
fn cmd_style_curl_never_yields_caret_url() {
    for text in [
        "curl ^\"https://example.test/a^\" ^\n -H ^\"x: y^\"",
        "curl \"https://example.test/a\" ^\n  -H \"x: y\"",
    ] {
        match parse_curl(text) {
            Err(_) => {}
            Ok(parsed) => assert!(!parsed.request.url.contains('^'), "{}", parsed.request.url),
        }
    }
}
