use serde::Serialize;
use serde_json::{Map, Value};
use sha1_smol::Sha1;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use url::{self, Url};

use anyhow::{bail, Result};
use bencode::ParsedValue;

use crate::bencode;

const META_URL_KEY: &str = "announce";
const META_INFO_KEY: &str = "info";
const INFO_LENGTH_KEY: &str = "length";
const INFO_NAME_KEY: &str = "name";
const INFO_PIECE_LENGTH_KEY: &str = "piece length";

pub struct TorrentFile {
    pub metadata: String,
    pub pieces_hashes: Vec<u8>,
}

impl fmt::Display for TorrentFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.metadata)
    }
}

impl TorrentFile {
    pub fn parse(torrent_path: &PathBuf) -> Result<TorrentFile> {
        let file = File::open(torrent_path)?;
        let mut reader = BufReader::new(file);
        let mut utf8_content = String::new();
        // We might not need the data, lets see.
        let mut data_content = Vec::new();
        let mut buffer = [0; 1024];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            // Convert if possible content in buffer to UTF-8.
            match std::str::from_utf8(&buffer[..bytes_read]) {
                Ok(valid_str) => utf8_content.push_str(valid_str),
                Err(error) => {
                    // Get the valid portion before the error and add it.
                    let valid_up_to = error.valid_up_to();
                    utf8_content.push_str(std::str::from_utf8(&buffer[..valid_up_to])?);

                    // Read the rest of the non UTF-8 data from buffer.
                    data_content.extend_from_slice(&buffer[valid_up_to..bytes_read]);

                    // Continue reading rest of file.
                    loop {
                        match reader.read(&mut buffer)? {
                            0 => break,
                            bytes_read => {
                                data_content.extend_from_slice(&buffer[..bytes_read]);
                            }
                        }
                    }
                }
            }
        }

        // Thats kinda hacky, but decode::decode expects a correct dict, so we need to remove the
        // variable <length>: after 'pieces' and add a small value and the ending token.
        // This is 'viable' because the dict keys are in lexigraphical ordering, so pieces is
        // always the last key, so cutting off in that manner is ok.
        let split_index = match utf8_content.rfind("6:pieces") {
            Some(idx) => idx,
            None => bail!("expected string 6:pieces missing from info"),
        };

        let main_part = match utf8_content.get_mut(..split_index) {
            Some(rest) => rest,
            None => bail!("could not split by index"),
        };

        // Add arbitrary value for pieces to make string parsable.
        let mut metadata = main_part.to_string();
        metadata.push_str("6:pieces1:aee");

        Ok(TorrentFile {
            pieces_hashes: data_content,
            metadata,
        })
    }
}

pub struct Meta {
    tracker_url: Url,
    info: Info,
}

#[derive(Serialize, PartialEq, Eq, Debug)]
struct Info {
    length: u64,
    name: String,
    piece_length: u64,
    pieces_data: Vec<u8>,
}

fn info_hash(info: &Info) -> Result<String> {
    let mut hasher = Sha1::new();
    let info_bencoded = serde_bencode::to_string(&info)?;
    println!("{}", info_bencoded);

    hasher.update(info_bencoded.as_bytes());
    Ok(hasher.digest().to_string())
}

impl Info {
    fn parse(decoded_info: &Map<String, Value>, pieces_data: Vec<u8>) -> Result<Info> {
        let info_raw = decoded_info
            .get(META_INFO_KEY)
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("missing or invalid object for key: {META_INFO_KEY}"))?;

        let len = info_raw
            .get(INFO_LENGTH_KEY)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("expected u64 value for key: {INFO_LENGTH_KEY}"))?;

        let name = info_raw
            .get(INFO_NAME_KEY)
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("expected string value for key: {INFO_NAME_KEY}"))?
            .to_string();

        let piece_length = info_raw
            .get(INFO_PIECE_LENGTH_KEY)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                anyhow::anyhow!("expected u64 value for key: {INFO_PIECE_LENGTH_KEY}")
            })?;

        Ok(Info {
            length: len,
            name,
            piece_length,
            pieces_data,
        })
    }
}

impl fmt::Display for Meta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tracker URL: {}", self.tracker_url.as_str())?;
        writeln!(f, "Length: {}", self.info.length)
    }
}

impl Meta {
    pub fn parse(decoded_metadata: &ParsedValue, pieces_data: Vec<u8>) -> Result<Meta> {
        let meta = decoded_metadata
            .value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("expected a dict"))?;

        let raw_url = meta
            .get(META_URL_KEY)
            .ok_or_else(|| anyhow::anyhow!("expected key {META_URL_KEY} to be present"))?
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("expected value for key {META_URL_KEY} to be string"))?;

        let url = Url::parse(raw_url)?;

        let info = Info::parse(meta, pieces_data)?;

        return Ok(Meta {
            tracker_url: url,
            info,
        });
    }

    pub fn info_hash(&self) -> Result<String> {
        info_hash(&self.info)
    }
}
