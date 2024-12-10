use core::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;

use anyhow::{anyhow, bail, Context, Result};

use crate::peers::{Peer, PeerID};
use crate::torrent::{self, InfoHash, PieceHash};

const HANDSHAKE_BYTE_SIZE: usize = 68;
// PORT is for now just hardcoded.
const BLOCK_SIZE: u32 = 16 * 1024;
const MAX_PAYLOAD_LEN: u32 = 1048576;

const LENGTH_PREFIX_SIZE_BYTES: u32 = 4;
const ID_SIZE_BYTES: u32 = 1;

const INDEX_SIZE_BYTES: u32 = 4;
const BEGIN_SIZE_BYTES: u32 = 4;
const LENGTH_SIZE_BYTES: u32 = 4;

const REQUEST_MESSAGE_LENGTH_BYTES: u32 =
    ID_SIZE_BYTES + INDEX_SIZE_BYTES + BEGIN_SIZE_BYTES + LENGTH_SIZE_BYTES;

const REQUEST_PAYLOAD_BYTES_COUNT: u32 = INDEX_SIZE_BYTES + BEGIN_SIZE_BYTES + LENGTH_SIZE_BYTES;
const REQUEST_BYTES_COUNT: u32 =
    LENGTH_PREFIX_SIZE_BYTES + ID_SIZE_BYTES + REQUEST_PAYLOAD_BYTES_COUNT;

pub struct Handshake {
    info_hash: InfoHash,
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
    fn new(info_hash: &InfoHash, peer_id: &PeerID) -> Handshake {
        Handshake {
            info_hash: info_hash.clone(),
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
        out[28..48].copy_from_slice(self.info_hash.get_hash());
        out[48..68].copy_from_slice(&self.peer_id);

        out
    }

    fn from_bytes(data: [u8; HANDSHAKE_BYTE_SIZE]) -> Result<Handshake> {
        let info_hash: [u8; 20] = data[28..48]
            .try_into()
            .context("when converting to info_hash")?;

        Ok(Handshake {
            info_hash: InfoHash::new(info_hash),
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
        let mut pl = u32::from_be_bytes(
            self.meta_buf[0..4]
                .try_into()
                .expect("[u8; 5] into [u8; 4] will always work"),
        );
        // Take off 1 from the length as the ident byte is already read.
        if pl > 0 {
            pl -= 1
        }
        pl
    }

    fn from_stream(&mut self, s: &mut TcpStream) -> Result<PeerMessage> {
        s.read_exact(&mut self.meta_buf)?;
        let payload_len = self.payload_len();
        if payload_len > MAX_PAYLOAD_LEN {
            bail!(
                "message specifies too large payload length: allowed {} bytes wants {} bytes",
                MAX_PAYLOAD_LEN,
                payload_len
            );
        }
        let mut payload_buf = vec![0; payload_len as usize];
        s.read_exact(&mut payload_buf)?;
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

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            PeerMessage::Unchoke => vec![0, 0, 0, 1, 1],
            PeerMessage::Interested => vec![0, 0, 0, 1, 2],
            PeerMessage::Bitfield => vec![0, 0, 0, 1, 5],
            PeerMessage::Request(msg) => {
                let mut out: Vec<u8> = Vec::with_capacity(REQUEST_BYTES_COUNT as usize);
                out.extend_from_slice(&REQUEST_MESSAGE_LENGTH_BYTES.to_be_bytes());
                out.extend_from_slice(&6u8.to_be_bytes());
                msg.append_bytes(&mut out);
                out
            }
            PeerMessage::Piece(msg) => msg.to_bytes().to_vec(),
        }
    }
}

#[derive(Debug)]
struct PiecePayload {
    block: [u8; BLOCK_SIZE as usize],
}

impl PiecePayload {
    fn to_bytes(&self) -> &[u8] {
        unimplemented!()
    }

    fn from_bytes(b: &[u8]) -> Result<PiecePayload> {
        let _ = u32::from_be_bytes(b[..4].try_into()?);
        let _ = u32::from_be_bytes(b[4..8].try_into()?);
        let block: [u8; BLOCK_SIZE as usize] = b[8..8 + BLOCK_SIZE as usize].try_into()?;
        Ok(PiecePayload { block })
    }
}

struct RequestPayloadGen {
    piece_len: u32,
    piece_idx: u32,
    progress: u32,
}

impl RequestPayloadGen {
    fn new(piece_len: u32, piece_idx: u32) -> Self {
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

        let next_progress = self.progress + BLOCK_SIZE as u32;
        let len = if next_progress <= self.piece_len {
            BLOCK_SIZE as u32
        } else {
            self.piece_len - (next_progress - BLOCK_SIZE as u32)
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
    index: u32,
    begin: u32,
    length: u32,
}

impl RequestPayload {
    fn from_bytes(_: &[u8]) -> Result<RequestPayload> {
        bail!("unexpected RequestPayload in PeerMessage, from_bytes is not implemented")
    }

    fn append_bytes(&self, to: &mut Vec<u8>) {
        to.extend_from_slice(&self.index.to_be_bytes());
        to.extend_from_slice(&self.begin.to_be_bytes());
        to.extend_from_slice(&self.length.to_be_bytes());
    }
}

pub struct Client {
    // Unique, 20 char String.
    peer_id: PeerID,
}

impl Client {
    pub fn new(id: PeerID) -> Result<Client> {
        Ok(Client { peer_id: id })
    }

    pub fn download_piece(
        &self,
        peer: &Peer,
        download_req: torrent::DownloadRequest,
        piece_idx: usize,
    ) -> Result<Vec<u8>> {
        let piece = download_req
            .pieces
            .get(piece_idx)
            .ok_or(anyhow!("no piece at index {}", piece_idx))?;

        let mut stream = TcpStream::connect(peer.to_string())?;
        self.handshake(download_req.info_hash, &mut stream)?;
        println!("Performed handshake");
        let mut reader = PeerMessageReader::new();

        // Read Bitfield
        let mut msg = reader.from_stream(&mut stream)?;
        match msg {
            PeerMessage::Bitfield => {}
            other => bail!("expected Bitfield PeerMessage, got {:?}", other),
        }
        println!("Received Bitfield");

        // Send Interested
        stream.write_all(&PeerMessage::Interested.to_bytes())?;
        println!("Sent Interested");

        // Read Unchoke
        msg = reader.from_stream(&mut stream)?;
        match msg {
            PeerMessage::Unchoke => {}
            other => bail!("expected Unchoke PeerMessage, got {:?}", other),
        }
        println!("Read Unchoke");

        // Download Piece by requesting blocks of data until all data is read.
        let mut piece_data: Vec<u8> = Vec::with_capacity(download_req.piece_length);
        let mut req_gen =
        // TODO: piece_length might be u32 always, same with piece_idx.
            RequestPayloadGen::new(download_req.piece_length as u32, piece_idx as u32);
        while let Some(req) = req_gen.next() {
            println!("Writing request for offset: {}", req.begin);
            let peer_msg = PeerMessage::Request(req);
            let payload = peer_msg.to_bytes();
            stream.write_all(&payload)?;
            println!("Written Request");

            msg = reader.from_stream(&mut stream)?;
            println!("Read Message from stream");
            let piece_msg = match msg {
                PeerMessage::Piece(piece) => piece,
                other => bail!("expected Piece PeerMessage, got {:?}", other),
            };
            println!("Received Piece data.");
            piece_data.append(&mut piece_msg.block.to_vec());
        }

        // Checksums with sha1.
        let downloaded_piece_hash = &PieceHash::hash(&piece_data);
        if downloaded_piece_hash != piece {
            bail!(
                "hash not matching of downloaded piece have: {} want: {}",
                downloaded_piece_hash.to_hex(),
                piece.to_hex()
            )
        }

        Ok(piece_data)
    }

    pub fn perform_handshake(&self, peer: &Peer, info_hash: &InfoHash) -> Result<Handshake> {
        let mut stream = TcpStream::connect(peer.to_string())?;
        self.handshake(info_hash, &mut stream)
    }

    fn handshake(&self, info_hash: &InfoHash, stream: &mut TcpStream) -> Result<Handshake> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_payload_gen_next() -> Result<(), Box<dyn std::error::Error>> {
        let piece_len = 32768 as usize;
        let mut gen = RequestPayloadGen::new(piece_len as u32, 0);
        let counts = 2; // BLOCK_SIZE * 2 == piece_len
        let mut cnt = 0;
        for _n in 0..counts {
            let is_some = gen.next().is_some();
            cnt += 1;
            assert_eq!(is_some, true)
        }

        assert_eq!(gen.next().is_some(), false);
        assert_eq!(cnt, 2);
        Ok(())
    }

    #[test]
    fn test_piece_payload_from_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let mut piece_bytes: Vec<u8> = Vec::new();
        piece_bytes.extend_from_slice(&0u32.to_be_bytes());
        piece_bytes.extend_from_slice(&0u32.to_be_bytes());
        let random_data: Vec<u8> = (0..16384).map(|_| rand::random::<u8>()).collect();
        piece_bytes.extend_from_slice(&random_data);

        PiecePayload::from_bytes(&piece_bytes)?;

        Ok(())
    }
}
