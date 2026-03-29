use crate::storage::ProxyConfig;
use anyhow::Result;
use reqwest::Client;

/// Build a reqwest Client with optional proxy support
pub fn build_client(proxy: &ProxyConfig, timeout_secs: u64) -> Result<Client> {
    let mut builder = Client::builder().timeout(std::time::Duration::from_secs(timeout_secs));

    if proxy.enabled && !proxy.url.is_empty() {
        let proxy = reqwest::Proxy::all(&proxy.url)?;
        builder = builder.proxy(proxy);
    }

    Ok(builder.build()?)
}
