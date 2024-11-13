use std::path::PathBuf;

use anyhow::Result;
use bencode::decode;
use clap::{Parser, Subcommand};
use torrent::{parse_torrent_metadata, Meta};

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
