use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Bytes};
use sha1::{Digest, Sha1};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct TorrentFile {
    pub metadata: Meta,
}

impl fmt::Display for TorrentFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.metadata)
    }
}

impl TorrentFile {
    pub fn parse(torrent_path: &PathBuf) -> Result<TorrentFile> {
        let mut file = File::open(torrent_path)?;

        let mut content = Vec::new();
        file.read_to_end(&mut content)?;

        let meta: Meta =
            serde_bencode::from_bytes(&content).context("could not parse content into Meta")?;

        Ok(TorrentFile { metadata: meta })
    }

    pub fn info_hash(&self) -> Result<String> {
        let info_encoded =
            serde_bencode::to_bytes(&self.metadata.info).context("could not bencode info")?;

        let mut hasher = Sha1::new();
        hasher.update(info_encoded);
        let res = hasher.finalize();

        Ok(res.iter().map(|byte| format!("{:02x}", byte)).collect())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Meta {
    #[serde(rename = "announce")]
    tracker_url: String,
    #[serde(rename = "created by")]
    created_by: String,
    info: Info,
}

#[serde_as]
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct Info {
    length: u64,
    name: String,
    #[serde(rename = "piece length")]
    piece_length: u64,
    #[serde_as(as = "Bytes")]
    pieces: Vec<u8>,
}

impl fmt::Display for Meta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tracker URL: {}", self.tracker_url.as_str())?;
        writeln!(f, "Length: {}", self.info.length)
    }
}
