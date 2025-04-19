mod common;
mod config;
mod proxy;

use crate::config::Config;
use crate::proxy::*;

use std::collections::HashMap;
use uuid::Uuid;
use worker::*;
use once_cell::sync::Lazy;
use regex::Regex;

static PROXYIP_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(.+?)[:=-](\d{1,5})$").unwrap());
static PROXYKV_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^([a-zA-Z]{2})").unwrap());

#[event(fetch)]
async fn main(req: Request, env: Env, _: Context) -> Result<Response> {
    let uuid = env
        .var("UUID")
        .map(|x| Uuid::parse_str(&x.to_string()).unwrap_or_default())?;
    let host = req.url()?.host().map(|x| x.to_string()).unwrap_or_default();
    let main_page_url = env.var("MAIN_PAGE_URL").map(|x|x.to_string()).unwrap();
    let proxy_kv_url = env.var("PROXY_KV_URL").map(|x|x.to_string()).unwrap();
    let config = Config { uuid, proxy_addr: host, proxy_port: 443, main_page_url, proxy_kv_url };

    Router::with_data(config)
        .on_async("/", fe)
        .on_async("/free/cc/:proxyip", tunnel)
        .on_async("/free/:proxyip", tunnel)
        .on_async("/:proxyip", tunnel)
        .run(req, env)
        .await
}

async fn fe(_: Request, cx: RouteContext<Config>) -> Result<Response> {
    Response::redirect(cx.data.main_page_url.parse()?)
}

async fn tunnel(req: Request, mut cx: RouteContext<Config>) -> Result<Response> {
    let proxyip = cx.param("proxyip").unwrap().to_string();

    if PROXYKV_PATTERN.is_match(&proxyip) {
        let country_code = proxyip.to_uppercase();
        
        let kv = cx.kv("SIREN")?;
        let mut proxy_kv_str = kv.get("proxy_kv").text().await?.unwrap_or_default();

        if proxy_kv_str.is_empty() {
            console_log!("Fetching proxy list from GitHub...");
            let req = Fetch::Url(Url::parse(&cx.data.proxy_kv_url)?);
            let mut res = req.send().await?;
            
            if res.status_code() != 200 {
                return Err(Error::from(format!("Failed to fetch proxy list: {}", res.status_code())));
            }
            
            proxy_kv_str = res.text().await?;
            kv.put("proxy_kv", &proxy_kv_str)?
                .expiration_ttl(60 * 60 * 6)
                .execute()
                .await?;
        }
        
        let proxy_kv: HashMap<String, Vec<String>> = serde_json::from_str(&proxy_kv_str)
            .map_err(|e| Error::from(format!("Failed to parse proxy list: {}", e)))?;

        let proxy_list = proxy_kv.get(&country_code)
            .ok_or_else(|| Error::from(format!("No proxies available for country: {}", country_code)))?;
        
        if proxy_list.is_empty() {
            return Err(Error::from(format!("Proxy list is empty for country: {}", country_code)));
        }

        let mut rand_buf = [0u8; 4];
        getrandom::getrandom(&mut rand_buf).expect("Failed to generate random number");
        let proxy_index = (rand_buf[0] as usize) % proxy_list.len();
        let selected_proxy = &proxy_list[proxy_index];

        if let Some(captures) = PROXYIP_PATTERN.captures(selected_proxy) {
            cx.data.proxy_addr = captures.get(1).unwrap().as_str().to_string();
            cx.data.proxy_port = captures.get(2).unwrap().as_str().parse()
                .map_err(|e| Error::from(format!("Invalid port number: {}", e)))?;
        } else {
            return Err(Error::from(format!("Invalid proxy format: {}", selected_proxy)));
        }
    } else if PROXYIP_PATTERN.is_match(&proxyip) {
        if let Some(captures) = PROXYIP_PATTERN.captures(&proxyip) {
            cx.data.proxy_addr = captures.get(1).unwrap().as_str().to_string();
            cx.data.proxy_port = captures.get(2).unwrap().as_str().parse()
                .map_err(|e| Error::from(format!("Invalid port number: {}", e)))?;
        }
    } else {
        return Err(Error::from("Invalid proxy format. Use either country code (e.g. AE, ID) or IP:PORT"));
    }

    let upgrade = req.headers().get("Upgrade")?.unwrap_or_default();
    if upgrade.to_lowercase() == "websocket" {
        let WebSocketPair { server, client } = WebSocketPair::new()?;
        server.accept()?;
    
        wasm_bindgen_futures::spawn_local(async move {
            let events = server.events().unwrap();
            if let Err(e) = ProxyStream::new(cx.data, &server, events).process().await {
                console_log!("[tunnel error]: {}", e);
            }
        });
    
        Response::from_websocket(client)
    } else {
        Response::redirect(cx.data.main_page_url.parse()?)
    }
}
