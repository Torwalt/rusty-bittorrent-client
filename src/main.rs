use std::fs;
use std::io::Write; // bring trait into scope
use std::path::PathBuf;

use anyhow::{anyhow, Result};
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
            let peers = client.find_peers(torrent.to_peer_request())?;
            println!("{}", peers)
        }
        Some(Commands::Handshake { torrent_path, peer }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            let client = Client::new()?;
            let handshake = client.perform_handshake(peer, &torrent.to_peer_request().info_hash)?;
            println!("{}", handshake)
        }
        Some(Commands::DownloadPiece {
            torrent_path,
            output_path,
            piece_index,
        }) => {
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            let client = Client::new()?;
            let peers = client.find_peers(torrent.to_peer_request())?;
            let peer = peers
                .iter()
                .next()
                .ok_or(anyhow!("no peers found in torrent file"))?;
            let download_req = torrent.to_download_request();
            let piece_data = client.download_piece(peer, download_req, piece_index.clone())?;
            let mut file = fs::OpenOptions::new().write(true).open(output_path)?;
            file.write_all(&piece_data)?;
        }
        None => {}
    };

    Ok(())
}
