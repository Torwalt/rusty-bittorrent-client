use core::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Deserialize;
use serde_with::{serde_as, Bytes};
use sha1::{Digest, Sha1};

use crate::torrent;

// PORT is for now just hardcoded.
const PORT: usize = 6881;
const ID_SIZE: usize = 20;
const PEER_BYTE_SIZE: usize = 6;
const HANDSHAKE_BYTE_SIZE: usize = 68;
const BLOCK_SIZE: usize = 16 * 1024;
const MAX_PAYLOAD_LEN: u32 = 1048576;

struct QueryParams<'a> {
    info_hash: &'a str,
    peer_id: &'a String,
    port: usize,
    uploaded: usize,
    downloaded: usize,
    left: usize,
    compact: u8,
}

#[serde_as]
#[derive(Deserialize, Debug)]
pub struct PeerResponse {
    interval: u64,
    #[serde_as(as = "Bytes")]
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
}

impl fmt::Display for Peers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for peer in self.iter() {
            writeln!(f, "{}:{}", peer.ip, peer.port)?;
        }
        Ok(())
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

    fn to_string(&self) -> String {
        return format!("{}:{}", self.ip, self.port);
    }
}

pub struct Handshake {
    info_hash: torrent::InfoHash,
    peer_id: Vec<u8>,
}

impl fmt::Display for Handshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_representation: String = self
            .peer_id
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect();
        writeln!(f, "Peer ID: {}", hex_representation)
    }
}

impl Handshake {
    fn new(info_hash: &torrent::InfoHash, peer_id: &String) -> Handshake {
        Handshake {
            info_hash: torrent::InfoHash(info_hash.0.to_owned()),
            peer_id: peer_id.as_bytes().to_vec(),
        }
    }

    fn to_bytes(&self) -> [u8; HANDSHAKE_BYTE_SIZE] {
        /*
            length of the protocol string (BitTorrent protocol) which is 19 (1 byte)
            the string BitTorrent protocol (19 bytes)
            eight reserved bytes, which are all set to zero (8 bytes)
            sha1 infohash (20 bytes) (NOT the hexadecimal representation, which is 40 bytes long)
            peer id (20 bytes) (generate 20 random byte values)
        */
        let mut out = [0; HANDSHAKE_BYTE_SIZE];

        const PROTOCOL: &str = "BitTorrent protocol";
        const PROTOCOL_LEN: u8 = PROTOCOL.len() as u8;

        out[0] = PROTOCOL_LEN;
        out[1..20].copy_from_slice(&PROTOCOL.as_bytes());
        // out[20..28] -> Reserved
        out[28..48].copy_from_slice(&self.info_hash.0);
        out[48..68].copy_from_slice(&self.peer_id);

        out
    }

    fn from_bytes(data: [u8; HANDSHAKE_BYTE_SIZE]) -> Result<Handshake> {
        let info_hash: [u8; 20] = data[28..48]
            .try_into()
            .context("when converting to info_hash")?;

        Ok(Handshake {
            info_hash: torrent::InfoHash(info_hash),
            peer_id: data[48..68].to_vec(),
        })
    }
}

struct PeerMessageReader {
    meta_buf: [u8; 5],
}

impl PeerMessageReader {
    fn new() -> Self {
        Self { meta_buf: [0; 5] }
    }
    fn ident_byte(&self) -> u8 {
        self.meta_buf[4]
    }

    fn payload_len(&self) -> u32 {
        u32::from_be_bytes(
            self.meta_buf[0..4]
                .try_into()
                .expect("[u8; 5] into [u8; 4] will always work"),
        )
    }

    fn from_stream(&mut self, s: &mut TcpStream) -> Result<PeerMessage> {
        s.read_exact(&mut self.meta_buf)?;
        let payload_len = self.payload_len();
        if payload_len > MAX_PAYLOAD_LEN {
            bail!(
                "message specifies too large payload length: allowed {} bytes wants {} bytes",
                MAX_PAYLOAD_LEN,
                payload_len
            )
        }
        let payload_buf = vec![0; self.payload_len() as usize];
        let pm = PeerMessage::from_bytes(self.ident_byte(), &payload_buf)?;

        Ok(pm)
    }
}

#[derive(Debug)]
enum PeerMessage {
    Bitfield,
    Interested,
    Unchoke,
    Request(RequestPayload),
    Piece(PiecePayload),
}

impl PeerMessage {
    fn from_bytes(ident: u8, payload: &[u8]) -> Result<PeerMessage> {
        match ident {
            1 => Ok(Self::Unchoke),
            2 => Ok(Self::Interested),
            5 => Ok(Self::Bitfield),
            6 => {
                let msg = RequestPayload::from_bytes(payload)?;
                Ok(Self::Request(msg))
            }
            7 => {
                let msg = PiecePayload::from_bytes(payload)?;
                Ok(Self::Piece(msg))
            }
            other => bail!("unknown byte message id: {}", other),
        }
    }

    fn to_bytes(&self) -> &[u8] {
        match self {
            PeerMessage::Unchoke => &[1],
            PeerMessage::Interested => &[0, 0, 0, 1, 2],
            PeerMessage::Bitfield => &[5],
            PeerMessage::Request(msg) => msg.to_bytes(),
            PeerMessage::Piece(msg) => msg.to_bytes(),
        }
    }
}

#[derive(Debug)]
struct PiecePayload {
    index: u32,
    begin: u32,
    block: [u8; BLOCK_SIZE],
}

impl PiecePayload {
    fn to_bytes(&self) -> &[u8] {
        unimplemented!()
    }

    fn from_bytes(b: &[u8]) -> Result<PiecePayload> {
        unimplemented!()
    }
}

struct RequestPayloadGen {
    piece_len: usize,
    piece_idx: usize,
    progress: usize,
}

impl RequestPayloadGen {
    fn new(piece_len: usize, piece_idx: usize) -> Self {
        Self {
            piece_len,
            piece_idx,
            progress: 0,
        }
    }

    fn next(&mut self) -> Option<RequestPayload> {
        if self.progress >= self.piece_len {
            return None;
        }

        let next_progress = self.progress + BLOCK_SIZE;
        let len = if next_progress <= self.piece_len {
            BLOCK_SIZE
        } else {
            self.piece_len - (next_progress - BLOCK_SIZE)
        };

        let rp = RequestPayload {
            index: self.piece_idx,
            begin: self.progress,
            length: len,
        };
        self.progress = next_progress;
        Some(rp)
    }
}

#[derive(Debug)]
struct RequestPayload {
    index: usize,
    begin: usize,
    length: usize,
}

impl RequestPayload {
    fn from_bytes(bytes: &[u8]) -> Result<RequestPayload> {
        unimplemented!()
    }

    fn to_bytes(&self) -> &[u8] {
        unimplemented!()
    }
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

    pub fn download_piece(
        &self,
        peer: &Peer,
        req: torrent::DownloadRequest,
        piece_idx: usize,
    ) -> Result<Vec<u8>> {
        let mut stream = TcpStream::connect(peer.to_string())?;
        self.handshake(req.info_hash, &mut stream)?;
        let mut reader = PeerMessageReader::new();

        // Read Bitfield
        let mut msg = reader.from_stream(&mut stream)?;
        match msg {
            PeerMessage::Bitfield => {}
            other => bail!("expected Bitfield PeerMessage, got {:?}", other),
        }

        // Send Interested
        stream.write_all(PeerMessage::Interested.to_bytes())?;

        // Read Unchoke
        msg = reader.from_stream(&mut stream)?;
        match msg {
            PeerMessage::Unchoke => {}
            other => bail!("expected Unchoke PeerMessage, got {:?}", other),
        }

        let piece = req
            .pieces
            .get(piece_idx)
            .ok_or(anyhow!("no piece at index {}", piece_idx))?;

        let mut piece_data: Vec<u8> = Vec::new();
        let mut req_gen = RequestPayloadGen::new(req.piece_length, piece_idx);
        while let Some(req) = req_gen.next() {
            let payload = req.to_bytes();
            stream.write_all(payload)?;

            msg = reader.from_stream(&mut stream)?;
            let piece_msg = match msg {
                PeerMessage::Piece(piece) => piece,
                other => bail!("expected Piece PeerMessage, got {:?}", other),
            };
            piece_data.append(&mut piece_msg.block.to_vec());
        }

        let mut hasher = Sha1::new();
        hasher.update(&piece_data);
        let downloaded_piece_hash: [u8; 20] = hasher.finalize().into();

        if downloaded_piece_hash != piece.hash[0..20] {
            bail!(
                "hash not matching of downloaded piece have: {} want: {}",
                hash_to_hex(&downloaded_piece_hash),
                hash_to_hex(&piece.hash)
            )
        }

        Ok(piece_data)
    }

    pub fn perform_handshake(
        &self,
        peer: &Peer,
        info_hash: &torrent::InfoHash,
    ) -> Result<Handshake> {
        let mut stream = TcpStream::connect(peer.to_string())?;
        self.handshake(info_hash, &mut stream)
    }

    fn handshake(
        &self,
        info_hash: &torrent::InfoHash,
        stream: &mut TcpStream,
    ) -> Result<Handshake> {
        let handshake = Handshake::new(info_hash, &self.peer_id);
        let bytes = handshake.to_bytes();

        stream.write_all(&bytes)?;

        let mut buf = [0; HANDSHAKE_BYTE_SIZE];
        let mut total_read = 0;
        while total_read < HANDSHAKE_BYTE_SIZE {
            let bytes_read = stream.read(&mut buf[total_read..])?;
            if bytes_read == 0 {
                bail!("Connection closed before handshake was fully read")
            }
            total_read += bytes_read;
        }

        Handshake::from_bytes(buf)
    }

    pub fn find_peers(&self, req: torrent::PeerRequest) -> Result<Peers> {
        let hash_url_encoded = urlencoding::encode_binary(&req.info_hash.0);

        let query_params = QueryParams {
            info_hash: &hash_url_encoded.into_owned(),
            peer_id: &self.peer_id,
            port: PORT,
            uploaded: 0,
            downloaded: 0,
            left: req.length,
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
            .context("failed to sent GET request")?;

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

        Peers::from_peer_response(parsed)
    }
}

// TODO: need to consolidate that into a type or so.
fn hash_to_hex(hash: &[u8]) -> String {
    hash.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_peers() -> Result<(), Box<dyn std::error::Error>> {
        let bencoded = b"d8:completei4e10:incompletei1e8:intervali60e12:min intervali60e5:peers18:\xa5\xe8)I\xc9d\xa5\xe8&\xa4\xc9L\xa5\xe8#r\xc8\xede";

        let response: PeerResponse = serde_bencode::from_bytes(bencoded)?;

        assert_eq!(response.interval, 60);
        let has_data = response.peers.len() > 0;
        assert_eq!(true, has_data);

        Ok(())
    }
}
