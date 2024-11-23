use std::path::PathBuf;

use anyhow::Result;
use bencode::decode;
use clap::{Parser, Subcommand};
use torrent::TorrentFile;

use self::torrent::Torrent;
use self::tracker::Client;

mod bencode;
mod torrent;
mod tracker;

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
    Peers { torrent_path: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Decode { input }) => {
            let parsed_value = decode(input)?;
            println!("{}", parsed_value.value)
        }
        Some(Commands::Info { torrent_path }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            println!("{}", torrent)
        }
        Some(Commands::Peers { torrent_path }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            let client = Client::new()?;
            let peers = client.find_peers(torrent.to_request())?;
            println!("{}", peers)
        }
        None => {}
    };

    Ok(())
}
