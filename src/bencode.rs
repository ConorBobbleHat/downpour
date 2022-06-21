use std::collections::HashMap;

use nom::{
    branch::alt,
    character::complete::{char, digit1},
    combinator::{map, map_res},
    sequence::{delimited, tuple},
    IResult, InputTakeAtPosition, multi::many0, bytes::complete::take, error::ErrorKind, AsChar
};

type BencodeBytes = Vec<u8>;

#[derive(Debug)]
pub enum BencodeValue {
    Bytes(BencodeBytes),
    Integer(i64),
    List(Vec<BencodeValue>),
    Dictionary(HashMap<BencodeBytes, BencodeValue>),
}

pub fn digit1_or_negative(input: & [u8]) -> IResult<& [u8], & [u8]> {
    input.split_at_position1_complete(|item| !(item.is_dec_digit() || item == ('-' as u8)), ErrorKind::Digit)
}

pub fn parse_integer(input: & [u8]) -> IResult<& [u8], BencodeValue> {
    map(
        delimited(
            char('i'),
            map_res(map_res(digit1_or_negative, std::str::from_utf8), str::parse),
            char('e')
        ),
        BencodeValue::Integer
    )(input)
}

pub fn parse_list(input: & [u8]) -> IResult<& [u8], BencodeValue> {
    map(
        delimited(
                char('l'), 
                many0(parse_bencode), 
                char('e')
        ),
        BencodeValue::List
    )(input)
}

pub fn parse_dictionary_pair(input: & [u8]) -> IResult<& [u8], (BencodeBytes, BencodeValue)> {
    tuple((
        parse_bytes,
        parse_bencode
    ))(input)
}

pub fn parse_dictionary(input: & [u8]) -> IResult<& [u8], BencodeValue> {
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

pub fn parse_bytes(input: & [u8]) -> IResult<& [u8], BencodeBytes> {
    let (rest, byte_string_len): (&[u8], u64) = map_res(map_res(digit1, std::str::from_utf8), str::parse)(input)?;
    let (rest, _) = char(':')(rest)?;
    let (rest, bytes) = take(byte_string_len)(rest)?;

    return Ok((rest, bytes.to_vec()))
}

pub fn parse_bencode(input: & [u8]) -> IResult<& [u8], BencodeValue> {
    alt((
        parse_integer,
        parse_list,
        parse_dictionary,
        map(parse_bytes, BencodeValue::Bytes),
    ))(input)
}
