//! Retry transport/setup failures before any response stream has been consumed.

use reqwest::{header::RETRY_AFTER, RequestBuilder, Response};
use std::time::Duration;

fn retryable(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}

pub(crate) async fn send(mut request: RequestBuilder) -> Result<Response, reqwest::Error> {
    for attempt in 0..=2 {
        let repeat = request.try_clone();
        match request.send().await {
            Ok(response) => {
                if attempt == 2 || !retryable(response.status()) || repeat.is_none() {
                    return Ok(response);
                }
                let after = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|secs| *secs <= 120)
                    .map(Duration::from_secs);
                drop(response);
                tokio::time::sleep(after.unwrap_or_else(|| backoff(attempt))).await;
            }
            Err(error) => {
                if attempt == 2 || repeat.is_none() || !(error.is_connect() || error.is_timeout()) {
                    return Err(error);
                }
                tokio::time::sleep(backoff(attempt)).await;
            }
        }
        request = repeat.expect("clone checked above");
    }
    unreachable!("retry loop always returns on final attempt")
}

fn backoff(attempt: usize) -> Duration {
    let base = 500_u64 << attempt;
    let jitter = rand::random::<u64>() % (base / 4 + 1);
    Duration::from_millis(base + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn retries_only_transient_statuses() {
        for code in [408, 409, 429, 500, 503] {
            assert!(retryable(reqwest::StatusCode::from_u16(code).unwrap()));
        }
        for code in [400, 401, 403, 404] {
            assert!(!retryable(reqwest::StatusCode::from_u16(code).unwrap()));
        }
    }

    #[tokio::test]
    async fn transient_http_status_retries_before_body_consumption() {
        use axum::{routing::post, Router};
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let router = Router::new().route(
            "/",
            post(move || {
                let count = count.clone();
                async move {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        reqwest::StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        reqwest::StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let response = send(reqwest::Client::new().post(format!("http://127.0.0.1:{port}/")))
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.abort();
    }
}
