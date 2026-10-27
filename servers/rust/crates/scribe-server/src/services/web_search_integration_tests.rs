//! Local-only provider contract fixtures. No real API keys or network providers.
use super::*;
use crate::services::search_connections::{SearchConnection, SearchProvider};
use std::{io::{Read, Write}, net::TcpListener, thread, time::{Duration, Instant}};

fn mock(responses: Vec<(u16, Value)>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        let mut captured = Vec::new();
        for (status, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                    Err(_) => return captured,
                }
            };
            socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0u8; 4096];
                let n = socket.read(&mut buffer).unwrap_or(0);
                if n == 0 { break; }
                bytes.extend_from_slice(&buffer[..n]);
                assert!(bytes.len() < 16384, "Fixture received an unexpectedly large search query");
                let text = String::from_utf8_lossy(&bytes);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().unwrap())
                    }).unwrap_or(0);
                    if bytes.len() >= end + 4 + length { break; }
                }
            }
            captured.push(String::from_utf8(bytes).unwrap());
            let body = body.to_string();
            let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            // Early body-limit rejection may close the socket intentionally.
            let _ = socket.write_all(response.as_bytes());
        }
        captured
    });
    (format!("http://{address}/search"), worker)
}
fn connection(provider: SearchProvider, revision: i64, key: &str) -> SearchConnection {
    let endpoint = match provider {
        SearchProvider::Doubao => "https://open.feedcoopapi.com/search_api/web_search",
        SearchProvider::Deepseek => "https://api.deepseek.com/v1/tools/search",
        SearchProvider::Searxng => "https://search.example.com/search",
    };
    SearchConnection { provider, endpoint: endpoint.into(), api_key: key.into(), revision }
}
fn payload(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}
fn deepseek_result(label: &str) -> Value {
    json!({"results":[{"title":label,"url":"https://example.com/reference","snippet":"bounded snippet","summary":"bounded summary","content":"WHOLE_PAGE_MUST_NOT_BE_FORWARDED"}]})
}
fn doubao_result() -> Value {
    json!({"ResponseMetadata":{"RequestId":"fixture-request"},"Result":{"ResultCount":1,"WebResults":[{"Title":"Official result","Url":"https://example.com/reference","Summary":"DOUBAO_SAFE_SUMMARY","Snippet":"short snippet","Content":"WHOLE_PAGE_MUST_NOT_BE_FORWARDED","PublishTime":"2026-10-27T12:00:00+08:00"}]}})
}

#[tokio::test]
async fn doubao_wire_matches_user_example_and_uses_only_search_connection_key() {
    let (url, worker) = mock(vec![(200, doubao_result())]);
    let config = connection(SearchProvider::Doubao, 1, "fake-owned-doubao-search-key");
    let mut session = SearchSession::default();
    let result = session.search_connection_at(Some(&config), &json!({"query":" public academic topic "}), &url).await.unwrap();
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(request.starts_with("POST /search "));
    assert!(request.to_ascii_lowercase().contains("authorization: bearer fake-owned-doubao-search-key"));
    let body = payload(request);
    assert_eq!(body["Query"], "public academic topic");
    assert_eq!(body["SearchType"], "web");
    assert_eq!(body["Count"], 5);
    assert_eq!(body["Filter"]["NeedContent"], false);
    assert_eq!(body["Filter"]["NeedUrl"], true);
    assert_eq!(body["NeedSummary"], true);
    assert!(result.to_string().contains("DOUBAO_SAFE_SUMMARY"));
    assert!(!result.to_string().contains("WHOLE_PAGE_MUST_NOT_BE_FORWARDED"));
    assert!(!result.to_string().contains("fake-owned-doubao-search-key"));
}

#[tokio::test]
async fn deepseek_wire_uses_own_search_key_and_keeps_success_query_cache() {
    let (url, worker) = mock(vec![(200, deepseek_result("DeepSeek fixture"))]);
    let config = connection(SearchProvider::Deepseek, 2, "fake-owned-independent-search-key");
    let mut session = SearchSession::default();
    let first = session.search_connection_at(Some(&config), &json!({"query":" public query "}), &url).await.unwrap();
    let second = session.search_connection_at(Some(&config), &json!({"query":"public query"}), &url).await.unwrap();
    assert_eq!(first["cached"], false);
    assert_eq!(second["cached"], true);
    assert_eq!(session.attempts(), 1);
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].to_ascii_lowercase().contains("authorization: bearer fake-owned-independent-search-key"));
    assert_eq!(payload(&requests[0]), json!({"query":"public query","count":5,"need_content":false}));
}

#[tokio::test]
async fn provider_switch_cannot_reuse_prior_provider_query_cache_or_key() {
    let (url, worker) = mock(vec![(200, doubao_result()), (200, deepseek_result("New provider result"))]);
    let a = connection(SearchProvider::Doubao, 10, "fake-provider-a-key");
    let b = connection(SearchProvider::Deepseek, 10, "fake-provider-b-key");
    let mut session = SearchSession::default();
    session.search_connection_at(Some(&a), &json!({"query":"same query"}), &url).await.unwrap();
    let result = session.search_connection_at(Some(&b), &json!({"query":"same query"}), &url).await.unwrap();
    assert_eq!(result["cached"], false);
    assert!(result.to_string().contains("New provider result"));
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("fake-provider-a-key"));
    assert!(!requests[1].contains("fake-provider-a-key"));
    assert!(requests[1].contains("fake-provider-b-key"));
}

#[tokio::test]
async fn http200_doubao_error_envelope_is_not_silently_empty_or_credential_echo() {
    let (url, worker) = mock(vec![(200, json!({"ResponseMetadata":{"Error":{"Code":"invalid_api_key","Message":"UPSTREAM_ERROR_MUST_NOT_ECHO_SECRET"}},"Result":null}))]);
    let config = connection(SearchProvider::Doubao, 1, "fake-fixture-key");
    let error = SearchSession::default().search_connection_at(Some(&config), &json!({"query":"public query"}), &url).await.unwrap_err();
    assert!(!error.contains("UPSTREAM_ERROR_MUST_NOT_ECHO_SECRET"));
    assert!(!error.contains("fake-fixture-key"));
    assert_eq!(worker.join().unwrap().len(), 1);
}

#[tokio::test]
async fn normalized_results_filter_internal_urls_duplicates_and_large_whole_pages() {
    let rows = json!({"ResponseMetadata":{},"Result":{"WebResults":[
        {"Title":"private","Url":"http://127.0.0.1/private","Summary":"no"},
        {"Title":"private","Url":"http://10.0.0.1/private","Summary":"no"},
        {"Title":"metadata","Url":"http://169.254.169.254/latest/meta-data","Summary":"no"},
        {"Title":"ipv6 private","Url":"http://[::1]/private","Summary":"no"},
        {"Title":"public","Url":"https://example.com/reference","Summary":"safe"},
        {"Title":"duplicate","Url":"https://example.com/reference","Summary":"same"}
    ]}});
    let (url, worker) = mock(vec![(200, rows)]);
    let config = connection(SearchProvider::Doubao, 1, "fake-fixture-key");
    let result = SearchSession::default().search_connection_at(Some(&config), &json!({"query":"public query"}), &url).await.unwrap();
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(result["results"][0]["url"], "https://example.com/reference");
    assert_eq!(worker.join().unwrap().len(), 1);
}

#[tokio::test]
async fn failed_requests_use_the_same_three_attempt_budget() {
    let (url, worker) = mock(vec![(401, json!({"error":"credential echo must not forward"})); 3]);
    let config = connection(SearchProvider::Deepseek, 1, "fake-fixture-key");
    let mut session = SearchSession::default();
    for _ in 0..4 {
        let error = session.search_connection_at(Some(&config), &json!({"query":"public query"}), &url).await.unwrap_err();
        assert!(!error.contains("credential echo"));
    }
    assert_eq!(session.attempts(), 3);
    assert_eq!(worker.join().unwrap().len(), 3);
}

#[tokio::test]
async fn doubao_rejects_101_characters_before_dispatch_without_silently_truncating() {
    let config = connection(SearchProvider::Doubao, 1, "fake-fixture-key");
    let mut session = SearchSession::default();
    let query = "界".repeat(101);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/search", listener.local_addr().unwrap());
    let result = session.search_connection_at(Some(&config), &json!({"query":query}), &endpoint).await;
    assert!(result.is_err());
    assert!(listener.accept().is_err(), "An over-limit query must not reach the HTTP provider");
    assert_eq!(session.attempts(), 1, "Rejected attempts remain part of the bounded request budget");
}

#[tokio::test]
async fn doubao_100_unicode_characters_and_deepseek_101_are_sent_unchanged() {
    let query = "界".repeat(100);
    let (url, worker) = mock(vec![(200, doubao_result())]);
    let config = connection(SearchProvider::Doubao, 1, "fake-fixture-key");
    SearchSession::default().search_connection_at(Some(&config), &json!({"query":query}), &url).await.unwrap();
    assert_eq!(payload(&worker.join().unwrap()[0])["Query"], query);
    let query = "x".repeat(101);
    let (url, worker) = mock(vec![(200, deepseek_result("Longer supported query"))]);
    let config = connection(SearchProvider::Deepseek, 1, "fake-fixture-key");
    SearchSession::default().search_connection_at(Some(&config), &json!({"query":query}), &url).await.unwrap();
    assert_eq!(payload(&worker.join().unwrap()[0])["query"], query);
}

#[tokio::test]
async fn searxng_posts_standard_form_without_implicit_chat_or_search_key() {
    let (url, worker) = mock(vec![(200, json!({"results":[{"title":"SearXNG reference","url":"https://example.com/docs","content":"Public result excerpt"}]}))]);
    let config = connection(SearchProvider::Searxng, 1, "");
    let mut session = SearchSession::default();
    let result = session.search_connection_at(Some(&config), &json!({"query":"中文 a&b"}), &url).await.unwrap();
    assert_eq!(result["provider"], "searxng_search");
    assert_eq!(result["results"][0]["snippet"], "Public result excerpt");
    let cached = session.search_connection_at(Some(&config), &json!({"query":"中文 a&b"}), &url).await.unwrap();
    assert_eq!(cached["cached"], true);
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(request.to_ascii_lowercase().contains("content-type: application/x-www-form-urlencoded"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
    let pairs = url::form_urlencoded::parse(request.split_once("\r\n\r\n").unwrap().1.as_bytes()).collect::<Vec<_>>();
    assert_eq!(pairs, vec![("q".into(), "中文 a&b".into()), ("format".into(), "json".into())]);
}
#[tokio::test]
async fn searxng_optional_gateway_key_changes_cache_and_can_be_removed() {
    let rows = json!({"results":[{"title":"reference","url":"https://example.com/docs","content":"excerpt"}]});
    let (url, worker) = mock(vec![(200, rows.clone()), (200, rows)]);
    let with_key = connection(SearchProvider::Searxng, 1, "fake-gateway-key");
    let without_key = connection(SearchProvider::Searxng, 2, "");
    let mut session = SearchSession::default();
    session.search_connection_at(Some(&with_key), &json!({"query":"same query"}), &url).await.unwrap();
    let result = session.search_connection_at(Some(&without_key), &json!({"query":"same query"}), &url).await.unwrap();
    assert_eq!(result["cached"], false);
    let requests = worker.join().unwrap();
    assert!(requests[0].to_ascii_lowercase().contains("authorization: bearer fake-gateway-key"));
    assert!(!requests[1].to_ascii_lowercase().contains("authorization:"));
    assert!(!result.to_string().contains("fake-gateway-key"));
}
#[tokio::test]
async fn searxng_403_explains_json_format_or_gateway_without_echoing_body() {
    let (url, worker) = mock(vec![(403, json!({"error":"PRIVATE_UPSTREAM_BODY"}))]);
    let config = connection(SearchProvider::Searxng, 1, "");
    let error = SearchSession::default().search_connection_at(Some(&config), &json!({"query":"query"}), &url).await.unwrap_err();
    assert!(error.contains("format=json"));
    assert!(!error.contains("PRIVATE_UPSTREAM_BODY"));
    assert_eq!(worker.join().unwrap().len(), 1);
}
#[test]
fn searxng_results_remain_bounded_and_filter_private_source_links() {
    let mut rows = vec![json!({"title":"private","url":"https://127.0.0.1/private","content":"no"})];
    rows.extend((0..8).map(|n| json!({"title":"t".repeat(500),"url":format!("https://example.com/{n}"),"content":"c".repeat(5000)})));
    let output = normalize_provider(json!({"results":rows}), "query", SearchProvider::Searxng).unwrap();
    assert_eq!(output["results"].as_array().unwrap().len(), 5);
    assert!(!output.to_string().contains("127.0.0.1"));
    assert!(output["results"][0]["summary"].as_str().unwrap().len() <= 800);
}

#[tokio::test]
async fn personal_budget_one_stops_new_calls_but_cache_does_not_spend_budget() {
    let (url, worker) = mock(vec![(200, deepseek_result("first"))]);
    let config = connection(SearchProvider::Deepseek, 1, "fake-key");
    let mut session = SearchSession::default(); session.set_max_calls(1);
    session.search_connection_at(Some(&config), &json!({"query":"first"}), &url).await.unwrap();
    assert_eq!(session.search_connection_at(Some(&config), &json!({"query":"first"}), &url).await.unwrap()["cached"], true);
    assert!(session.search_connection_at(Some(&config), &json!({"query":"second"}), &url).await.is_err());
    assert_eq!(session.attempts(), 1);
    assert_eq!(session.metadata()["configuredLimit"], 1);
    assert_eq!(session.metadata()["remainingAttempts"], 0);
    assert_eq!(worker.join().unwrap().len(), 1);
}
#[tokio::test]
async fn zero_budget_means_no_separate_search_limit_and_changes_take_effect() {
    let (url, worker) = mock(vec![(401, json!({"error":"static only"})); 5]);
    let config = connection(SearchProvider::Deepseek, 1, "fake-key");
    let mut session = SearchSession::default(); session.set_max_calls(0);
    for _ in 0..4 { assert!(session.search_connection_at(Some(&config), &json!({"query":"query"}), &url).await.is_err()); }
    assert_eq!(session.attempts(), 4);
    assert!(session.metadata()["remainingAttempts"].is_null());
    session.set_max_calls(1);
    assert!(session.search_connection_at(Some(&config), &json!({"query":"query"}), &url).await.unwrap_err().contains("你设置"));
    assert_eq!(session.attempts(), 4);
    session.set_max_calls(5);
    assert!(session.search_connection_at(Some(&config), &json!({"query":"query"}), &url).await.is_err());
    assert_eq!(session.attempts(), 5);
    assert_eq!(worker.join().unwrap().len(), 5);
}
