use std::collections::HashMap;

use nom::{
    branch::alt,
    character::complete::{char, digit1},
    combinator::{map, map_res},
    sequence::{delimited, tuple},
    IResult, InputTakeAtPosition, multi::many0, bytes::complete::{take, take_until}, error::ErrorKind, AsChar
};

use anyhow::{anyhow, Result};

type BencodeBytes = Vec<u8>;

#[derive(Debug, Clone)]
pub enum BencodeValue {
    Bytes(BencodeBytes),
    Integer(i64),
    List(Vec<BencodeValue>),
    Dictionary(HashMap<BencodeBytes, BencodeValue>),
}

impl BencodeValue {
    pub fn as_bytes(&self) -> Result<&BencodeBytes> {
        if let BencodeValue::Bytes(bytes) = self  {
            Ok(bytes)
        } else {
            Err(anyhow!("Expected byte string, found {}", format!("{:?}", self)))
        }
    }

    pub fn as_integer(&self) -> Result<i64> {
        if let BencodeValue::Integer(int) = self {
            Ok(*int)
        } else {
            Err(anyhow!("Expected integer, found {}", format!("{:?}", self)))
        }
    }

    pub fn as_list(&self) -> Result<&Vec<BencodeValue>> {
        if let BencodeValue::List(list) = self {
            Ok(list)
        } else {
            Err(anyhow!("Expected list, found {}", format!("{:?}", self)))
        }
    }

    pub fn as_dict(&self) -> Result<&HashMap<BencodeBytes, BencodeValue>> {
        if let BencodeValue::Dictionary(dict) = self {
            Ok(dict)
        } else {
            Err(anyhow!("Expected dictionary, found {}", format!("{:?}", self)))
        }
    }

    pub fn as_str(&self) -> Result<&str> {
        let self_bytes = self.as_bytes()?;
        Ok(std::str::from_utf8(self_bytes)?)
    }

}

fn digit1_or_negative(input: &[u8]) -> IResult<&[u8], &[u8]> {
    input.split_at_position1_complete(|item| !(item.is_dec_digit() || item == b'-'), ErrorKind::Digit)
}

fn parse_integer(input: &[u8]) -> IResult<&[u8], BencodeValue> {
    map(
        delimited(
            char('i'),
            map_res(map_res(digit1_or_negative, std::str::from_utf8), str::parse),
            char('e')
        ),
        BencodeValue::Integer
    )(input)
}

fn parse_list(input: &[u8]) -> IResult<&[u8], BencodeValue> {
    map(
        delimited(
                char('l'), 
                many0(parse_bencode), 
                char('e')
        ),
        BencodeValue::List
    )(input)
}

fn parse_dictionary_pair(input: &[u8]) -> IResult<&[u8], (BencodeBytes, BencodeValue)> {
    tuple((
        parse_bytes,
        parse_bencode
    ))(input)
}

fn parse_dictionary(input: &[u8]) -> IResult<&[u8], BencodeValue> {
    map(
map(
    delimited(
                char('d'), 
                many0(parse_dictionary_pair), 
                char('e')
            ),
            |tuple_vec| {
                tuple_vec.into_iter().collect()
            }
        ),
        BencodeValue::Dictionary
    )(input)

}

fn parse_bytes(input: &[u8]) -> IResult<&[u8], BencodeBytes> {
    let (rest, byte_string_len): (&[u8], u64) = map_res(map_res(digit1, std::str::from_utf8), str::parse)(input)?;
    let (rest, _) = char(':')(rest)?;
    let (rest, bytes) = take(byte_string_len)(rest)?;

    Ok((rest, bytes.to_vec()))
}

pub fn parse_bencode(input: &[u8]) -> IResult<&[u8], BencodeValue> {
    alt((
        parse_integer,
        parse_list,
        parse_dictionary,
        map(parse_bytes, BencodeValue::Bytes),
    ))(input)
}

// The info_hash of a metainfo file is defined as the sha1 hash of the raw value of the "info" key of the file
// Given nom doesn't give us a way to return the byte range (without using something like nom_locate), and the alternative
// is writing a bencode serializer (which no other part of the protocol requires), this parse function allows us to return the
// raw byte representation of the info dictionary
// TODO: this assumes the first time the bytestring "info" appears is as the key of the info dict. Replace with something a bit more robust.
pub fn parse_info_dict_raw(input: &[u8]) -> IResult<&[u8], &[u8]> {
    let (remaining, _) = take_until("4:info".as_bytes())(input)?;
    let (remaining, _) = take(6usize)(remaining)?;

    let (remaining_after_info_dict, _) = parse_dictionary(remaining)?;
    let info_dict_length = remaining.len() - remaining_after_info_dict.len();

    Ok((remaining_after_info_dict, &remaining[0..info_dict_length]))
}