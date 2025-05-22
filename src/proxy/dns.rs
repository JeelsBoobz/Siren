use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::Client;
use futures_util::future::join_all;

pub async fn doh(req_wireformat: &[u8]) -> Result<Vec<u8>> {
    let servers = vec![
        "https://1.1.1.1/dns-query",
        "https://8.8.8.8/dns-query",
    ];

    let client = Client::new();

    let headers = {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/dns-message"));
        h.insert(ACCEPT, HeaderValue::from_static("application/dns-message"));
        h
    };

    let body = req_wireformat.to_vec();

    let futures = servers.into_iter().map(|url| {
        let client = &client;
        let headers = headers.clone();
        let body = body.clone();
        async move {
            client
                .post(url)
                .headers(headers)
                .body(body)
                .send()
                .await?
                .bytes()
                .await
        }
    });

    let results = join_all(futures).await;

    let mut merged = Vec::new();
    for res in results {
        if let Ok(bytes) = res {
            merged.extend(bytes);
        }
    }

    Ok(merged)
}
