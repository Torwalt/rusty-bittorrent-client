use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

const BENCODE_STRING_SPLIT_CHAR: &str = ":";
const BENCODE_INT_PREFIX: &str = "i";
const BENCODE_INT_SUFIX: &str = "e";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Decode { input: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Decode { input }) => decode(input)?,
        None => {}
    };

    Ok(())
}

fn decode(input: &String) -> Result<()> {
    let res = match input {
        _ if input.contains(BENCODE_STRING_SPLIT_CHAR) => bdecode_string(input)?,
        _ if input.starts_with("i") && input.ends_with("e") => bdecode_int(input)?,
        _ => bail!("cannot decode unknown format {input}"),
    };

    println!("{}", res);
    Ok(())
}

fn bdecode_string(input: &String) -> Result<String> {
    // encoded like <length:contents>
    let mut split = input.splitn(2, BENCODE_STRING_SPLIT_CHAR);

    let _ = split.next().ok_or_else(|| {
        anyhow::anyhow!("length missing before {BENCODE_STRING_SPLIT_CHAR}: input: {input}")
    })?;
    let contents = split.next().ok_or_else(|| {
        anyhow::anyhow!("content missing after {BENCODE_STRING_SPLIT_CHAR}: input: {input}")
    })?;

    Ok(serde_json::to_string(contents)?)
}

fn bdecode_int(input: &String) -> Result<String> {
    // endcoded like i<number>e. Number can be negative.
    let num_string = input
        .strip_prefix(BENCODE_INT_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("expected prefix {BENCODE_INT_PREFIX} missing"))?
        .strip_suffix(BENCODE_INT_SUFIX)
        .ok_or_else(|| anyhow::anyhow!("expected suffix {BENCODE_INT_SUFIX} missing"))?;

    let num: i64 = num_string
        .parse()
        .context(format!("cannot parse {num_string} as i64"))?;

    Ok(serde_json::to_string(&num)?)
}
