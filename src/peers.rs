use core::fmt;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Deserialize;
use serde_with::{serde_as, Bytes};

use crate::torrent;

const PEER_BYTE_SIZE: usize = 6;
const PORT: usize = 6881;
const ID_SIZE: usize = 20;

pub struct PeerID(String);

impl Clone for PeerID {
    fn clone(&self) -> PeerID {
        PeerID(self.0.clone())
    }
}

impl PeerID {
    pub fn new() -> PeerID {
        let id = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(ID_SIZE)
            .map(char::from)
            .collect();
        PeerID(id)
    }

    pub fn to_string(&self) -> &String {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0.as_bytes()
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    ip: IpAddr,
    port: u16,
}

impl std::str::FromStr for Peer {
    type Err = String;

    fn from_str(s: &str) -> Result<Peer, String> {
        s.parse::<SocketAddr>()
            .map(|addr| Peer {
                ip: addr.ip(),
                port: addr.port(),
            })
            .map_err(|_| format!("Invalid Peer SocketAddr: {}", s))
    }
}

impl std::fmt::Display for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl Peer {
    fn from_bytes(b: &[u8]) -> Result<Peer> {
        if b.len() != 6 {
            anyhow::bail!(format!(
                "expected 6 bytes to build a Peer, have {}",
                b.len()
            ));
        }

        let ip_bytes: [u8; 4] = [b[0], b[1], b[2], b[3]];
        let port_bytes: [u8; 2] = [b[4], b[5]];

        let ip = IpAddr::from(ip_bytes);
        let port = u16::from_be_bytes(port_bytes);

        Ok(Peer { ip, port })
    }

    pub fn to_string(&self) -> String {
        return format!("{}:{}", self.ip, self.port);
    }
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    #[serde(rename = "failure reason")]
    failure_reason: String,
}

struct QueryParams<'a> {
    info_hash: &'a str,
    peer_id: &'a String,
    port: usize,
    uploaded: usize,
    downloaded: usize,
    left: usize,
    compact: u8,
}

pub struct Peers(Vec<Peer>);

impl Peers {
    pub fn iter(&self) -> std::slice::Iter<'_, Peer> {
        self.0.iter()
    }

    fn from_peer_response(pr: PeerResponse) -> Result<Peers> {
        let mut out = Vec::new();
        let chunks = pr.peers.chunks(PEER_BYTE_SIZE);
        for chunk in chunks {
            let p = Peer::from_bytes(chunk)?;
            out.push(p);
        }

        Ok(Peers(out))
    }

    pub(crate) fn into_iter(self) -> std::vec::IntoIter<Peer> {
        self.0.into_iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for Peers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for peer in self.iter() {
            writeln!(f, "{}:{}", peer.ip, peer.port)?;
        }
        Ok(())
    }
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct PeerResponse {
    #[serde_as(as = "Bytes")]
    pub peers: Vec<u8>,
}

pub struct Client {
    // Unique, 20 char String.
    peer_id: PeerID,
    inner: reqwest::Client,
}

impl Client {
    pub fn new(id: PeerID) -> Result<Client> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Client {
            peer_id: id,
            inner: client,
        })
    }

    pub async fn find_peers(&self, req: torrent::PeerRequest<'_>) -> Result<Peers> {
        let hash_url_encoded = urlencoding::encode_binary(req.info_hash.get_hash());

        let query_params = QueryParams {
            info_hash: &hash_url_encoded.into_owned(),
            peer_id: &self.peer_id.to_string(),
            port: PORT,
            uploaded: 0,
            downloaded: 0,
            left: req.length as usize,
            compact: 1,
        };

        // Thats kinda shitty, but I did not find a way to encode info_hash, and skip double
        // encoding by url::Url or .query (of reqwest).
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
            .await
            .context("failed to sent GET request")?;

        let status = resp.status();

        let body = resp.bytes().await?;

        if !status.is_success() {
            anyhow::bail!("Request failed with status: {}", status);
        }

        if let Ok(error) = serde_bencode::from_bytes::<ErrorResponse>(&body) {
            anyhow::bail!(format!("API Error: {}", error.failure_reason))
        }

        let parsed: PeerResponse = serde_bencode::from_bytes(&body)
            .with_context(|| format!("Failed to parse bencoded string: {:?}", body))?;

        Peers::from_peer_response(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_peers() -> Result<(), Box<dyn std::error::Error>> {
        let bencoded = b"d8:completei4e10:incompletei1e8:intervali60e12:min intervali60e5:peers18:\xa5\xe8)I\xc9d\xa5\xe8&\xa4\xc9L\xa5\xe8#r\xc8\xede";

        let response: PeerResponse = serde_bencode::from_bytes(bencoded)?;

        let has_data = response.peers.len() > 0;
        assert_eq!(true, has_data);

        Ok(())
    }
}
