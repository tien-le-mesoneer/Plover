use std::time::Duration;

use brs_core::executor::{ExecError, ExecOptions, Session};
use brs_core::request::{Method, RequestSettings};
use brs_core::resolve::{ResolvedBody, ResolvedPart, ResolvedRequest};
use wiremock::matchers::{body_string, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn req(method: Method, url: String) -> ResolvedRequest {
    ResolvedRequest {
        method,
        url,
        headers: Vec::new(),
        body: ResolvedBody::None,
        settings: RequestSettings::default(),
    }
}

fn session() -> Session {
    Session::new(ExecOptions::default()).unwrap()
}

#[tokio::test]
async fn sends_method_headers_and_json_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/items"))
        .and(header("x-a", "1"))
        .and(header("content-type", "application/json"))
        .and(body_string(r#"{"a":1}"#))
        .respond_with(ResponseTemplate::new(201).set_body_string("created"))
        .expect(1)
        .mount(&server)
        .await;
    let mut r = req(Method::Post, format!("{}/items", server.uri()));
    r.headers.push(("x-a".into(), "1".into()));
    r.headers
        .push(("content-type".into(), "application/json".into()));
    r.body = ResolvedBody::Bytes(br#"{"a":1}"#.to_vec());
    let res = session().send(&r).await.unwrap();
    assert_eq!(res.status, 201);
    assert_eq!(res.body, b"created");
    assert_eq!(res.size, 7);
}

#[tokio::test]
async fn form_and_multipart_bodies() {
    let server = MockServer::start().await;
    Mock::given(path("/form"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string("a=1&b=2"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/multi"))
        .and(body_string_contains("name=\"t\""))
        .and(body_string_contains("hello"))
        .and(body_string_contains("filename=\"up.txt\""))
        .and(body_string_contains("file-content"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("up.txt");
    std::fs::write(&file, "file-content").unwrap();

    let s = session();
    let mut r = req(Method::Post, format!("{}/form", server.uri()));
    r.body = ResolvedBody::Form(vec![("a".into(), "1".into()), ("b".into(), "2".into())]);
    assert_eq!(s.send(&r).await.unwrap().status, 200);
    let mut r = req(Method::Post, format!("{}/multi", server.uri()));
    r.body = ResolvedBody::Multipart(vec![
        ResolvedPart::Text {
            name: "t".into(),
            value: "hello".into(),
            content_type: None,
        },
        ResolvedPart::File {
            name: "f".into(),
            path: file,
            content_type: Some("text/plain".into()),
        },
    ]);
    assert_eq!(s.send(&r).await.unwrap().status, 200);
}

#[tokio::test]
async fn basic_and_bearer_auth_headers() {
    let server = MockServer::start().await;
    Mock::given(header("authorization", "Basic dTpw"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(header("authorization", "Bearer t"))
        .respond_with(ResponseTemplate::new(202))
        .expect(1)
        .mount(&server)
        .await;
    let s = session();
    let mut r = req(Method::Get, server.uri());
    r.headers = vec![("Authorization".into(), "Basic dTpw".into())];
    assert_eq!(s.send(&r).await.unwrap().status, 200);
    r.headers = vec![("Authorization".into(), "Bearer t".into())];
    assert_eq!(s.send(&r).await.unwrap().status, 202);
}

#[tokio::test]
async fn redirect_toggle() {
    let server = MockServer::start().await;
    Mock::given(path("/r"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/ok"))
        .mount(&server)
        .await;
    Mock::given(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let s = session();
    let mut r = req(Method::Get, format!("{}/r", server.uri()));
    assert_eq!(s.send(&r).await.unwrap().status, 200);
    r.settings.follow_redirects = Some(false);
    assert_eq!(s.send(&r).await.unwrap().status, 302);
    let no_follow = Session::new(ExecOptions {
        follow_redirects: false,
        ..ExecOptions::default()
    })
    .unwrap();
    r.settings.follow_redirects = None;
    assert_eq!(no_follow.send(&r).await.unwrap().status, 302);
}

#[tokio::test]
async fn cookies_persist_in_session() {
    let server = MockServer::start().await;
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200).insert_header("set-cookie", "sid=abc; Path=/"))
        .mount(&server)
        .await;
    Mock::given(path("/me"))
        .and(header("cookie", "sid=abc"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let s = session();
    s.send(&req(Method::Get, format!("{}/login", server.uri())))
        .await
        .unwrap();
    let me = req(Method::Get, format!("{}/me", server.uri()));
    assert_eq!(s.send(&me).await.unwrap().status, 200);
    assert_eq!(session().send(&me).await.unwrap().status, 401);
}

#[tokio::test]
async fn timeout_is_transport_error() {
    let server = MockServer::start().await;
    Mock::given(path("/slow"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(500)))
        .mount(&server)
        .await;
    let s = Session::new(ExecOptions {
        timeout: Some(Duration::from_millis(100)),
        ..ExecOptions::default()
    })
    .unwrap();
    let err = s
        .send(&req(Method::Get, format!("{}/slow", server.uri())))
        .await
        .unwrap_err();
    assert!(matches!(err, ExecError::Timeout), "{err}");
}

#[tokio::test]
async fn connection_refused_is_transport_error() {
    let err = session()
        .send(&req(Method::Get, "http://127.0.0.1:9".into()))
        .await
        .unwrap_err();
    assert!(matches!(err, ExecError::Transport(_)), "{err}");
}

async fn received_headers(server: &MockServer, name: &str) -> Vec<String> {
    let requests = server.received_requests().await.unwrap();
    let last = requests.last().unwrap();
    last.headers
        .get_all(name)
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect()
}

// run-single-request.js:555-575: one content type, carrying the form's boundary; a
// boundary the user already wrote is reused.
#[tokio::test]
async fn multipart_sends_one_content_type_with_boundary() {
    let server = MockServer::start().await;
    Mock::given(path("/m"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let s = session();
    let parts = vec![ResolvedPart::Text {
        name: "t".into(),
        value: "hello".into(),
        content_type: None,
    }];
    for (given, expected_prefix) in [
        ("multipart/mixed", "multipart/mixed; boundary="),
        ("multipart/form-data", "multipart/form-data; boundary="),
        (
            "multipart/form-data; boundary=abc",
            "multipart/form-data; boundary=abc",
        ),
    ] {
        let mut r = req(Method::Post, format!("{}/m", server.uri()));
        r.headers.push(("Content-Type".into(), given.into()));
        r.body = ResolvedBody::Multipart(parts.clone());
        s.send(&r).await.unwrap();
        let ct = received_headers(&server, "content-type").await;
        assert_eq!(ct.len(), 1, "{ct:?}");
        assert!(ct[0].starts_with(expected_prefix), "{ct:?}");
        let boundary = ct[0].split("boundary=").nth(1).unwrap();
        let body = String::from_utf8(
            server
                .received_requests()
                .await
                .unwrap()
                .last()
                .unwrap()
                .body
                .clone(),
        )
        .unwrap();
        assert!(body.starts_with(&format!("--{boundary}\r\n")), "{body}");
        assert!(body.contains("name=\"t\"\r\n\r\nhello\r\n"), "{body}");
        assert!(body.ends_with(&format!("--{boundary}--\r\n")), "{body}");
    }
}

// Bruno keeps a user-set content type on form bodies (prepare-request.js:430-432).
#[tokio::test]
async fn form_keeps_explicit_content_type() {
    let server = MockServer::start().await;
    Mock::given(path("/f"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let mut r = req(Method::Post, format!("{}/f", server.uri()));
    r.headers.push((
        "Content-Type".into(),
        "application/x-www-form-urlencoded; charset=utf-8".into(),
    ));
    r.body = ResolvedBody::Form(vec![("a b".into(), "1&2".into())]);
    session().send(&r).await.unwrap();
    assert_eq!(
        received_headers(&server, "content-type").await,
        ["application/x-www-form-urlencoded; charset=utf-8"]
    );
    let body = server
        .received_requests()
        .await
        .unwrap()
        .last()
        .unwrap()
        .body
        .clone();
    assert_eq!(body, b"a+b=1%262");
}

// axios-instance.js:166-169: past the redirect limit the 3xx itself is the response.
#[tokio::test]
async fn redirect_limit_returns_last_redirect() {
    let server = MockServer::start().await;
    for (from, to) in [("/r1", "/r2"), ("/r2", "/r3"), ("/r3", "/ok")] {
        Mock::given(path(from))
            .respond_with(ResponseTemplate::new(302).insert_header("location", to))
            .mount(&server)
            .await;
    }
    Mock::given(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let s = Session::new(ExecOptions {
        max_redirects: 2,
        ..ExecOptions::default()
    })
    .unwrap();
    let mut r = req(Method::Get, format!("{}/r1", server.uri()));
    let res = s.send(&r).await.unwrap();
    assert_eq!(res.status, 302);
    assert!(
        res.headers
            .iter()
            .any(|(k, v)| k == "location" && v == "/ok")
    );
    r.settings.max_redirects = Some(0);
    let res = s.send(&r).await.unwrap();
    assert!(
        res.headers
            .iter()
            .any(|(k, v)| k == "location" && v == "/r2")
    );
    r.settings.max_redirects = Some(3);
    assert_eq!(s.send(&r).await.unwrap().status, 200);
}

// axios-instance.js:101-111: default Accept (axios common default) and User-Agent;
// the user's own value (any case) replaces them. We identify as bru-rs.
#[tokio::test]
async fn default_accept_and_user_agent() {
    let server = MockServer::start().await;
    Mock::given(path("/d"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let s = session();
    let mut r = req(Method::Get, format!("{}/d", server.uri()));
    s.send(&r).await.unwrap();
    assert_eq!(
        received_headers(&server, "accept").await,
        ["application/json, text/plain, */*"]
    );
    assert_eq!(
        received_headers(&server, "user-agent").await,
        [format!("bru-rs/{}", env!("CARGO_PKG_VERSION"))]
    );
    r.headers.push(("accept".into(), "text/html".into()));
    r.headers.push(("USER-AGENT".into(), "mine".into()));
    s.send(&r).await.unwrap();
    assert_eq!(received_headers(&server, "accept").await, ["text/html"]);
    assert_eq!(received_headers(&server, "user-agent").await, ["mine"]);
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn brotli(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    brotli::BrotliCompress(
        &mut &data[..],
        &mut out,
        &brotli::enc::BrotliEncoderParams::default(),
    )
    .unwrap();
    out
}

fn encoded(body: Vec<u8>, encoding: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-encoding", encoding)
        .set_body_bytes(body)
}

// axios 1.18.0 lib/adapters/http.js:55 + :820-824: `Accept-Encoding: gzip, compress,
// deflate, br` is set unless the request already has one (any case).
#[tokio::test]
async fn default_accept_encoding() {
    let server = MockServer::start().await;
    Mock::given(path("/e"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    session()
        .send(&req(Method::Get, format!("{}/e", server.uri())))
        .await
        .unwrap();
    assert_eq!(
        received_headers(&server, "accept-encoding").await,
        ["gzip, compress, deflate, br"]
    );
}

// http.js:1031-1063 decodes gzip and br and drops `content-encoding`. The reported size
// is the decoded length, like Bruno's `Buffer.byteLength(response.dataBuffer)`
// (run-single-request.js:1004).
#[tokio::test]
async fn gzip_and_br_bodies_are_decoded() {
    let plain = b"hello hello hello hello".to_vec();
    let server = MockServer::start().await;
    Mock::given(path("/gz"))
        .respond_with(encoded(gzip(&plain), "gzip"))
        .mount(&server)
        .await;
    Mock::given(path("/br"))
        .respond_with(encoded(brotli(&plain), "br"))
        .mount(&server)
        .await;
    let s = session();
    for p in ["/gz", "/br"] {
        let res = s
            .send(&req(Method::Get, format!("{}{p}", server.uri())))
            .await
            .unwrap();
        assert_eq!(res.body, plain, "{p}");
        assert_eq!(res.size, plain.len(), "{p}");
        assert!(
            !res.headers.iter().any(|(k, _)| k == "content-encoding"),
            "{p}"
        );
    }
}

// A user Accept-Encoding is sent verbatim. reqwest still decodes encodings it supports
// (as axios does); an encoding it does not support (zstd) comes back as received.
#[tokio::test]
async fn user_accept_encoding_wins() {
    let plain = b"hello hello hello hello".to_vec();
    let server = MockServer::start().await;
    Mock::given(path("/gz"))
        .respond_with(encoded(gzip(&plain), "gzip"))
        .mount(&server)
        .await;
    Mock::given(path("/zstd"))
        .respond_with(encoded(b"raw-bytes".to_vec(), "zstd"))
        .mount(&server)
        .await;
    let s = session();
    let mut r = req(Method::Get, format!("{}/gz", server.uri()));
    r.headers
        .push(("ACCEPT-ENCODING".into(), "identity".into()));
    let res = s.send(&r).await.unwrap();
    assert_eq!(
        received_headers(&server, "accept-encoding").await,
        ["identity"]
    );
    assert_eq!(res.body, plain);
    r.url = format!("{}/zstd", server.uri());
    r.headers = vec![("Accept-Encoding".into(), "zstd".into())];
    let res = s.send(&r).await.unwrap();
    assert_eq!(received_headers(&server, "accept-encoding").await, ["zstd"]);
    assert_eq!(res.body, b"raw-bytes");
    assert!(
        res.headers
            .iter()
            .any(|(k, v)| k == "content-encoding" && v == "zstd")
    );
}

/// A localhost server that answers every connection with an endless chunked
/// body (optionally gzip-encoded zeros) and reports how many bytes it wrote.
async fn endless_server(gzip: bool) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let written = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = written.clone();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let head = if gzip {
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-encoding: gzip\r\ntransfer-encoding: chunked\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ntransfer-encoding: chunked\r\n\r\n"
                };
                if socket.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                let block = vec![b'a'; 64 * 1024];
                loop {
                    let chunk = if gzip {
                        encoder.write_all(&block).unwrap();
                        encoder.flush().unwrap();
                        std::mem::take(encoder.get_mut())
                    } else {
                        block.clone()
                    };
                    if chunk.is_empty() {
                        continue;
                    }
                    let framed = [
                        format!("{:x}\r\n", chunk.len()).into_bytes(),
                        chunk,
                        b"\r\n".to_vec(),
                    ]
                    .concat();
                    if socket.write_all(&framed).await.is_err() {
                        return;
                    }
                    counter.fetch_add(framed.len(), Ordering::Relaxed);
                }
            });
        }
    });
    (format!("http://{addr}/"), written)
}

#[tokio::test]
async fn body_cap_stops_reading_an_endless_stream() {
    const CAP: usize = 1024 * 1024;
    let (url, written) = endless_server(false).await;
    let session = Session::new(ExecOptions {
        max_body_bytes: Some(CAP),
        ..ExecOptions::default()
    })
    .unwrap();
    let res = tokio::time::timeout(
        Duration::from_secs(10),
        session.send(&req(Method::Get, url)),
    )
    .await
    .expect("a capped read must return promptly")
    .unwrap();
    assert_eq!(res.body.len(), CAP);
    assert!(res.truncated);
    assert!(res.size >= CAP, "size counts the bytes read");
    // The connection is dropped: the server stops being able to write soon after.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after_drop = written.load(std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let later = written.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        later - after_drop < 4 * 1024 * 1024,
        "server kept streaming: {after_drop} -> {later}"
    );
}

#[tokio::test]
async fn body_cap_applies_to_decoded_gzip_stream() {
    const CAP: usize = 2 * 1024 * 1024;
    let (url, _) = endless_server(true).await;
    let session = Session::new(ExecOptions {
        max_body_bytes: Some(CAP),
        ..ExecOptions::default()
    })
    .unwrap();
    let res = tokio::time::timeout(
        Duration::from_secs(10),
        session.send(&req(Method::Get, url)),
    )
    .await
    .expect("a capped read must return promptly")
    .unwrap();
    assert_eq!(res.body.len(), CAP);
    assert!(res.body.iter().all(|b| *b == b'a'), "body is decoded");
    assert!(res.truncated);
    assert!(
        !res.headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-encoding"))
    );
}

#[tokio::test]
async fn no_cap_reads_whole_body_and_is_not_truncated() {
    let server = MockServer::start().await;
    Mock::given(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![7u8; 3 * 1024 * 1024]))
        .mount(&server)
        .await;
    let res = session()
        .send(&req(Method::Get, format!("{}/big", server.uri())))
        .await
        .unwrap();
    assert_eq!(res.body.len(), 3 * 1024 * 1024);
    assert_eq!(res.size, 3 * 1024 * 1024);
    assert!(!res.truncated);
}
