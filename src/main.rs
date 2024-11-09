use clap::{Parser, Subcommand};
use anyhow::Result;

const BENCODE_SPLIT_CHAR: &str = ":";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Decode{input: String}
}

fn main() -> Result<()>{
    let cli = Cli::parse();

    match &cli.command {
    Some(Commands::Decode { input }) => {
        decode_command(input)?
    }
    None => {}
    };

    Ok(())
}

fn decode_command(input: &String) -> Result<()>{
    let decoded_json = bdecode_string(input)?;
    println!("{}", decoded_json );
    Ok(())
}

fn bdecode_string(input: &String) -> Result<String> {
    // encoded like <length:contents>
    let mut split = input.splitn(2, BENCODE_SPLIT_CHAR);

    let _ = split.next().ok_or_else(|| anyhow::anyhow!("length missing before {BENCODE_SPLIT_CHAR}: input: {input}"))?;
    let contents = split.next().ok_or_else(|| anyhow::anyhow!("content missing after {BENCODE_SPLIT_CHAR}: input: {input}"))?;


    Ok(serde_json::to_string(contents)?)
}

