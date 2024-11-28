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
    pub pieces: &'a [Piece],
    // TODO: Should be static.
    pub info_hash: &'a InfoHash,
}

pub struct Torrent {
    tracker_url: Url,
    created_by: String,
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
            created_by: tf.created_by.clone(),
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

pub struct InfoHash(pub [u8; 20]);

struct Info {
    length: usize,
    name: String,
    piece_length: usize,
    pieces: Vec<Piece>,
    hash: InfoHash,
}

impl fmt::Display for Info {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Length: {}", self.length)?;
        writeln!(f, "Info Hash {}", hash_to_hex(&self.hash.0))?;
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
        let mut pieces: Vec<Piece> = Vec::new();
        let chunks = fi.pieces.chunks(20);

        for chunk in chunks {
            pieces.push(Piece {
                hash: chunk.to_vec(),
            })
        }

        let hash = Self::hash(fi)?;

        Ok(Info {
            length: fi.length,
            name: fi.name.clone(),
            piece_length: fi.piece_length,
            pieces,
            hash: InfoHash(hash),
        })
    }

    fn hash(fi: &FileInfo) -> Result<[u8; 20]> {
        let info_encoded = serde_bencode::to_bytes(fi).context("could not bencode info")?;

        let mut hasher = Sha1::new();
        hasher.update(info_encoded);
        let res = hasher.finalize();

        Ok(res.into())
    }
}

pub struct Piece {
    pub hash: Vec<u8>,
}

impl fmt::Display for Piece {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", hash_to_hex(&self.hash))
    }
}

fn hash_to_hex(hash: &[u8]) -> String {
    hash.iter().map(|byte| format!("{:02x}", byte)).collect()
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
