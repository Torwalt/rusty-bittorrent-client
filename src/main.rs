use std::fs;
use std::io::Write; // bring trait into scope
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use bencode::decode;
use clap::Parser;
use torrent::TorrentFile;

use self::torrent::Torrent;

mod bencode;
mod peers;
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
        #[arg(value_parser = clap::value_parser!(peers::Peer))]
        peer: peers::Peer,
    },
    #[command(alias = "download_piece")]
    DownloadPiece {
        #[arg(short, long, required = true)]
        output_path: PathBuf,
        #[arg(required = true)]
        torrent_path: PathBuf,
        #[arg(required = true)]
        piece_index: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::init();

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
            let id = peers::PeerID::new();
            let client = peers::Client::new(id)?;
            let peers = client.find_peers(torrent.to_peer_request()).await?;
            println!("{}", peers)
        }
        Some(Commands::Handshake { torrent_path, peer }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            let id = peers::PeerID::new();
            let client = tracker::Client::new(id)?;
            let handshake = client
                .perform_handshake(peer, &torrent.to_peer_request().info_hash)
                .await?;
            println!("{}", handshake)
        }
        Some(Commands::DownloadPiece {
            torrent_path,
            output_path,
            piece_index,
        }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            let id = peers::PeerID::new();

            let peer_client = peers::Client::new(id.clone())?;
            let tracker_client = tracker::Client::new(id.clone())?;

            let peers = peer_client.find_peers(torrent.to_peer_request()).await?;
            let peer = peers
                .iter()
                .next()
                .ok_or(anyhow!("no peers found in torrent file"))?;

            let download_req = torrent.to_download_request();
            let piece_data = tracker_client
                .download_piece(peer, download_req, piece_index.clone().try_into()?)
                .await?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(output_path)?;
            file.write_all(&piece_data)?;
        }
        None => {}
    };

    Ok(())
}
