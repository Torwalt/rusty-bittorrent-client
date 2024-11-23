use std::path::PathBuf;

use anyhow::Result;
use bencode::decode;
use clap::Parser;
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

#[derive(Parser)]
enum Commands {
    Decode {
        input: String,
    },
    Info {
        torrent_path: PathBuf,
    },
    Peers {
        torrent_path: PathBuf,
    },
    Handshake {
        torrent_path: PathBuf,
        #[arg(value_parser = clap::value_parser!(tracker::Peer))]
        peer: tracker::Peer,
    },
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
        Some(Commands::Handshake { torrent_path, peer }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            let client = Client::new()?;
            let handshake = client.perform_handshake(peer, torrent.to_request())?;
            println!("{}", handshake)
        },
        None => {}
    };

    Ok(())
}
