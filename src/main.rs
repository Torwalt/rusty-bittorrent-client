use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

const BENCODE_STRING_SPLIT_CHAR: char = ':';
const BENCODE_INT_PREFIX: char = 'i';
const BENCODE_INT_SUFFIX: char = 'e';
const BENCODE_LIST_PREFIX: char = 'l';
const BENCODE_LIST_SUFFIX: char = 'e';

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

struct ParsedValue {
    length: usize,
    value: serde_json::Value,
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
            _ if input.starts_with(BENCODE_INT_PREFIX) && input.ends_with(BENCODE_INT_SUFFIX) => {
                BencodeType::Number
            }
            _ if input.starts_with(BENCODE_LIST_PREFIX) && input.ends_with(BENCODE_LIST_SUFFIX) => {
                BencodeType::List
            }
            _ if input.contains(BENCODE_STRING_SPLIT_CHAR) => BencodeType::String,
            _ => BencodeType::Invalid,
        }
    }

    fn identify_char(input: &char) -> BencodeType {
        match input {
            _ if input.is_numeric() => BencodeType::String,
            _ if *input == BENCODE_INT_PREFIX => BencodeType::Number,
            _ if *input == BENCODE_LIST_PREFIX => BencodeType::List,
            _ => BencodeType::Invalid,
        }
    }
}

fn decode(input: &String) -> Result<serde_json::Value> {
    let bencode_type = BencodeType::new(input);
    let res = match bencode_type {
        BencodeType::String => bdecode_string(input)?,
        BencodeType::Number => bdecode_num(input)?,
        BencodeType::List => bdecode_list(input)?,
        BencodeType::Invalid => bail!("dont know how to handle {input}"),
    };

    Ok(res.value)
}

fn bdecode_list(input: &str) -> Result<ParsedValue> {
    // encoded like l5:helloi52ee
    let mut elements = &input[1..input.len() - 1];

    let mut list: Vec<serde_json::Value> = Vec::new();

    let mut elem_iter = elements.chars();
    let mut step = 0;
    loop {
        let char = match elem_iter.nth(step) {
            Some(x) => x,
            None => break,
        };

        let next_type = BencodeType::identify_char(&char);

        let res = match next_type {
            BencodeType::String => bdecode_string(elements)?,
            BencodeType::Number => bdecode_num(elements)?,
            BencodeType::List => bdecode_list(elements)?,
            BencodeType::Invalid => bail!("dont know how to handle {input}"),
        };

        list.push(res.value);
        step = res.length - 1;
        elements = match elements.get(res.length..elements.len()) {
            Some(slice) => slice,
            None => break,
        };
    }

    Ok(ParsedValue {
        length: input.len(),
        value: serde_json::to_value(&list)?,
    })
}

fn bdecode_string(input: &str) -> Result<ParsedValue> {
    // encoded like <length:contents>
    let mut split = input.splitn(2, BENCODE_STRING_SPLIT_CHAR);

    let length_string = split.next().ok_or_else(|| {
        anyhow::anyhow!("length missing before {BENCODE_STRING_SPLIT_CHAR}: input: {input}")
    })?;

    let length = length_string
        .parse()
        .context(format!("parsing length in encoded string {input}"))?;

    let contents = split.next().ok_or_else(|| {
        anyhow::anyhow!("content missing after {BENCODE_STRING_SPLIT_CHAR}: input: {input}")
    })?;

    let relevant_content = match contents.get(0..length) {
        Some(slice) => slice,
        None => bail!(
            "incorrect length encoding! Expected {length} characters, but have these {contents}"
        ),
    };

    let len = relevant_content.len() + length_string.len() + 1;

    Ok(ParsedValue {
        length: len,
        value: serde_json::to_value(&relevant_content)?,
    })
}

fn bdecode_num(input: &str) -> Result<ParsedValue> {
    // endcoded like i<number>e. Number can be negative.

    let mut input_chars = input.chars();
    let mut num_string = String::new();
    let mut signed_seen = false;

    while let Some(ch) = input_chars.next() {
        match ch {
            BENCODE_INT_PREFIX => continue,
            BENCODE_INT_SUFFIX => break,
            _ if ch.is_numeric() || (ch == '-' && !signed_seen) => {
                num_string.push(ch);
                signed_seen = true;
                continue;
            }
            _ => bail!("unexpected char '{ch}' in number input '{input}'"),
        }
    }

    let num: i64 = num_string
        .parse()
        .context(format!("cannot parse {num_string} as i64"))?;

    let len = num_string.len() + 2; // prefix and suffix was stripped.

    Ok(ParsedValue {
        length: len,
        value: serde_json::to_value(&num)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bdecode_list() -> Result<(), Box<dyn std::error::Error>> {
        struct TestCase {
            input: String,
            expected: serde_json::Value,
        }

        let test_cases = vec![
            TestCase {
                input: String::from("l5:helloi52ee"),
                expected: serde_json::json!(["hello", 52]),
            },
            TestCase {
                input: String::from("l5:helloi52ei43ee"),
                expected: serde_json::json!(["hello", 52, 43]),
            },
            TestCase {
                input: String::from("l5:helloi52ei43e4:adade"),
                expected: serde_json::json!(["hello", 52, 43, "adad"]),
            },
        ];

        for test_case in test_cases {
            let decoded = bdecode_list(&test_case.input)?;
            assert_eq!(test_case.expected, decoded.value);
        }

        Ok(())
    }
}
