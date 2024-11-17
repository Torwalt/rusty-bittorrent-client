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
    metadata: FileMeta,
}

impl TorrentFile {
    pub fn parse_from_file(torrent_path: &PathBuf) -> Result<TorrentFile> {
        let mut file = File::open(torrent_path)?;

        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        Self::parse(content)
    }

    fn parse(content: Vec<u8>) -> Result<TorrentFile> {
        let meta: FileMeta =
            serde_bencode::from_bytes(&content).context("could not parse content into Meta")?;

        Ok(TorrentFile { metadata: meta })
    }
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
        let parsed_url = url::Url::parse(&tf.metadata.tracker_url)?;
        let info = Info::from_file_info(&tf.metadata.info)?;

        Ok(Torrent {
            tracker_url: parsed_url,
            created_by: tf.metadata.created_by.clone(),
            info,
        })
    }
}

struct Info {
    length: usize,
    name: String,
    piece_length: usize,
    pieces: Vec<Piece>,
    hash: Vec<u8>,
}

impl fmt::Display for Info {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Length: {}", self.length)?;
        writeln!(f, "Info Hash {}", hash_to_hex(&self.hash))?;
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
            hash,
        })
    }

    fn hash(fi: &FileInfo) -> Result<Vec<u8>> {
        let info_encoded = serde_bencode::to_bytes(fi).context("could not bencode info")?;

        let mut hasher = Sha1::new();
        hasher.update(info_encoded);
        let res = hasher.finalize();

        Ok(res.to_vec())
    }
}

struct Piece {
    hash: Vec<u8>,
}

fn hash_to_hex(hash: &Vec<u8>) -> String {
    hash.iter().map(|byte| format!("{:02x}", byte)).collect()
}

impl fmt::Display for Piece {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", hash_to_hex(&self.hash))
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct FileMeta {
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
