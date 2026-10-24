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

// ---- writing

mod writer {
    use super::*;
    use plover_core::curl::{CurlBody, CurlCommand, CurlPart, to_curl};
    use plover_core::request::RequestSettings;
    use plover_core::resolve::{
        PreparedBody, PreparedRequest, ResolvedBody, ResolvedPart, ResolvedRequest,
    };

    fn cmd(method: Method, url: &str, headers: &[(&str, &str)], body: CurlBody) -> CurlCommand {
        CurlCommand {
            method,
            url: url.to_owned(),
            headers: h(headers),
            basic_auth: None,
            body,
        }
    }

    #[test]
    fn writes_posix_shell_format() {
        let text = to_curl(&cmd(
            Method::Get,
            "https://example.test/a",
            &[("a", "1"), ("b", "2")],
            CurlBody::None,
        ));
        assert_eq!(
            text,
            "curl -X GET 'https://example.test/a' \\\n  -H 'a: 1' \\\n  -H 'b: 2'"
        );
    }

    #[test]
    fn single_quote_escaping() {
        let text = to_curl(&cmd(
            Method::Get,
            "https://example.test/",
            &[("x", "it's")],
            CurlBody::None,
        ));
        assert!(text.contains("-H 'x: it'\\''s'"), "{text}");
        let (req, _) = parse(&text);
        assert_eq!(req.headers[0].value, "it's");
    }

    #[test]
    fn form_uses_data_urlencode_per_field() {
        let text = to_curl(&cmd(
            Method::Post,
            "https://example.test/f",
            &[],
            CurlBody::Form(vec![("a".into(), "1".into()), ("b".into(), "x y&z".into())]),
        ));
        assert!(text.contains("  --data-urlencode 'a=1'"), "{text}");
        assert!(text.contains("  --data-urlencode 'b=x y&z'"), "{text}");
    }

    fn source(method: Method, headers: &[(&str, &str)], body: Body, auth: Auth) -> HttpRequest {
        let mut req = HttpRequest::blank();
        req.method = method;
        req.url = "https://example.test/r?q=1".into();
        req.query = vec![kv("q", "1")];
        req.headers = headers.iter().map(|(n, v)| kv(n, v)).collect();
        req.body = body;
        req.auth = auth;
        req
    }

    #[test]
    fn round_trip_each_body_mode() {
        let file = |name: &str, path: &str, ct: Option<&str>| MultipartField {
            name: name.into(),
            value: MultipartValue::Files(vec![path.into()]),
            enabled: true,
            content_type: ct.map(str::to_owned),
        };
        let text_field = |name: &str, value: &str| MultipartField {
            name: name.into(),
            value: MultipartValue::Text(value.into()),
            enabled: true,
            content_type: None,
        };
        let cases = vec![
            source(Method::Get, &[("x", "1")], Body::None, Auth::None),
            source(
                Method::Post,
                &[("Content-Type", "application/json")],
                Body::Json("{\"a\": \"it's\"}".into()),
                Auth::None,
            ),
            source(
                Method::Put,
                &[("Content-Type", "text/plain")],
                Body::Text("line 1\nline 2".into()),
                Auth::None,
            ),
            source(
                Method::Post,
                &[("Content-Type", "application/xml")],
                Body::Xml("<a b='c'/>".into()),
                Auth::None,
            ),
            source(
                Method::Post,
                &[],
                Body::FormUrlEncoded(vec![kv("a", "1"), kv("b", "x y&z=%"), kv("c d", "e")]),
                Auth::None,
            ),
            source(
                Method::Post,
                &[],
                Body::Multipart(vec![
                    text_field("name", "x"),
                    text_field("semi", "a;type=b"),
                    text_field("at", "@not-a-file"),
                    file("f", "/tmp/a.png", Some("image/png")),
                    file("g", "rel/b.txt", None),
                ]),
                Auth::None,
            ),
            source(
                Method::Delete,
                &[],
                Body::None,
                Auth::Basic {
                    username: "alice".into(),
                    password: "p:w'd".into(),
                },
            ),
        ];
        for src in cases {
            let text = to_curl(&CurlCommand::from(&src));
            let (back, notices) = parse(&text);
            assert!(notices.is_empty(), "{text}\n{notices:?}");
            assert_eq!(back.method, src.method, "{text}");
            assert_eq!(back.url, src.url, "{text}");
            assert_eq!(back.query, src.query, "{text}");
            assert_eq!(back.headers, src.headers, "{text}");
            assert_eq!(back.body, src.body, "{text}");
            assert_eq!(back.auth, src.auth, "{text}");
        }
    }

    #[test]
    fn request_adds_default_content_type_and_bearer() {
        let mut req = source(
            Method::Post,
            &[],
            Body::Text("hi".into()),
            Auth::Bearer {
                token: "{{token}}".into(),
            },
        );
        req.headers.push(KeyValue {
            name: "off".into(),
            value: "1".into(),
            enabled: false,
            local: false,
        });
        let cmd = CurlCommand::from(&req);
        assert_eq!(
            cmd.headers,
            h(&[
                ("Authorization", "Bearer {{token}}"),
                ("content-type", "text/plain")
            ])
        );
    }

    #[test]
    fn from_prepared_keeps_variables() {
        let prepared = PreparedRequest {
            method: Method::Post,
            url: "{{host}}/a".into(),
            headers: h(&[("x", "{{v}}")]),
            body: PreparedBody::Form(vec![("a".into(), "{{b}}".into())]),
            basic_auth: Some(("{{u}}".into(), "{{p}}".into())),
            path_params: Vec::new(),
            settings: RequestSettings::default(),
        };
        let cmd = CurlCommand::from(&prepared);
        assert_eq!(cmd.url, "{{host}}/a");
        assert_eq!(cmd.basic_auth, Some(("{{u}}".into(), "{{p}}".into())));
        assert_eq!(cmd.body, CurlBody::Form(vec![("a".into(), "{{b}}".into())]));
    }

    #[test]
    fn from_resolved_writes_bytes_and_files() {
        let resolved = ResolvedRequest {
            method: Method::Post,
            url: "http://h/a".into(),
            headers: h(&[("content-type", "application/json")]),
            body: ResolvedBody::Bytes(b"{\"a\":1}".to_vec()),
            settings: RequestSettings::default(),
        };
        assert_eq!(
            CurlCommand::from(&resolved).body,
            CurlBody::Raw("{\"a\":1}".into())
        );
        let multipart = ResolvedRequest {
            body: ResolvedBody::Multipart(vec![
                ResolvedPart::Text {
                    name: "a".into(),
                    value: "1".into(),
                    content_type: None,
                },
                ResolvedPart::File {
                    name: "f".into(),
                    path: "/root/x.bin".into(),
                    content_type: Some("application/octet-stream".into()),
                },
            ]),
            ..resolved
        };
        assert_eq!(
            CurlCommand::from(&multipart).body,
            CurlBody::Multipart(vec![
                CurlPart::Text {
                    name: "a".into(),
                    value: "1".into()
                },
                CurlPart::File {
                    name: "f".into(),
                    path: "/root/x.bin".into(),
                    content_type: Some("application/octet-stream".into())
                },
            ])
        );
    }
}
