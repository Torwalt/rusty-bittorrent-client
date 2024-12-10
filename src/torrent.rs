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
    pub info_hash: &'a Hash,
    pub length: usize,
}

pub struct DownloadRequest<'a> {
    pub length: usize,
    pub piece_length: usize,
    pub pieces: &'a [Hash],
    // TODO: Should be static.
    pub info_hash: &'a Hash,
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

struct Info {
    length: usize,
    piece_length: usize,
    pieces: Vec<Hash>,
    hash: Hash,
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
        let mut pieces: Vec<Hash> = Vec::new();
        let chunks = fi.pieces.chunks(20);

        for chunk in chunks {
            pieces.push(Hash::new(
                chunk
                    .try_into()
                    .context("expected to cast chunk of 20 into array of 20")?,
            ))
        }

        let hash = Self::hash(fi)?;

        Ok(Info {
            length: fi.length,
            piece_length: fi.piece_length,
            pieces,
            hash,
        })
    }

    fn hash(fi: &FileInfo) -> Result<Hash> {
        let info_encoded = serde_bencode::to_bytes(fi).context("could not bencode info")?;

        Ok(Hash::hash(&info_encoded))
    }
}

pub struct Hash([u8; 20]);

impl Clone for Hash {
    fn clone(&self) -> Hash {
        Hash(self.0.clone())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.to_hex())
    }
}

impl PartialEq for Hash {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Hash {
    pub fn new(hash: [u8; 20]) -> Hash {
        Hash(hash)
    }

    pub fn get_hash(&self) -> &[u8; 20] {
        &self.0
    }

    pub fn hash(data: &Vec<u8>) -> Hash {
        let mut hasher = Sha1::new();
        hasher.update(data);
        let res = hasher.finalize();
        Hash(res.into())
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{:02x}", byte)).collect()
    }
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
