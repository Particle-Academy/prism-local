//! Sequential JSON-RPC skeleton. No model, worker pool or inference exists yet.

use prism_local_core::{
    json::{string_output_len, write_string},
    json_arena::{self, Arena, Kind},
    memory_plan::RequestBuffers,
};
use std::time::Duration;

const DEFAULT_DEADLINE_MS: usize = 30_000;

#[derive(Default)]
pub struct Server {
    settings: Option<Settings>,
}

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
        let plan = prism_local_core::memory_plan::checked_plan(
            2,
            0,
            &[prism_local_core::memory_plan::Allocation {
                count: 2 * 1024 * 1024,
                element_bytes: 1,
            }],
        )
        .unwrap();
        let mut buffers = RequestBuffers::new(plan);
        self.handle_budgeted(frame, &mut buffers)
    }

    pub fn timeout(&self) -> Duration {
        let millis = self
            .settings
            .as_ref()
            .map_or(DEFAULT_DEADLINE_MS, |settings| settings.deadline_ms);
        Duration::from_millis(millis as u64)
    }

    pub fn handle_budgeted(
        &mut self,
        frame: &[u8],
        buffers: &mut RequestBuffers,
    ) -> (Option<Vec<u8>>, bool) {
        let arena = match json_arena::parse(frame, buffers) {
            Ok(value) => value,
            Err(json_arena::Error::Invalid) => {
                return (Some(error("null", -32700, "Parse error")), false)
            }
            Err(json_arena::Error::Resource) => {
                return (
                    Some(error("null", -32001, "Request memory unavailable")),
                    false,
                )
            }
        };
        let root = arena.root();
        if arena.kind(root) != Some(Kind::Object) {
            return (Some(error("null", -32600, "Invalid Request")), false);
        }
        let id = match arena.get(root, "id") {
            None => None,
            Some(index) => match arena.kind(index) {
                Some(Kind::Null) => Some("null".to_owned()),
                Some(Kind::String) => match arena.string(index) {
                    Some(value) => Some(quote(value)),
                    None => return (Some(error("null", -32600, "Invalid Request")), false),
                },
                Some(Kind::Number) => match arena.number(index) {
                    Some(value) if valid_id_number(value) => Some(value.to_owned()),
                    _ => return (Some(error("null", -32600, "Invalid Request")), false),
                },
                _ => return (Some(error("null", -32600, "Invalid Request")), false),
            },
        };
        if arena
            .get(root, "jsonrpc")
            .and_then(|index| arena.string(index))
            != Some("2.0")
        {
            return (Some(error("null", -32600, "Invalid Request")), false);
        }
        let method = match arena
            .get(root, "method")
            .and_then(|index| arena.string(index))
        {
            Some(value) => value,
            _ => return (Some(error("null", -32600, "Invalid Request")), false),
        };
        let params = match arena.get(root, "params") {
            None => None,
            Some(index) if arena.kind(index) == Some(Kind::Object) => Some(index),
            Some(_) => return (id.map(|id| error(&id, -32602, "Invalid params")), false),
        };
        let (result, stop) = match method {
            "hello" => match self.hello(&arena, params) {
                Some(s) => (Ok(s), false),
                None => (Err((-32602, "Invalid params")), false),
            },
            "stats" if params.is_none_or(|index| arena.first_child(index).is_none()) => {
                match &self.settings {
                    Some(s) => (Ok(settings_result(s)), false),
                    None => (Err((-32000, "Call hello first")), false),
                }
            }
            "shutdown" if params.is_none_or(|index| arena.first_child(index).is_none()) => {
                (Ok("null".into()), true)
            }
            "stats" | "shutdown" => (Err((-32602, "Invalid params")), false),
            _ => (Err((-32601, "Method not found")), false),
        };
        let response = id.map(|id| match result {
            Ok(result) => {
                format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{result}}}").into_bytes()
            }
            Err((code, message)) => error(&id, code, message),
        });
        (response, stop)
    }

    fn hello(&mut self, arena: &Arena<'_>, params: Option<usize>) -> Option<String> {
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
        let settings = Settings {
            threads,
            deadline_ms,
        };
        let result = settings_result(&settings);
        self.settings = Some(settings);
        Some(result)
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
fn valid_id_number(s: &str) -> bool {
    if s.starts_with('-') {
        s.parse::<i64>().is_ok()
    } else {
        integer(s).is_some()
    }
}
fn settings_result(s: &Settings) -> String {
    format!(
        "{{\"threads\":{},\"deadlineMs\":{},\"inferenceAvailable\":false}}",
        s.threads, s.deadline_ms
    )
}
fn error(id: &str, code: i32, message: &str) -> Vec<u8> {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"error\":{{\"code\":{code},\"message\":{}}}}}",
        quote(message)
    )
    .into_bytes()
}
fn quote(s: &str) -> String {
    let length = string_output_len(s).expect("bounded JSON string");
    let mut out = vec![0; length];
    write_string(s, &mut out).expect("exact JSON string buffer");
    String::from_utf8(out).expect("JSON string is valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::Server;
    use prism_local_core::json::{parse, Value};
    use prism_local_core::memory_plan::{checked_plan, RequestBuffers};
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
        let plan = checked_plan(1, 0, &[]).unwrap();
        let mut buffers = RequestBuffers::new(plan);
        let request = br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"threads":1}}"#;
        let (reply, stop) = server.handle_budgeted(request, &mut buffers);
        assert!(!stop);
        assert!(text(reply).contains("-32001"));
        let (reply, stop) = server.handle(request);
        assert!(!stop);
        assert!(text(reply).contains("\"inferenceAvailable\":false"));
    }
}
