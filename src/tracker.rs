use core::fmt;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc::{self, Receiver, Sender};

use anyhow::{anyhow, bail, Context, Result};
use log::debug;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

use crate::peers::{Peer, PeerID, Peers};
use crate::torrent::{DownloadRequest, Hash};

const HANDSHAKE_BYTE_SIZE: usize = 68;
// PORT is for now just hardcoded.
const BLOCK_SIZE: usize = 16 * 1024;
const MAX_PAYLOAD_LEN: usize = 1048576;

const LENGTH_PREFIX_SIZE_BYTES: usize = 4;
const ID_SIZE_BYTES: usize = 1;

const INDEX_SIZE_BYTES: usize = 4;
const BEGIN_SIZE_BYTES: usize = 4;
const LENGTH_SIZE_BYTES: usize = 4;

const REQUEST_MESSAGE_LENGTH_BYTES: usize =
    ID_SIZE_BYTES + INDEX_SIZE_BYTES + BEGIN_SIZE_BYTES + LENGTH_SIZE_BYTES;

const REQUEST_PAYLOAD_BYTES_COUNT: usize = INDEX_SIZE_BYTES + BEGIN_SIZE_BYTES + LENGTH_SIZE_BYTES;
const REQUEST_BYTES_COUNT: usize =
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

    fn payload_len(&self) -> usize {
        let mut pl = u32::from_be_bytes(
            self.meta_buf[0..4]
                .try_into()
                .expect("[u8; 5] into [u8; 4] will always work"),
        );
        // Take off 1 from the length as the ident byte is already read.
        if pl > 0 {
            pl -= 1
        }
        pl as usize
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
        let mut payload_buf = vec![0; payload_len];
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
                let mut out: Vec<u8> = Vec::with_capacity(REQUEST_BYTES_COUNT);
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

        let block = if block_rest.len() < BLOCK_SIZE {
            &block_rest
        } else {
            &b[8..8 + BLOCK_SIZE]
        };

        Ok(PiecePayload {
            block: block.to_vec(),
        })
    }
}

struct DownloadingFile {
    piece_len: usize,
    file: File,
}

impl DownloadingFile {
    async fn new(piece_len: usize, dest: PathBuf) -> Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(dest)
            .await?;

        Ok(Self { piece_len, file })
    }

    async fn write_full_piece(&mut self, fp: FullPiece) -> Result<()> {
        let idx = fp.piece.idx;
        let offset = idx * self.piece_len;

        self.file.seek(SeekFrom::Start(offset as u64)).await?;
        self.file.write_all(&fp.data).await?;

        Ok(())
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
            index: self.piece_idx.try_into().expect("must fit into u32"),
            begin: self.progress.try_into().expect("must fit into u32"),
            length: len.try_into().expect("must fit into u32"),
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
    idx: usize,
    len: usize,
}

impl fmt::Display for Piece {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "hash: {}, idx: {}, len: {}",
            self.hash.to_string(),
            self.idx,
            self.len
        )?;
        Ok(())
    }
}

struct PeerWorkerSetup {
    info_hash: Arc<Hash>,
    client_id: Arc<PeerID>,
    result_tx: Arc<Sender<FullPiece>>,
    job_rx: Arc<async_channel::Receiver<Piece>>,
    peers: Peers,
}

fn setup_peer_workers(pws: PeerWorkerSetup) -> Vec<JoinHandle<Result<(), anyhow::Error>>> {
    // Spawn multiple job executors, one for each available Peer.
    let mut handles = Vec::with_capacity(pws.peers.len());
    for peer in pws.peers.into_iter() {
        let handle = tokio::spawn({
            let info_hash = Arc::clone(&pws.info_hash);
            let job_rx = Arc::clone(&pws.job_rx);
            let result_tx = Arc::clone(&pws.result_tx);
            let client_id = Arc::clone(&pws.client_id);

            async move {
                let peer_info = peer.to_string();
                let mut stream = setup_peer(&client_id, peer, &info_hash).await?;
                while let Ok(job) = job_rx.recv().await {
                    debug!("Executing Job {} on Peer {}", job, peer_info);
                    let full_piece = download_piece(job, &mut stream).await?;
                    result_tx.send(full_piece).await?;
                }
                debug!("Closing connection to Peer {}", peer_info);

                Ok::<_, anyhow::Error>(())
            }
        });
        handles.push(handle);
    }
    handles
}

pub async fn download_file(
    client_id: PeerID,
    peers: Peers,
    download_req: DownloadRequest,
    output_path: PathBuf,
) -> Result<()> {
    debug!("Have {} pieces to download.", download_req.pieces.len());
    debug!("Piece len is {}.", download_req.piece_length);
    debug!("Total length is {}.", download_req.length);

    // Job channel for peer tasks to grab next job.
    let (job_tx, job_rx) = async_channel::bounded::<Piece>(download_req.pieces.len());
    // Result channel for tasks to pass pieces to.
    let (result_tx, mut result_rx) = mpsc::channel::<FullPiece>(10); // Arbitrary num for now.

    let piece_len = download_req.piece_length;
    let last_piece_len = download_req.last_piece_len();
    let pieces_cnt = download_req.pieces.len();

    // Spawn multiple job executors, one for each available Peer.
    let handles = setup_peer_workers(PeerWorkerSetup {
        info_hash: Arc::new(download_req.info_hash),
        client_id: Arc::new(client_id),
        result_tx: Arc::new(result_tx),
        job_rx: Arc::new(job_rx),
        peers,
    });

    debug!("Filling up job channels.");
    for (idx, hash) in download_req.pieces.into_iter().enumerate() {
        let current_piece_len = if idx + 1 == pieces_cnt {
            last_piece_len
        } else {
            piece_len
        };

        let piece = Piece {
            hash,
            idx,
            len: current_piece_len,
        };

        debug!("Sending job {}", piece);
        job_tx
            .send(piece)
            .await
            .context("job channel closed unexpectedly")?;
    }
    job_tx.close();
    debug!("Closed job channels.");

    // Wait for results and gather them.
    let mut df = DownloadingFile::new(piece_len, output_path).await?;
    while let Some(full_piece) = result_rx.recv().await {
        debug!(
            "Received FullPiece {} at {}",
            full_piece.piece,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_micros()
        );
        df.write_full_piece(full_piece).await?;
    }

    // Report if any peers failed. In a real scenario, we would introduce retry mechanisms, e.g.
    // retry with same peer, or just put the job back into the channel so another Peer worker can
    // grab it. However, as I am developing against a specific bittorrent impl, there are no
    // error cases.
    for handle in handles {
        if let Err(e) = handle.await? {
            bail!("Task failed: {:?}", e);
        }
    }

    Ok(())
}

pub async fn perform_download_piece(
    client_id: PeerID,
    peer: &Peer,
    download_req: DownloadRequest,
    piece_idx: usize,
) -> Result<Vec<u8>> {
    let mut stream = setup_peer(&client_id, peer.to_owned(), &download_req.info_hash).await?;
    let hash = download_req
        .pieces
        .get(piece_idx)
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
    let mut piece_data: Vec<u8> = Vec::with_capacity(piece.len);
    let req_gen = RequestPayloadGen::new(piece.len, piece.idx);
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

pub async fn perform_handshake(
    client_id: PeerID,
    peer: &Peer,
    info_hash: &Hash,
) -> Result<Handshake> {
    let mut stream = TcpStream::connect(peer.to_string()).await?;
    handshake(&client_id, info_hash, &mut stream).await
}

async fn handshake(
    client_id: &PeerID,
    info_hash: &Hash,
    stream: &mut TcpStream,
) -> Result<Handshake> {
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

    #[test]
    fn test_request_payload_gen_next() -> Result<(), Box<dyn std::error::Error>> {
        let piece_len = 32768;
        let mut gen = RequestPayloadGen::new(piece_len, 0);
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
        let piece_len = 6241;
        let mut gen = RequestPayloadGen::new(piece_len, 0);
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
}
