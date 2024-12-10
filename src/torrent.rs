use core::fmt;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Bytes};
use sha1::{Digest, Sha1};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use url::Url;

use anyhow::{Context, Result};

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct TorrentFile {
    #[serde(rename = "announce")]
    tracker_url: String,
    #[serde(rename = "created by")]
    created_by: String,
    info: FileInfo,
}

#[serde_as]
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct FileInfo {
    length: usize,
    name: String,
    #[serde(rename = "piece length")]
    piece_length: usize,
    #[serde_as(as = "Bytes")]
    pieces: Vec<u8>,
}

impl TorrentFile {
    pub fn parse_from_file(torrent_path: &PathBuf) -> Result<TorrentFile> {
        let mut file = File::open(torrent_path)?;

        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        Self::parse(content)
    }

    fn parse(content: Vec<u8>) -> Result<TorrentFile> {
        serde_bencode::from_bytes(&content).context("could not parse content into Meta")
    }
}

pub struct PeerRequest<'a> {
    pub url: Url,
    pub info_hash: &'a InfoHash,
    pub length: usize,
}

pub struct DownloadRequest<'a> {
    pub length: usize,
    pub piece_length: usize,
    pub pieces: &'a [PieceHash],
    // TODO: Should be static.
    pub info_hash: &'a InfoHash,
}

pub struct Torrent {
    tracker_url: Url,
    info: Info,
}

impl fmt::Display for Torrent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tracker URL: {}", self.tracker_url)?;
        writeln!(f, "{}", self.info)
    }
}

impl Torrent {
    pub fn from_file_torrent(tf: &TorrentFile) -> Result<Torrent> {
        let parsed_url = url::Url::parse(&tf.tracker_url)?;
        let info = Info::from_file_info(&tf.info)?;

        Ok(Torrent {
            tracker_url: parsed_url,
            info,
        })
    }

    pub fn to_peer_request(&self) -> PeerRequest {
        PeerRequest {
            // Cloning is ok here, as it is done once per file.
            url: self.tracker_url.clone(),
            info_hash: &self.info.hash,
            length: self.info.length,
        }
    }

    pub fn to_download_request(&self) -> DownloadRequest {
        DownloadRequest {
            length: self.info.length,
            piece_length: self.info.piece_length,
            pieces: self.info.pieces.as_slice(),
            info_hash: &self.info.hash,
        }
    }
}

pub struct InfoHash([u8; 20]);

impl Clone for InfoHash {
    fn clone(&self) -> InfoHash {
        InfoHash(self.0.clone())
    }
}

impl InfoHash {
    pub fn new(hash: [u8; 20]) -> InfoHash {
        InfoHash(hash)
    }

    pub fn get_hash(&self) -> &[u8; 20] {
        return &self.0;
    }

    pub fn to_hex(&self) -> String {
        hash_to_hex(&self.0.to_vec())
    }
}

struct Info {
    length: usize,
    piece_length: usize,
    pieces: Vec<PieceHash>,
    hash: InfoHash,
}

impl fmt::Display for Info {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Length: {}", self.length)?;
        writeln!(f, "Info Hash {}", self.hash.to_hex())?;
        writeln!(f, "Piece Length: {}", self.piece_length)?;
        writeln!(f, "Piece Hashes")?;
        for p in &self.pieces {
            write!(f, "{}", p)?
        }

        Ok(())
    }
}

impl Info {
    fn from_file_info(fi: &FileInfo) -> Result<Info> {
        let mut pieces: Vec<PieceHash> = Vec::new();
        let chunks = fi.pieces.chunks(20);

        for chunk in chunks {
            pieces.push(PieceHash::new(chunk.to_vec()))
        }

        let hash = Self::hash(fi)?;

        Ok(Info {
            length: fi.length,
            piece_length: fi.piece_length,
            pieces,
            hash: InfoHash(hash),
        })
    }

    fn hash(fi: &FileInfo) -> Result<[u8; 20]> {
        let info_encoded = serde_bencode::to_bytes(fi).context("could not bencode info")?;

        Ok(hash(&info_encoded))
    }
}

pub struct PieceHash(Vec<u8>);

impl PartialEq for PieceHash {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl fmt::Display for PieceHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.to_hex())
    }
}

impl PieceHash {
    pub fn new(hash: Vec<u8>) -> PieceHash {
        // TODO: invariants
        PieceHash(hash)
    }

    pub fn hash(data: &Vec<u8>) -> PieceHash {
        PieceHash(hash(data).to_vec())
    }

    pub fn to_hex(&self) -> String {
        hash_to_hex(&self.0)
    }
}

fn hash_to_hex(hash: &Vec<u8>) -> String {
    hash.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn hash(data: &Vec<u8>) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let res = hasher.finalize();

    res.into()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_torrent() -> Result<(), Box<dyn std::error::Error>> {
        let path = PathBuf::from_str("sample.torrent")?;
        let torrent_file = TorrentFile::parse_from_file(&path)?;
        let torrent = Torrent::from_file_torrent(&torrent_file)?;
        println!("{}", torrent);

        Ok(())
    }
}
