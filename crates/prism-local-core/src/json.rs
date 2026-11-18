//! Bounded strict JSON parsing for the future stdio protocol.

use std::collections::BTreeMap;

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 16_384;
const MAX_STRING: usize = 65_536;

#[derive(Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidJson;

#[derive(Debug, PartialEq, Eq)]
pub struct OutputTooSmall;

#[derive(Debug, PartialEq, Eq)]
pub enum StringDecodeError {
    Invalid,
    OutputTooSmall,
}

/// Decode exactly one JSON string token into fixed storage without allocating.
/// On any error the output may be partially written and must be discarded.
pub fn decode_string_into(input: &[u8], output: &mut [u8]) -> Result<usize, StringDecodeError> {
    let mut pos = 0;
    let mut written = 0_usize;
    decode_string(input, &mut pos, |bytes| {
        let end = written
            .checked_add(bytes.len())
            .ok_or(StringDecodeError::OutputTooSmall)?;
        let target = output
            .get_mut(written..end)
            .ok_or(StringDecodeError::OutputTooSmall)?;
        target.copy_from_slice(bytes);
        written = end;
        Ok(())
    })?;
    if pos != input.len() || std::str::from_utf8(&output[..written]).is_err() {
        return Err(StringDecodeError::Invalid);
    }
    Ok(written)
}

fn decode_string(
    input: &[u8],
    pos: &mut usize,
    mut emit: impl FnMut(&[u8]) -> Result<(), StringDecodeError>,
) -> Result<(), StringDecodeError> {
    if read_byte(input, pos)? != b'"' {
        return Err(StringDecodeError::Invalid);
    }
    let mut produced = 0_usize;
    loop {
        let byte = read_byte(input, pos)?;
        let mut encoded = [0_u8; 4];
        let size = match byte {
            b'"' => return Ok(()),
            0..=31 => return Err(StringDecodeError::Invalid),
            b'\\' => match read_byte(input, pos)? {
                escape @ (b'"' | b'\\' | b'/') => {
                    encoded[0] = escape;
                    1
                }
                b'b' => {
                    encoded[0] = 8;
                    1
                }
                b'f' => {
                    encoded[0] = 12;
                    1
                }
                b'n' => {
                    encoded[0] = b'\n';
                    1
                }
                b'r' => {
                    encoded[0] = b'\r';
                    1
                }
                b't' => {
                    encoded[0] = b'\t';
                    1
                }
                b'u' => {
                    let first = read_hex4(input, pos)?;
                    let code = if (0xd800..=0xdbff).contains(&first) {
                        if read_byte(input, pos)? != b'\\' || read_byte(input, pos)? != b'u' {
                            return Err(StringDecodeError::Invalid);
                        }
                        let second = read_hex4(input, pos)?;
                        if !(0xdc00..=0xdfff).contains(&second) {
                            return Err(StringDecodeError::Invalid);
                        }
                        0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                    } else {
                        first
                    };
                    char::from_u32(code)
                        .ok_or(StringDecodeError::Invalid)?
                        .encode_utf8(&mut encoded)
                        .len()
                }
                _ => return Err(StringDecodeError::Invalid),
            },
            _ => {
                encoded[0] = byte;
                1
            }
        };
        produced = produced
            .checked_add(size)
            .ok_or(StringDecodeError::Invalid)?;
        if produced > MAX_STRING {
            return Err(StringDecodeError::Invalid);
        }
        emit(&encoded[..size])?;
    }
}

fn read_byte(input: &[u8], pos: &mut usize) -> Result<u8, StringDecodeError> {
    let byte = *input.get(*pos).ok_or(StringDecodeError::Invalid)?;
    *pos = pos.checked_add(1).ok_or(StringDecodeError::Invalid)?;
    Ok(byte)
}

fn read_hex4(input: &[u8], pos: &mut usize) -> Result<u32, StringDecodeError> {
    let mut value = 0_u32;
    for _ in 0..4 {
        let digit = match read_byte(input, pos)? {
            b @ b'0'..=b'9' => b - b'0',
            b @ b'a'..=b'f' => b - b'a' + 10,
            b @ b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(StringDecodeError::Invalid),
        };
        value = value * 16 + u32::from(digit);
    }
    Ok(value)
}

/// Count bytes for a quoted JSON string, including escapes, without allocating.
pub fn string_output_len(value: &str) -> Option<usize> {
    value.chars().try_fold(2_usize, |len, ch| {
        let bytes = match ch {
            '"' | '\\' => 2,
            '\u{0}'..='\u{1f}' => 6,
            _ => ch.len_utf8(),
        };
        len.checked_add(bytes)
    })
}

/// Write a quoted JSON string into fixed storage. Insufficient storage is
/// rejected before writing any bytes; this function never grows the buffer.
pub fn write_string(value: &str, output: &mut [u8]) -> Result<usize, OutputTooSmall> {
    let length = string_output_len(value).ok_or(OutputTooSmall)?;
    if output.len() < length {
        return Err(OutputTooSmall);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output[0] = b'"';
    let mut pos = 1;
    for ch in value.chars() {
        match ch {
            '"' | '\\' => {
                output[pos] = b'\\';
                output[pos + 1] = ch as u8;
                pos += 2;
            }
            '\u{0}'..='\u{1f}' => {
                let byte = ch as u8;
                output[pos..pos + 4].copy_from_slice(b"\\u00");
                output[pos + 4] = HEX[usize::from(byte >> 4)];
                output[pos + 5] = HEX[usize::from(byte & 15)];
                pos += 6;
            }
            _ => {
                let mut encoded = [0; 4];
                let bytes = ch.encode_utf8(&mut encoded).as_bytes();
                output[pos..pos + bytes.len()].copy_from_slice(bytes);
                pos += bytes.len();
            }
        }
    }
    output[pos] = b'"';
    Ok(length)
}

pub fn parse(input: &[u8]) -> Result<Value, InvalidJson> {
    if input.len() > crate::frame::HARD_MAX_FRAME {
        return Err(InvalidJson);
    }
    let mut p = Parser {
        input,
        pos: 0,
        nodes: 0,
    };
    let value = p.value(0)?;
    p.ws();
    if p.pos != input.len() {
        return Err(InvalidJson);
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.input.get(self.pos), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }
    fn take(&mut self, b: u8) -> bool {
        if self.input.get(self.pos) == Some(&b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn literal(&mut self, suffix: &[u8]) -> Result<(), InvalidJson> {
        let end = self.pos.checked_add(suffix.len()).ok_or(InvalidJson)?;
        if self.input.get(self.pos..end) != Some(suffix) {
            return Err(InvalidJson);
        }
        self.pos = end;
        Ok(())
    }
    fn value(&mut self, depth: usize) -> Result<Value, InvalidJson> {
        self.ws();
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(InvalidJson);
        }
        self.nodes += 1;
        match self.input.get(self.pos).copied().ok_or(InvalidJson)? {
            b'n' => {
                self.literal(b"null")?;
                Ok(Value::Null)
            }
            b't' => {
                self.literal(b"true")?;
                Ok(Value::Bool(true))
            }
            b'f' => {
                self.literal(b"false")?;
                Ok(Value::Bool(false))
            }
            b'"' => Ok(Value::String(self.string()?)),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => Ok(Value::Number(self.number()?)),
            _ => Err(InvalidJson),
        }
    }
    fn array(&mut self, depth: usize) -> Result<Value, InvalidJson> {
        self.pos += 1;
        let mut values = Vec::new();
        self.ws();
        if self.take(b']') {
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value(depth + 1)?);
            self.ws();
            if self.take(b']') {
                return Ok(Value::Array(values));
            }
            if !self.take(b',') {
                return Err(InvalidJson);
            }
        }
    }
    fn object(&mut self, depth: usize) -> Result<Value, InvalidJson> {
        self.pos += 1;
        let mut values = BTreeMap::new();
        self.ws();
        if self.take(b'}') {
            return Ok(Value::Object(values));
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            if !self.take(b':') {
                return Err(InvalidJson);
            }
            let value = self.value(depth + 1)?;
            if values.insert(key, value).is_some() {
                return Err(InvalidJson);
            }
            self.ws();
            if self.take(b'}') {
                return Ok(Value::Object(values));
            }
            if !self.take(b',') {
                return Err(InvalidJson);
            }
        }
    }
    fn string(&mut self) -> Result<String, InvalidJson> {
        let mut output = Vec::new();
        decode_string(self.input, &mut self.pos, |bytes| {
            output.extend_from_slice(bytes);
            Ok(())
        })
        .map_err(|_| InvalidJson)?;
        String::from_utf8(output).map_err(|_| InvalidJson)
    }
    fn number(&mut self) -> Result<String, InvalidJson> {
        let start = self.pos;
        self.take(b'-');
        if !self.take(b'0') {
            if !matches!(self.input.get(self.pos), Some(b'1'..=b'9')) {
                return Err(InvalidJson);
            }
            self.pos += 1;
            while matches!(self.input.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if self.take(b'.') {
            self.digits()?;
        }
        if matches!(self.input.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if !self.take(b'+') {
                self.take(b'-');
            }
            self.digits()?;
        }
        let value = self.input.get(start..self.pos).ok_or(InvalidJson)?;
        if value.len() > MAX_STRING {
            return Err(InvalidJson);
        }
        Ok(std::str::from_utf8(value)
            .map_err(|_| InvalidJson)?
            .to_owned())
    }
    fn digits(&mut self) -> Result<(), InvalidJson> {
        let start = self.pos;
        while matches!(self.input.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == start {
            Err(InvalidJson)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_string_into, parse, string_output_len, write_string, OutputTooSmall,
        StringDecodeError, Value,
    };

    #[test]
    fn accepts_json_values_and_decodes_surrogate_pair() {
        assert_eq!(
            parse(br#"[null,true,-12.5e+3,"\uD83D\uDE00"]"#).unwrap(),
            Value::Array(vec![
                Value::Null,
                Value::Bool(true),
                Value::Number("-12.5e+3".into()),
                Value::String("😀".into())
            ])
        );
    }

    #[test]
    fn fixed_string_output_roundtrips_and_refuses_short_buffers_without_writing() {
        let controls: String = (0..=31)
            .map(|value| char::from_u32(value).unwrap())
            .collect();
        for value in [
            "",
            "plain",
            "quote\"slash\\",
            "\u{0}\n\u{1f}",
            "café 😀",
            controls.as_str(),
        ] {
            let needed = string_output_len(value).unwrap();
            let mut output = vec![0; needed];
            assert_eq!(write_string(value, &mut output), Ok(needed));
            assert_eq!(parse(&output), Ok(Value::String(value.to_owned())));
            let mut decoded = vec![0; value.len()];
            assert_eq!(decode_string_into(&output, &mut decoded), Ok(value.len()));
            assert_eq!(decoded, value.as_bytes());
            let mut short = vec![b'x'; needed - 1];
            assert_eq!(write_string(value, &mut short), Err(OutputTooSmall));
            assert!(short.iter().all(|&byte| byte == b'x'));
        }
        let value = "\"\\\u{0}\n\u{1f}";
        let mut output = vec![0; string_output_len(value).unwrap()];
        write_string(value, &mut output).unwrap();
        assert_eq!(&output, br#""\"\\\u0000\u000a\u001f""#);
    }

    #[test]
    fn fixed_string_decoder_rejects_invalid_tokens_and_insufficient_storage() {
        let token = br#""a\"\\\/\b\f\n\r\t\u00e9\uD83D\uDE00""#;
        let expected = "a\"\\/\u{8}\u{c}\n\r\té😀".as_bytes();
        let mut output = vec![0; expected.len()];
        assert_eq!(decode_string_into(token, &mut output), Ok(expected.len()));
        assert_eq!(output, expected);
        let mut short = vec![0; expected.len() - 1];
        assert_eq!(
            decode_string_into(token, &mut short),
            Err(StringDecodeError::OutputTooSmall)
        );
        let boundaries = br#""\u0000\u007f\u0800\ud7ff\ue000\uffff\ud800\udc00\udbff\udfff""#;
        let expected = "\u{0}\u{7f}\u{800}\u{d7ff}\u{e000}\u{ffff}\u{10000}\u{10ffff}".as_bytes();
        let mut output = vec![0; expected.len()];
        assert_eq!(
            decode_string_into(boundaries, &mut output),
            Ok(expected.len())
        );
        assert_eq!(output, expected);
        for invalid in [
            &br#""\ud800""#[..],
            &br#""\udc00""#[..],
            &br#""\ud800\u0041""#[..],
            &br#""\u12xz""#[..],
            &b"\"\xff\""[..],
            &b"\"\xc0\xaf\""[..],
            &b"\"\xed\xa0\x80\""[..],
            &b"\"\xf4\x90\x80\x80\""[..],
            &b"\"\n\""[..],
            &br#""a" trailing"#[..],
        ] {
            assert_eq!(
                decode_string_into(invalid, &mut [0; 64]),
                Err(StringDecodeError::Invalid)
            );
        }
        let token = format!("\"{}\"", "a".repeat(65_537));
        assert_eq!(
            decode_string_into(token.as_bytes(), &mut vec![0; 65_537]),
            Err(StringDecodeError::Invalid)
        );
    }
    #[test]
    fn rejects_ambiguous_and_invalid_inputs() {
        let cases: &[&[u8]] = &[
            br#"{"a":1,"\u0061":2}"#,
            br#"{"x":1,}"#,
            b"[1,]",
            b"01",
            b"1.",
            b"1e+",
            b"true false",
            br#""\ud800""#,
            br#""\udc00""#,
            b"\"\xff\"",
            b"\"\n\"",
        ];
        for case in cases {
            assert!(parse(case).is_err(), "{case:?}");
        }
    }
    #[test]
    fn enforces_nesting_and_node_limits() {
        let deep = format!("{}0{}", "[".repeat(34), "]".repeat(34));
        assert!(parse(deep.as_bytes()).is_err());
        let nodes = format!("[{}0]", "0,".repeat(16_384));
        assert!(parse(nodes.as_bytes()).is_err());
        let string = format!("\"{}\"", "a".repeat(65_537));
        assert!(parse(string.as_bytes()).is_err());
    }

    #[test]
    fn deterministic_arbitrary_bytes_smoke_never_panics() {
        let mut state = 0xbb67_ae85_u32;
        for length in 0..512 {
            let mut input = Vec::with_capacity(length);
            for _ in 0..length {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                input.push((state >> 24) as u8);
            }
            let _ = parse(&input);
        }
    }
}
