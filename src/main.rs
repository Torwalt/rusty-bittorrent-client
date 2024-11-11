use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use url::{self, Url};

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use decode::{decode, ParsedValue};

mod decode;

const TORRENT_META_URL_KEY: &str = "announce";
const TORRENT_META_INFO_KEY: &str = "info";
const TORRENT_INFO_LENGTH_KEY: &str = "length";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Decode { input: String },
    Info { torrent_path: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Decode { input }) => {
            let parsed_value = decode(input)?;
            println!("{}", parsed_value.value)
        }
        Some(Commands::Info { torrent_path }) => {
            let raw_meta = parse_torrent_metadata(torrent_path)?;
            println!("{}", raw_meta);
            let parsed = decode(&raw_meta)?;
            println!("{}", parsed.value);
            let meta = Meta::parse(&parsed)?;
            println!("{}", meta);
        }
        None => {}
    };

    Ok(())
}

fn parse_torrent_metadata(torrent_path: &PathBuf) -> Result<String> {
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
    let split_index = match utf8_content.rfind("6:pieces") {
        Some(idx) => idx,
        None => bail!("expected string 6:pieces missing from info"),
    };

    let main_part = match utf8_content.get_mut(..split_index) {
        Some(rest) => rest,
        None => bail!("could not split by index"),
    };

    // Add arbitrary value for pieces to make string parsable.
    let mut info = main_part.to_string();
    info.push_str("6:pieces1:aee");

    Ok(info)
}

struct Meta {
    tracker_url: Url,
    length: u64,
}

impl fmt::Display for Meta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Tracker URL: {}", self.tracker_url.as_str())?;
        writeln!(f, "Length: {}", self.length)
    }
}

impl Meta {
    fn parse(parsed_metadata: &ParsedValue) -> Result<Meta> {
        let meta = parsed_metadata
            .value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("expected a dict"))?;

        let raw_url = meta
            .get(TORRENT_META_URL_KEY)
            .ok_or_else(|| anyhow::anyhow!("expected key {TORRENT_META_URL_KEY} to be present"))?
            .as_str()
            .ok_or_else(|| {
                anyhow::anyhow!("expected value for key {TORRENT_META_URL_KEY} to be string")
            })?;

        let url = Url::parse(raw_url)?;

        let len = meta
            .get(TORRENT_META_INFO_KEY)
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                anyhow::anyhow!("missing or invalid object for key: {TORRENT_META_INFO_KEY}")
            })?
            .get(TORRENT_INFO_LENGTH_KEY)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                anyhow::anyhow!("expected u64 value for key: {TORRENT_INFO_LENGTH_KEY}")
            })?;

        return Ok(Meta {
            tracker_url: url,
            length: len,
        });
    }
}
