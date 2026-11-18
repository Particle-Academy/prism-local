//! Fixed-storage JSON-RPC response encoding for the current skeleton.

use prism_local_core::{
    frame::HARD_MAX_FRAME,
    json::{string_output_len, write_string},
    memory_plan::{Buffer, RequestBuffers},
};
use std::fmt::{self, Write as _};

const PREFIX: &[u8] = br#"{"jsonrpc":"2.0","id":"#;

#[derive(Clone, Copy)]
pub enum Id<'a> {
    Null,
    Number(&'a str),
    String(&'a str),
}

#[derive(Clone, Copy)]
pub enum ErrorCode {
    Parse,
    InvalidRequest,
    InvalidParams,
    MethodNotFound,
    HelloRequired,
    Resource,
}

pub enum Success {
    Null,
    Settings { threads: usize, deadline_ms: usize },
}

pub enum Response<'a> {
    Success { id: Id<'a>, result: Success },
    Error { id: Id<'a>, code: ErrorCode },
}

#[derive(Debug, PartialEq, Eq)]
pub struct EncodeError;

/// Encode a response in one fixed buffer charged to the request plan.
pub fn encode(
    response: Response<'_>,
    buffers: &mut RequestBuffers,
) -> Result<Buffer<u8>, EncodeError> {
    let mut dynamic_tail = StackWriter::new();
    let (id, tail): (Id<'_>, &[u8]) = match response {
        Response::Success {
            id,
            result: Success::Null,
        } => (id, br#","result":null}"#),
        Response::Success {
            id,
            result:
                Success::Settings {
                    threads,
                    deadline_ms,
                },
        } => {
            write!(&mut dynamic_tail, ",\"result\":{{\"threads\":{threads},\"deadlineMs\":{deadline_ms},\"inferenceAvailable\":false}}}}")
                .map_err(|_| EncodeError)?;
            (id, dynamic_tail.bytes())
        }
        Response::Error { id, code } => (id, code.tail()),
    };
    let id_length = match id {
        Id::Null => 4,
        Id::Number(value) if valid_id_number(value) => value.len(),
        Id::Number(_) => return Err(EncodeError),
        Id::String(value) => string_output_len(value).ok_or(EncodeError)?,
    };
    let length = PREFIX
        .len()
        .checked_add(id_length)
        .and_then(|v| v.checked_add(tail.len()))
        .ok_or(EncodeError)?;
    if length > HARD_MAX_FRAME {
        return Err(EncodeError);
    }
    let mut output = buffers
        .allocate::<u8>(u64::try_from(length).map_err(|_| EncodeError)?)
        .map_err(|_| EncodeError)?;
    let bytes = output.as_mut_slice();
    bytes[..PREFIX.len()].copy_from_slice(PREFIX);
    let end = PREFIX.len() + id_length;
    let target = &mut bytes[PREFIX.len()..end];
    match id {
        Id::Null => target.copy_from_slice(b"null"),
        Id::Number(value) => target.copy_from_slice(value.as_bytes()),
        Id::String(value) => {
            write_string(value, target).map_err(|_| EncodeError)?;
        }
    }
    bytes[end..].copy_from_slice(tail);
    Ok(output)
}

/// Restrict numeric IDs to JSON integer syntax and the supported u64/i64 range.
pub fn valid_id_number(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
    {
        return false;
    }
    if value.starts_with('-') {
        value.parse::<i64>().is_ok()
    } else {
        value.parse::<u64>().is_ok()
    }
}

impl ErrorCode {
    fn tail(self) -> &'static [u8] {
        match self {
            Self::Parse => br#","error":{"code":-32700,"message":"Parse error"}}"#,
            Self::InvalidRequest => br#","error":{"code":-32600,"message":"Invalid Request"}}"#,
            Self::InvalidParams => br#","error":{"code":-32602,"message":"Invalid params"}}"#,
            Self::MethodNotFound => br#","error":{"code":-32601,"message":"Method not found"}}"#,
            Self::HelloRequired => br#","error":{"code":-32000,"message":"Call hello first"}}"#,
            Self::Resource => {
                br#","error":{"code":-32001,"message":"Request memory unavailable"}}"#
            }
        }
    }
}

struct StackWriter {
    bytes: [u8; 128],
    len: usize,
}

impl StackWriter {
    fn new() -> Self {
        Self {
            bytes: [0; 128],
            len: 0,
        }
    }
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl fmt::Write for StackWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(fmt::Error)?;
        let target = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{encode, valid_id_number, ErrorCode, Id, Response, Success};
    use prism_local_core::{
        json::{parse, Value},
        memory_plan::{checked_plan, Allocation, RequestBuffers},
    };

    fn buffers(bytes: u64) -> RequestBuffers {
        RequestBuffers::new(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: bytes,
                    element_bytes: 1,
                }],
            )
            .unwrap(),
        )
    }

    #[test]
    fn static_error_tails_have_exact_wire_format() {
        for (code, expected) in [
            (ErrorCode::Parse, br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#.as_slice()),
            (ErrorCode::InvalidRequest, br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}"#.as_slice()),
            (ErrorCode::InvalidParams, br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32602,"message":"Invalid params"}}"#.as_slice()),
            (ErrorCode::MethodNotFound, br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32601,"message":"Method not found"}}"#.as_slice()),
            (ErrorCode::HelloRequired, br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32000,"message":"Call hello first"}}"#.as_slice()),
            (ErrorCode::Resource, br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32001,"message":"Request memory unavailable"}}"#.as_slice()),
        ] {
            let mut budget = buffers(1024);
            let output = encode(Response::Error { id: Id::Null, code }, &mut budget).unwrap();
            assert_eq!(output.as_slice(), expected);
        }
    }

    #[test]
    fn numeric_ids_are_validated_before_writing() {
        for good in ["0", "-0", "18446744073709551615", "-9223372036854775808"] {
            assert!(valid_id_number(good));
            let mut budget = buffers(1024);
            assert!(encode(
                Response::Success {
                    id: Id::Number(good),
                    result: Success::Null
                },
                &mut budget
            )
            .is_ok());
        }
        for bad in [
            "",
            "+1",
            "01",
            "-00",
            "1.0",
            "1e3",
            "18446744073709551616",
            "-9223372036854775809",
        ] {
            assert!(!valid_id_number(bad));
            let mut budget = buffers(1024);
            assert!(encode(
                Response::Success {
                    id: Id::Number(bad),
                    result: Success::Null
                },
                &mut budget
            )
            .is_err());
            assert_eq!(budget.remaining_bytes(), 1024);
        }
    }

    #[test]
    fn settings_and_maximum_escaped_id_fit_fixed_storage() {
        let id = "\u{0}".repeat(65_536);
        let mut budget = buffers(1_048_576);
        let output = encode(
            Response::Success {
                id: Id::String(&id),
                result: Success::Settings {
                    threads: usize::MAX,
                    deadline_ms: usize::MAX,
                },
            },
            &mut budget,
        )
        .unwrap();
        let Value::Object(mut decoded) = parse(output.as_slice()).unwrap() else {
            panic!("response object")
        };
        assert_eq!(decoded.remove("id"), Some(Value::String(id)));
        assert!(decoded.contains_key("result"));
        assert!(budget.remaining_bytes() < 1_048_576);
    }

    #[test]
    fn response_allocation_failure_is_explicit() {
        let mut budget = buffers(0);
        assert!(encode(
            Response::Success {
                id: Id::Null,
                result: Success::Null
            },
            &mut budget
        )
        .is_err());
        assert_eq!(budget.remaining_bytes(), 0);
    }
}
