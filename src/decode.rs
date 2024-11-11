use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

const BENCODE_END: char = 'e';

const BENCODE_STRING_SPLIT_CHAR: char = ':';

const BENCODE_INT_PREFIX: char = 'i';
const BENCODE_INT_SUFFIX: char = BENCODE_END;

const BENCODE_LIST_PREFIX: char = 'l';
const BENCODE_LIST_SUFFIX: char = BENCODE_END;

const BENCODE_DICT_PREFIX: char = 'd';
const BENCODE_DICT_SUFFIX: char = BENCODE_END;

pub(crate) struct ParsedValue {
    length: usize,
    pub value: Value,
}

enum BencodeType {
    String,
    Number,
    List,
    Dictionary,
    Invalid,
}

impl BencodeType {
    fn new(input: &char) -> BencodeType {
        match input {
            _ if input.is_numeric() => BencodeType::String,
            _ if *input == BENCODE_INT_PREFIX => BencodeType::Number,
            _ if *input == BENCODE_LIST_PREFIX => BencodeType::List,
            _ if *input == BENCODE_DICT_PREFIX => BencodeType::Dictionary,
            _ => BencodeType::Invalid,
        }
    }
}

pub(crate) fn decode(input: &str) -> Result<ParsedValue> {
    let bencode_type = BencodeType::new(
        &input
            .chars()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty input"))?,
    );
    let res = match bencode_type {
        BencodeType::String => bdecode_string(input)?,
        BencodeType::Number => bdecode_num(input)?,
        BencodeType::List => bdecode_list(input)?,
        BencodeType::Dictionary => bdecode_dict(input)?,
        BencodeType::Invalid => bail!("dont know how to handle {input}"),
    };

    Ok(res)
}

fn bdecode_dict(input: &str) -> Result<ParsedValue> {
    // encoded like d3:foo3:bar5:helloi52ee -> {"hello": 52, "foo":"bar"}
    let mut map = Map::new();
    let mut dict_len = 2;

    let mut char_iter = input.chars().enumerate().peekable();
    loop {
        let (idx, peeked_char) = match char_iter.peek() {
            Some(x) => x,
            None => bail!("unexpected iterator end, expected peekable char (this means end token was skipped)"),
        };

        match *peeked_char {
            BENCODE_DICT_SUFFIX => break,
            BENCODE_DICT_PREFIX => {
                // Make sure to consume identifying char.
                char_iter.next();
                continue;
            }
            _ => {}
        }

        let rest = match input.get(*idx..input.len()) {
            Some(rest) => rest,
            None => bail!("unexpected end of input, expected {idx} to be in range"),
        };

        // First parse the key. It must be a string, so fail if it is not.
        let key =
            bdecode_string(rest).context("expected string to be present as key in dict {rest}")?;

        // Consume up until step, so next peek is the char identifying the value.
        let step = key.length - 1;
        dict_len += key.length;
        match char_iter.nth(step) {
            Some(x) => x,
            None => bail!("expected value to follow a key in dict iterator {rest}"),
        };

        let (idx, peeked_char) = match char_iter.peek() {
            Some(x) => x,
            None => bail!("unexpected iterator end, expected value {rest}"),
        };

        match *peeked_char {
            BENCODE_DICT_SUFFIX => bail!("unexpected end token in dict, expected value {rest}"),
            _ => {}
        }

        let rest = match input.get(*idx..input.len()) {
            Some(rest) => rest,
            None => bail!("unexpected end of input"),
        };

        let value = decode(rest)?;

        // Interesting interface: Returns Option, None if new, Old value if update.
        // NOTE: Thats kinda shitty. We need to allocate and cast back and forth to prevent string
        // escaping.
        map.insert(
            key.value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("empty input"))?
                .to_string(),
            value.value,
        );

        // Consume again until step, but this time the value.
        let step = value.length - 1;
        dict_len += value.length;
        match char_iter.nth(step) {
            Some(x) => x,
            None => bail!("unexpected end in value"),
        };
    }

    Ok(ParsedValue {
        length: dict_len,
        value: Value::Object(map),
    })
}

fn bdecode_list(input: &str) -> Result<ParsedValue> {
    // encoded like l5:helloi52ee
    let mut list: Vec<Value> = Vec::new();
    let mut list_len = 2;

    let mut char_iter = input.chars().enumerate().peekable();
    loop {
        let (idx, peeked_char) = match char_iter.peek() {
            Some(char) => char,
            None => break,
        };

        match *peeked_char {
            BENCODE_LIST_SUFFIX => break,
            BENCODE_LIST_PREFIX => {
                // Make sure to consume identifying char.
                char_iter.next();
                continue;
            }
            _ => {}
        }

        let rest = match input.get(*idx..input.len()) {
            Some(rest) => rest,
            None => break,
        };

        let parsed_value = decode(rest)?;
        list.push(parsed_value.value);

        // Consume iter up to step, as that part was already processed.
        let step = parsed_value.length - 1;
        list_len += parsed_value.length;
        match char_iter.nth(step) {
            Some(_) => {}
            None => bail!("unexpected end of iter"),
        };
    }

    Ok(ParsedValue {
        length: list_len,
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

    let len = num_string.len() + 2; // + prefix and suffix

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
            expected: Value,
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
            TestCase {
                input: String::from("l5:helloi52ei43e4:adadd3:foo3:bar5:helloi52eee"),
                expected: serde_json::json!(["hello", 52, 43, "adad", {"hello": 52, "foo":"bar"}]),
            },
            TestCase {
                input: String::from("l5:helloi52ei43e4:adadd3:foo3:bar5:helloi52eei1337ee"),
                expected: serde_json::json!(["hello", 52, 43, "adad", {"hello": 52, "foo":"bar"}, 1337]),
            },
        ];

        for test_case in test_cases {
            let decoded = bdecode_list(&test_case.input)?;
            assert_eq!(test_case.expected, decoded.value);
        }

        Ok(())
    }

    #[test]
    fn test_bdecode_dict() -> Result<(), Box<dyn std::error::Error>> {
        struct TestCase {
            input: String,
            expected: Value,
        }

        let test_cases = vec![
            TestCase {
                input: String::from("d3:foo3:bar5:helloi52ee"),
                expected: serde_json::json!({"hello": 52, "foo":"bar"}),
            },
            TestCase {
                input: String::from("d3:foo3:bar5:helloi52e4:listl5:helloi52ee2:asi1337ee"),
                expected: serde_json::json!(
                    {"hello": 52, "foo":"bar", "list": ["hello", 52], "as": 1337}
                ),
            },
            TestCase {
                input: String::from("d8:announce55:http://bittorrent-test-tracker.codecrafters.io/announce10:created by13:mktorrent 1.14:infod6:lengthi92063e4:name10:sample.txt12:piece lengthi32768e6:pieces1:aee"),
                expected: serde_json::json!(
                    {"announce": "http://bittorrent-test-tracker.codecrafters.io/announce", "created by": "mktorrent 1.1", "info": {"length": 92063, "name": "sample.txt", "piece length": 32768, "pieces": "a"}}
                ),
            },
        ];

        for test_case in test_cases {
            let decoded = bdecode_dict(&test_case.input)?;
            assert_eq!(test_case.expected, decoded.value);
        }

        Ok(())
    }
}
