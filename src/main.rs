use std::path::PathBuf;

use anyhow::Result;
use bencode::decode;
use clap::{Parser, Subcommand};
use torrent::TorrentFile;

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
            let torrent_file = TorrentFile::parse(torrent_path)?;
            print!("{}", torrent_file);
            let info_hash = torrent_file.info_hash()?;
            println!("{}", info_hash)
        }
        None => {}
    };

    Ok(())
}
