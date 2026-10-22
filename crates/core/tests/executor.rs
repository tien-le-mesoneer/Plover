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
    r.body = ResolvedBody::Bytes {
        content_type: Some("application/json".into()),
        data: br#"{"a":1}"#.to_vec(),
    };
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
