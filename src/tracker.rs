use core::fmt;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{self, Receiver};

use anyhow::{anyhow, bail, Context, Result};
use log::debug;
use tokio::net::TcpStream;

use crate::peers::{Peer, PeerID, Peers};
use crate::torrent::{DownloadRequest, Hash};

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
    info_hash: Hash,
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
    fn new(info_hash: &Hash, peer_id: &PeerID) -> Handshake {
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
            info_hash: Hash::new(info_hash),
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

    async fn from_stream(&mut self, s: &mut TcpStream) -> Result<PeerMessage> {
        s.read_exact(&mut self.meta_buf).await?;
        let payload_len = self.payload_len();
        if payload_len > MAX_PAYLOAD_LEN {
            bail!(
                "message specifies too large payload length: allowed {} bytes wants {} bytes",
                MAX_PAYLOAD_LEN,
                payload_len
            );
        }
        let mut payload_buf = vec![0; payload_len as usize];
        s.read_exact(&mut payload_buf).await?;
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

struct FullPiece {
    data: Vec<u8>,
    piece: Piece,
}

#[derive(Debug)]
struct PiecePayload {
    block: Vec<u8>,
}

impl PiecePayload {
    fn to_bytes(&self) -> &[u8] {
        unimplemented!()
    }

    fn from_bytes(b: &[u8]) -> Result<PiecePayload> {
        let _ = u32::from_be_bytes(b[..4].try_into()?);
        let _ = u32::from_be_bytes(b[4..8].try_into()?);
        let block_rest = &b[8..];

        let block = if block_rest.len() < BLOCK_SIZE as usize {
            &block_rest
        } else {
            &b[8..8 + BLOCK_SIZE as usize]
        };

        Ok(PiecePayload {
            block: block.to_vec(),
        })
    }
}

struct DownloadingFile {
    bytes: Vec<u8>,
    piece_len: usize,
    piece_cnt: usize,
}

impl DownloadingFile {
    fn new(piece_len: usize, piece_cnt: usize) -> Self {
        let max_len = piece_len * piece_cnt;
        let mut bytes = Vec::with_capacity(max_len);
        bytes.resize(max_len, 0);
        Self {
            // Real usage will be less as last piece is usually smaller.
            bytes,
            piece_len,
            piece_cnt,
        }
    }

    fn add_full_piece(&mut self, fp: FullPiece) -> Result<()> {
        let idx = fp.piece.idx as usize;
        let offset = idx * self.piece_len;

        for (i, byte) in fp.data.into_iter().enumerate() {
            self.bytes.insert(offset + i, byte)
        }

        Ok(())
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

        let next_progress = self.progress + BLOCK_SIZE;
        let len = if next_progress <= self.piece_len {
            BLOCK_SIZE as u32
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

struct RequestQueue {
    gen: RequestPayloadGen,
}

impl RequestQueue {
    fn new(gen: RequestPayloadGen) -> Self {
        Self { gen }
    }

    fn receiver(mut self) -> Receiver<Option<RequestPayload>> {
        let (tx, rx) = mpsc::channel(5);
        tokio::spawn(async move {
            let mut req_cnt = 0;
            loop {
                let request = self.gen.next();
                req_cnt += 1;
                if tx.send(request).await.is_err() {
                    debug!("Receiver channel closed, closing sender channel.");
                    break;
                }
            }
            debug!("Created {} async requests", req_cnt);
        });

        rx
    }
}

struct Piece {
    hash: Hash,
    idx: u32,
    len: u32,
}

pub async fn download_file(
    client_id: PeerID,
    peers: Peers, // Use Arc<Peers> for shared ownership across tasks
    download_req: DownloadRequest,
) -> Result<Vec<u8>> {
    debug!("Have {} pieces to download.", download_req.pieces.len());
    debug!("Piece len is {}.", download_req.piece_length);
    debug!("Total length is {}.", download_req.length);

    // TODO: Just dummp all jobs into queue?
    // Job channel for peer tasks to grab next job.
    let (job_tx, job_rx) = async_channel::bounded::<Piece>(download_req.pieces.len());
    // Result channel for tasks to pass pieces to.
    let (result_tx, mut result_rx) = mpsc::channel::<FullPiece>(10); // Arbitrary num for now.

    let info_hash = Arc::new(download_req.info_hash.clone());
    let c_id = Arc::new(client_id);

    // Spawn multiple job executors, one for each available Peer.
    let mut handles = Vec::with_capacity(peers.iter().len());
    for peer in peers.into_iter() {
        let info_hash_clone = Arc::clone(&info_hash);
        let job_rx_clone = job_rx.clone();
        let result_tx_clone = result_tx.clone();
        let c_id_clone = Arc::clone(&c_id);
        let handle = tokio::spawn(async move {
            let peer_info = peer.to_string();
            let mut stream = setup_peer(&c_id_clone, peer, &info_hash_clone).await?;
            while let Ok(job) = job_rx_clone.recv().await {
                let full_piece = download_piece(job, &mut stream).await?;
                // TODO: Retry.
                result_tx_clone.send(full_piece).await?;
            }
            debug!("Closing connection to Peer {}", peer_info);

            Ok::<_, anyhow::Error>(())
        });
        handles.push(handle);
    }

    let piece_len = download_req.piece_length;
    let last_piece_len = download_req.last_piece_len();
    let pieces_cnt = download_req.pieces.len();

    // Fill up job queue.
    for (idx, hash) in download_req.pieces.into_iter().enumerate() {
        let current_piece_len = if idx + 1 == pieces_cnt {
            last_piece_len as u32
        } else {
            piece_len
        };

        let piece = Piece {
            hash,
            // TODO: Use usize everywhere.
            idx: idx.try_into()?,
            len: current_piece_len,
        };

        job_tx
            .send(piece)
            .await
            .context("job channel closed unexpectedly")?;
    }
    job_tx.close();

    // Wait for results and gather them.
    // TODO: Stream directly into file.
    let mut df = DownloadingFile::new(piece_len as usize, pieces_cnt as usize);
    let mut piece_counter = 0;
    while let Some(full_piece) = result_rx.recv().await {
        df.add_full_piece(full_piece)?;

        piece_counter += 1;
        if piece_counter == pieces_cnt {
            break;
        }
    }

    // Collect results from all spawned tasks
    for handle in handles {
        if let Err(e) = handle.await? {
            bail!("Task failed: {:?}", e);
        }
    }

    Ok(df.bytes)
}

pub async fn perform_download_piece(
    client_id: PeerID,
    peer: &Peer,
    download_req: DownloadRequest,
    piece_idx: u32,
) -> Result<Vec<u8>> {
    let mut stream = setup_peer(&client_id, peer.to_owned(), &download_req.info_hash).await?;
    let hash = download_req
        .pieces
        .get(piece_idx as usize)
        .ok_or(anyhow!("no piece at index {}", piece_idx))?
        .to_owned();
    let piece = Piece {
        hash,
        idx: piece_idx,
        len: download_req.piece_length,
    };

    let full_piece = download_piece(piece, &mut stream).await?;
    Ok(full_piece.data)
}

async fn setup_peer(client_id: &PeerID, peer: Peer, info_hash: &Hash) -> Result<TcpStream> {
    let mut stream = TcpStream::connect(peer.to_string()).await?;

    handshake(client_id, info_hash, &mut stream).await?;
    debug!("Performed Handshake for {}.", peer);
    let mut reader = PeerMessageReader::new();

    // Read Bitfield
    let mut msg = reader.from_stream(&mut stream).await?;
    match msg {
        PeerMessage::Bitfield => {}
        other => bail!("expected Bitfield PeerMessage, got {:?}", other),
    }
    debug!("Received Bitfield from {}.", peer);

    // Send Interested
    stream
        .write_all(&PeerMessage::Interested.to_bytes())
        .await?;
    debug!("Sent Interested to {}.", peer);

    // Read Unchoke
    msg = reader.from_stream(&mut stream).await?;
    match msg {
        PeerMessage::Unchoke => {}
        other => bail!("expected Unchoke PeerMessage, got {:?}", other),
    }
    debug!("Read Unchoke from {}", peer);

    Ok(stream)
}

async fn download_piece(piece: Piece, stream: &mut TcpStream) -> Result<FullPiece> {
    // Download Piece by requesting blocks of data until all data is read.
    let mut piece_data: Vec<u8> = Vec::with_capacity(piece.len as usize);
    let req_gen = RequestPayloadGen::new(piece.len, piece.idx as u32);
    let req_q = RequestQueue::new(req_gen);
    let mut rx = req_q.receiver();
    let mut reader = PeerMessageReader::new();
    while let Some(Some(req)) = rx.recv().await {
        debug!("Writing request for offset: {}.", req.begin);
        let peer_msg = PeerMessage::Request(req);
        let payload = peer_msg.to_bytes();
        stream.write_all(&payload).await?;
        debug!("Written Request.");

        let msg = reader.from_stream(stream).await?;
        debug!("Read Message from stream.");
        let piece_msg = match msg {
            PeerMessage::Piece(piece) => piece,
            other => bail!("expected Piece PeerMessage, got {:?}", other),
        };
        debug!("Received Piece data.");
        piece_data.append(&mut piece_msg.block.to_vec());
    }
    debug!("Closing receiver channel.");
    rx.close();

    // Checksums with sha1.
    let downloaded_piece_hash = Hash::hash(&piece_data);
    if downloaded_piece_hash != piece.hash {
        bail!(
            "hash not matching of downloaded piece have: {} want: {}",
            downloaded_piece_hash.to_hex(),
            piece.hash.to_hex()
        )
    }

    debug!("Download of piece with idx {} was successful", piece.idx);

    Ok(FullPiece {
        data: piece_data,
        piece,
    })
}

pub async fn perform_handshake(client_id: PeerID, peer: &Peer, info_hash: &Hash) -> Result<Handshake> {
    let mut stream = TcpStream::connect(peer.to_string()).await?;
    handshake(&client_id, info_hash, &mut stream).await
}

async fn handshake(client_id: &PeerID, info_hash: &Hash, stream: &mut TcpStream) -> Result<Handshake> {
    let handshake = Handshake::new(info_hash, client_id);
    let bytes = handshake.to_bytes();

    stream.write_all(&bytes).await?;

    let mut buf = [0; HANDSHAKE_BYTE_SIZE];
    let mut total_read = 0;
    while total_read < HANDSHAKE_BYTE_SIZE {
        let bytes_read = stream.read(&mut buf[total_read..]).await?;
        if bytes_read == 0 {
            bail!("Connection closed before handshake was fully read")
        }
        total_read += bytes_read;
    }

    Handshake::from_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

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
    fn test_request_payload_gen_next_smaller_piece_len() -> Result<(), Box<dyn std::error::Error>> {
        let piece_len = 6241 as usize;
        let mut gen = RequestPayloadGen::new(piece_len as u32, 0);
        let req = gen.next();
        assert_eq!(req.is_some(), true);

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

        let mut smaller_piece_bytes: Vec<u8> = Vec::new();
        smaller_piece_bytes.extend_from_slice(&0u32.to_be_bytes());
        smaller_piece_bytes.extend_from_slice(&0u32.to_be_bytes());
        let random_data: Vec<u8> = (0..1337).map(|_| rand::random::<u8>()).collect();
        smaller_piece_bytes.extend_from_slice(&random_data);
        PiecePayload::from_bytes(&smaller_piece_bytes)?;

        Ok(())
    }

    #[test]
    fn test_downloading_file_add_full_piece() -> Result<(), Box<dyn std::error::Error>> {
        let data = "this is some text";
        let piece_len = data.len();
        let mut df = DownloadingFile::new(piece_len as usize, 5);
        let mut rng = rand::thread_rng();

        let first_piece = FullPiece {
            data: data.into(),
            piece: Piece {
                hash: Hash::new(rng.gen()),
                idx: 0,
                len: piece_len as u32,
            },
        };
        df.add_full_piece(first_piece)?;

        let data2 = "THIS IS SOME TEXT";
        assert_eq!(piece_len, data2.len());

        let third_piece = FullPiece {
            data: data2.into(),
            piece: Piece {
                hash: Hash::new(rng.gen()),
                idx: 2,
                len: piece_len as u32,
            },
        };
        df.add_full_piece(third_piece)?;

        assert_eq!(
            data.as_bytes(),
            df.bytes.get(0..17).ok_or("bytes should have first piece")?
        );
        assert_eq!(
            data2.as_bytes(),
            df.bytes
                .get(34..51)
                .ok_or("bytes should have third piece")?
        );

        Ok(())
    }
}
