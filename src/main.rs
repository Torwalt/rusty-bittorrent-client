use std::path::PathBuf;

use anyhow::Result;
use bencode::decode;
use clap::{Parser, Subcommand};
use torrent::TorrentFile;

use self::torrent::Torrent;

mod bencode;
mod torrent;

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
            let torrent_file = TorrentFile::parse_from_file(torrent_path)?;
            let torrent = Torrent::from_file_torrent(&torrent_file)?;
            println!("{}", torrent)
        }
        None => {}
    };

    Ok(())
}
