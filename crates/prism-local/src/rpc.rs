//! Sequential JSON-RPC skeleton. No model, worker pool or inference exists yet.

use crate::rpc_response::{encode, valid_id_number, EncodeError, ErrorCode, Id, Response, Success};
use prism_local_core::{
    json_arena::{self, Arena, Kind},
    memory_plan::{Buffer, RequestBuffers},
};
use std::time::Duration;

const DEFAULT_DEADLINE_MS: usize = 30_000;

#[derive(Default)]
pub struct Server {
    settings: Option<Settings>,
}

#[derive(Clone, Copy)]
struct Settings {
    threads: usize,
    deadline_ms: usize,
}

impl Server {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn handle(&mut self, frame: &[u8]) -> (Option<Vec<u8>>, bool) {
        use prism_local_core::memory_plan::{checked_plan, Allocation};
        let mut arena = RequestBuffers::new(
            checked_plan(
                2,
                0,
                &[Allocation {
                    count: 2 * 1024 * 1024,
                    element_bytes: 1,
                }],
            )
            .unwrap(),
        );
        let mut response = RequestBuffers::new(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: 1024 * 1024,
                    element_bytes: 1,
                }],
            )
            .unwrap(),
        );
        let (value, stop) = self
            .handle_budgeted(frame, &mut arena, &mut response)
            .unwrap();
        (value.map(|buffer| buffer.as_slice().to_vec()), stop)
    }

    pub fn timeout(&self) -> Duration {
        let millis = self
            .settings
            .as_ref()
            .map_or(DEFAULT_DEADLINE_MS, |settings| settings.deadline_ms);
        Duration::from_millis(millis as u64)
    }

    /// On response-allocation failure the caller must terminate the stream.
    pub fn handle_budgeted(
        &mut self,
        frame: &[u8],
        buffers: &mut RequestBuffers,
        responses: &mut RequestBuffers,
    ) -> Result<(Option<Buffer<u8>>, bool), EncodeError> {
        let arena = match json_arena::parse(frame, buffers) {
            Ok(value) => value,
            Err(json_arena::Error::Invalid) => return failure(responses, ErrorCode::Parse),
            Err(json_arena::Error::Resource) => return failure(responses, ErrorCode::Resource),
        };
        let root = arena.root();
        if arena.kind(root) != Some(Kind::Object) {
            return failure(responses, ErrorCode::InvalidRequest);
        }
        let id = match arena.get(root, "id") {
            None => None,
            Some(index) => match arena.kind(index) {
                Some(Kind::Null) => Some(Id::Null),
                Some(Kind::String) => match arena.string(index) {
                    Some(value) => Some(Id::String(value)),
                    None => return failure(responses, ErrorCode::InvalidRequest),
                },
                Some(Kind::Number) => match arena.number(index) {
                    Some(value) if valid_id_number(value) => Some(Id::Number(value)),
                    _ => return failure(responses, ErrorCode::InvalidRequest),
                },
                _ => return failure(responses, ErrorCode::InvalidRequest),
            },
        };
        if arena
            .get(root, "jsonrpc")
            .and_then(|index| arena.string(index))
            != Some("2.0")
        {
            return failure(responses, ErrorCode::InvalidRequest);
        }
        let method = match arena
            .get(root, "method")
            .and_then(|index| arena.string(index))
        {
            Some(value) => value,
            None => return failure(responses, ErrorCode::InvalidRequest),
        };
        let params = match arena.get(root, "params") {
            None => None,
            Some(index) if arena.kind(index) == Some(Kind::Object) => Some(index),
            Some(_) => return respond_error(responses, id, ErrorCode::InvalidParams),
        };
        let mut pending = None;
        let (result, stop) = match method {
            "hello" => match self.hello(&arena, params) {
                Some(settings) => {
                    pending = Some(settings);
                    (Ok(settings.result()), false)
                }
                None => (Err(ErrorCode::InvalidParams), false),
            },
            "stats" if params.is_none_or(|index| arena.first_child(index).is_none()) => {
                match &self.settings {
                    Some(settings) => (Ok(settings.result()), false),
                    None => (Err(ErrorCode::HelloRequired), false),
                }
            }
            "shutdown" if params.is_none_or(|index| arena.first_child(index).is_none()) => {
                (Ok(Success::Null), true)
            }
            "stats" | "shutdown" => (Err(ErrorCode::InvalidParams), false),
            _ => (Err(ErrorCode::MethodNotFound), false),
        };
        let response = match id {
            Some(id) => Some(encode(
                match result {
                    Ok(result) => Response::Success { id, result },
                    Err(code) => Response::Error { id, code },
                },
                responses,
            )?),
            None => None,
        };
        if let Some(settings) = pending {
            self.settings = Some(settings);
        }
        Ok((response, stop))
    }

    fn hello(&self, arena: &Arena<'_>, params: Option<usize>) -> Option<Settings> {
        if self.settings.is_some() {
            return None;
        }
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        let threads = option_positive(arena, params, "threads", 1)?;
        let deadline_ms = option_positive(arena, params, "deadlineMs", DEFAULT_DEADLINE_MS)?;
        let mut child = params.and_then(|index| arena.first_child(index));
        while let Some(index) = child {
            if !matches!(arena.key(index), Some("threads" | "deadlineMs")) {
                return None;
            }
            child = arena.next_sibling(index);
        }
        if threads > cores || !(100..=300_000).contains(&deadline_ms) {
            return None;
        }
        Some(Settings {
            threads,
            deadline_ms,
        })
    }
}

fn option_positive(
    arena: &Arena<'_>,
    params: Option<usize>,
    key: &str,
    default: usize,
) -> Option<usize> {
    match params.and_then(|index| arena.get(index, key)) {
        None => Some(default),
        Some(index) => usize::try_from(integer(arena.number(index)?)?)
            .ok()
            .filter(|&v| v > 0),
    }
}

fn integer(s: &str) -> Option<u64> {
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl Settings {
    fn result(&self) -> Success {
        Success::Settings {
            threads: self.threads,
            deadline_ms: self.deadline_ms,
        }
    }
}

fn failure(
    buffers: &mut RequestBuffers,
    code: ErrorCode,
) -> Result<(Option<Buffer<u8>>, bool), EncodeError> {
    respond_error(buffers, Some(Id::Null), code)
}

fn respond_error(
    buffers: &mut RequestBuffers,
    id: Option<Id<'_>>,
    code: ErrorCode,
) -> Result<(Option<Buffer<u8>>, bool), EncodeError> {
    let response = match id {
        Some(id) => Some(encode(Response::Error { id, code }, buffers)?),
        None => None,
    };
    Ok((response, false))
}

#[cfg(test)]
mod tests {
    use super::Server;
    use prism_local_core::json::{parse, Value};
    use prism_local_core::memory_plan::{checked_plan, Allocation, RequestBuffers};

    fn text(b: Option<Vec<u8>>) -> String {
        String::from_utf8(b.unwrap()).unwrap()
    }

    #[test]
    fn hello_stats_and_shutdown_are_serial_and_notifications_are_silent() {
        let mut s = Server::new();
        let (reply, stop) =
            s.handle(br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"threads":1}}"#);
        assert!(!stop);
        assert!(text(reply).contains("\"inferenceAvailable\":false"));
        let (reply, _) = s.handle(br#"{"jsonrpc":"2.0","id":"x\"y","method":"stats"}"#);
        assert!(text(reply).contains("\"id\":\"x\\\"y\""));
        let (reply, _) = s.handle(br#"{"jsonrpc":"2.0","id":"\u0000\n\\\"","method":"stats"}"#);
        let Value::Object(mut reply) = parse(&reply.unwrap()).unwrap() else {
            panic!("response object")
        };
        assert_eq!(reply.remove("id"), Some(Value::String("\0\n\\\"".into())));
        let (reply, stop) = s.handle(br#"{"jsonrpc":"2.0","method":"shutdown"}"#);
        assert!(reply.is_none());
        assert!(stop);
    }

    #[test]
    fn rejects_ambiguous_json_and_unsafe_or_invalid_settings() {
        let mut s = Server::new();
        assert!(text(
            s.handle(br#"{"jsonrpc":"2.0","id":1,"method":"hello","method":"shutdown"}"#)
                .0
        )
        .contains("-32700"));
        for params in [
            r#"{"threads":0}"#,
            r#"{"threads":99999999}"#,
            r#"{"deadlineMs":99}"#,
            r#"{"deadlineMs":300001}"#,
            r#"{"maxMemoryMb":1}"#,
            r#"{"maxMemoryMb":64}"#,
            r#"{"unknown":true}"#,
        ] {
            let request = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"hello\",\"params\":{params}}}"
            );
            assert!(text(s.handle(request.as_bytes()).0).contains("-32602"));
        }
        assert!(
            text(s.handle(br#"{"jsonrpc":"2.0","id":1,"method":"embed"}"#).0).contains("-32601")
        );
    }

    #[test]
    fn arena_refusal_does_not_apply_settings() {
        let mut server = Server::new();
        let mut buffers = RequestBuffers::new(checked_plan(1, 0, &[]).unwrap());
        let mut responses = RequestBuffers::new(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: 1024,
                    element_bytes: 1,
                }],
            )
            .unwrap(),
        );
        let request = br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"threads":1}}"#;
        let (reply, stop) = server
            .handle_budgeted(request, &mut buffers, &mut responses)
            .unwrap();
        assert!(!stop);
        assert!(std::str::from_utf8(reply.unwrap().as_slice())
            .unwrap()
            .contains("-32001"));
        let (reply, stop) = server.handle(request);
        assert!(!stop);
        assert!(text(reply).contains("\"inferenceAvailable\":false"));
    }

    #[test]
    fn response_refusal_does_not_apply_settings() {
        let mut server = Server::new();
        let mut buffers = RequestBuffers::new(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: 4096,
                    element_bytes: 1,
                }],
            )
            .unwrap(),
        );
        let mut responses = RequestBuffers::new(checked_plan(1, 0, &[]).unwrap());
        let request = br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"threads":1}}"#;
        assert!(server
            .handle_budgeted(request, &mut buffers, &mut responses)
            .is_err());
        assert!(text(server.handle(request).0).contains("\"inferenceAvailable\":false"));
    }

    #[test]
    fn notification_does_not_need_response_storage() {
        let mut server = Server::new();
        let mut buffers = RequestBuffers::new(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: 4096,
                    element_bytes: 1,
                }],
            )
            .unwrap(),
        );
        let mut responses = RequestBuffers::new(checked_plan(1, 0, &[]).unwrap());
        let request = br#"{"jsonrpc":"2.0","method":"hello","params":{"threads":1}}"#;
        let (reply, stop) = server
            .handle_budgeted(request, &mut buffers, &mut responses)
            .unwrap();
        assert!(reply.is_none());
        assert!(!stop);
        assert!(text(
            server
                .handle(br#"{"jsonrpc":"2.0","id":1,"method":"stats"}"#)
                .0
        )
        .contains("\"inferenceAvailable\":false"));
    }

    #[test]
    fn bounded_mutations_produce_valid_responses_or_notifications() {
        let seeds: &[&[u8]] = &[
            br#"{"jsonrpc":"2.0","id":"\u0000\n\"","method":"hello","params":{"threads":1}}"#,
            br#"{"jsonrpc":"2.0","id":-9223372036854775808,"method":"stats"}"#,
            br#"{"jsonrpc":"2.0","method":"shutdown"}"#,
        ];
        let mut state = 0xa54f_f53a_u32;
        for iteration in 0..4096 {
            let mut request = seeds[iteration % seeds.len()].to_vec();
            for _ in 0..4 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let index = state as usize % request.len();
                request[index] ^= (state >> 24) as u8;
            }
            if iteration % 8 == 0 {
                request.truncate(state as usize % request.len());
            }
            let mut arena = RequestBuffers::new(
                checked_plan(
                    2,
                    0,
                    &[Allocation {
                        count: 2 * 1024 * 1024,
                        element_bytes: 1,
                    }],
                )
                .unwrap(),
            );
            let mut output = RequestBuffers::new(
                checked_plan(
                    1,
                    0,
                    &[Allocation {
                        count: 1024 * 1024,
                        element_bytes: 1,
                    }],
                )
                .unwrap(),
            );
            let result = Server::new().handle_budgeted(&request, &mut arena, &mut output);
            assert!(result.is_ok());
            let (response, _) = result.unwrap();
            if let Some(response) = response {
                assert!(parse(response.as_slice()).is_ok());
            }
        }
    }
}
