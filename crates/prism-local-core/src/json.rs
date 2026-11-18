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
        if !self.take(b'"') {
            return Err(InvalidJson);
        }
        let mut output = Vec::new();
        loop {
            let byte = *self.input.get(self.pos).ok_or(InvalidJson)?;
            self.pos += 1;
            match byte {
                b'"' => return String::from_utf8(output).map_err(|_| InvalidJson),
                0..=31 => return Err(InvalidJson),
                b'\\' => {
                    let escape = *self.input.get(self.pos).ok_or(InvalidJson)?;
                    self.pos += 1;
                    match escape {
                        b'"' | b'\\' | b'/' => output.push(escape),
                        b'b' => output.push(8),
                        b'f' => output.push(12),
                        b'n' => output.push(b'\n'),
                        b'r' => output.push(b'\r'),
                        b't' => output.push(b'\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xd800..=0xdbff).contains(&first) {
                                if !self.take(b'\\') || !self.take(b'u') {
                                    return Err(InvalidJson);
                                }
                                let second = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err(InvalidJson);
                                }
                                0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                            } else {
                                first
                            };
                            let ch = char::from_u32(code).ok_or(InvalidJson)?;
                            let mut buf = [0; 4];
                            output.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(InvalidJson),
                    }
                }
                _ => output.push(byte),
            }
            if output.len() > MAX_STRING {
                return Err(InvalidJson);
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, InvalidJson> {
        let mut value = 0_u32;
        for _ in 0..4 {
            let b = *self.input.get(self.pos).ok_or(InvalidJson)?;
            self.pos += 1;
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(InvalidJson),
            };
            value = value * 16 + u32::from(digit);
        }
        Ok(value)
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
    use super::{parse, Value};

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
}
