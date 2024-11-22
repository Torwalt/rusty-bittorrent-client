use std::time::Duration;

use anyhow::{Context, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Deserialize;

use crate::torrent;

// PORT is a hardcoded port number.
const PORT: usize = 6881;
const ID_SIZE: usize = 20;

struct QueryParams<'a> {
    info_hash: &'a str,
    peer_id: &'a String,
    port: usize,
    uploaded: usize,
    downloaded: usize,
    left: usize,
    compact: u8,
}

#[derive(Deserialize, Debug)]
pub struct PeerResponse {
    interval: u64,
    pub peers: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[serde(rename = "failure reason")]
    failure_reason: String,
}

pub struct Client {
    // Unique, 20 char String.
    peer_id: String,
    inner: reqwest::blocking::Client,
}

impl Client {
    pub fn new() -> Result<Client> {
        let id = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(ID_SIZE)
            .map(char::from)
            .collect();
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Client {
            peer_id: id,
            inner: client,
        })
    }

    pub fn find_peers(&self, req: torrent::Request) -> Result<PeerResponse> {
        let hash_url_encoded = urlencoding::encode_binary(&req.info_hash);
        println!("{}", hash_url_encoded);

        let query_params = QueryParams {
            info_hash: &hash_url_encoded.into_owned(),
            // info_hash: req.info_hash,
            peer_id: &self.peer_id,
            port: PORT,
            uploaded: 0,
            downloaded: 0,
            left: req.length,
            compact: 1,
        };

        let full_url = format!(
            "{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}",
            req.url.to_string(),
            query_params.info_hash,
            query_params.peer_id,
            query_params.port,
            query_params.uploaded,
            query_params.downloaded,
            query_params.left,
            query_params.compact
        );

        let resp = self
            .inner
            .request(reqwest::Method::GET, full_url)
            .send()
            .context("failed to sent GET request")?;

        println!("{}", resp.url());
        let status = resp.status();

        let body = resp.bytes()?;

        if !status.is_success() {
            anyhow::bail!("Request failed with status: {}", status);
        }

        if let Ok(error) = serde_bencode::from_bytes::<ErrorResponse>(&body) {
            anyhow::bail!(format!("API Error: {}", error.failure_reason))
        }

        let parsed: PeerResponse = serde_bencode::from_bytes(&body)
            .with_context(|| format!("Failed to parse bencoded string: {:?}", body))?;

        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_peers() -> Result<(), Box<dyn std::error::Error>> {
        let bencoded = b"d8:completei4e10:incompletei1e8:intervali60e12:min intervali60e5:peers18:\xa5\xe8)I\xc9d\xa5\xe8&\xa4\xc9L\xa5\xe8#r\xc8\xede";

        // How to make this work????
        let response: PeerResponse = serde_bencode::from_bytes(bencoded)?;

        Ok(())
    }
}
