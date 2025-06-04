use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::Client;
use tokio::time::{timeout, Duration};

pub async fn doh(req_wireformat: &[u8]) -> Result<Vec<u8>> {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/dns-message"),
    );
    headers.insert(ACCEPT, HeaderValue::from_static("application/dns-message"));

    let client = Client::new();
    let body = req_wireformat.to_vec();

    tokio::select! {
        res = doh_request(&client, "https://1.1.1.1/dns-query", &headers, &body) => res,
        res = doh_request(&client, "https://8.8.8.8/dns-query", &headers, &body) => res,
        _ = timeout(Duration::from_secs(5), std::future::pending::<()>()) => {
            Err(anyhow::anyhow!("DNS query timeout after 5 seconds"))
        }
    }
}

async fn doh_request(
    client: &Client,
    url: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Vec<u8>> {
    let response = client
        .post(url)
        .headers(headers.clone())
        .body(body.to_vec())
        .send()
        .await
        .with_context(|| format!("Failed to send request to {}", url))?
        .bytes()
        .await
        .with_context(|| format!("Failed to read response from {}", url))?;

    Ok(response.to_vec())
}