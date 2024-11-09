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
        Some(Commands::Decode { input }) => {
            let decoded_json_value = decode(input)?;
            println!("{}", decoded_json_value)
        }
        None => {}
    };

    Ok(())
}

enum BencodeType {
    String,
    Number,
    List,
    Invalid,
}

impl BencodeType {
    fn new(input: &String) -> BencodeType {
        match input {
            _ if input.contains(BENCODE_STRING_SPLIT_CHAR) => BencodeType::String,
            _ if input.starts_with("i") && input.ends_with("e") => BencodeType::Number,
            _ if input.starts_with("l") && input.ends_with("e") => BencodeType::List,
            _ => BencodeType::Invalid,
        }
    }
}

fn decode(input: &String) -> Result<String> {
    let bencode_type = BencodeType::new(input);
    let res = match bencode_type {
        BencodeType::String => bdecode_string(input)?,
        BencodeType::Number => bdecode_num(input)?,
        BencodeType::List => bdecode_list(input)?,
        BencodeType::Invalid => bail!("dont know how to handle {input}")
    };

    Ok(res)
}

fn bdecode_list(input: &String) -> Result<String> {
    todo!()
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

fn bdecode_num(input: &String) -> Result<String> {
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
