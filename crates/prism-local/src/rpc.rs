//! Sequential JSON-RPC skeleton. No model, worker pool or inference exists yet.

use prism_local_core::json::{parse, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Default)]
pub struct Server {
    settings: Option<Settings>,
}

struct Settings {
    threads: usize,
    max_memory_mb: usize,
}

impl Server {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&mut self, frame: &[u8]) -> (Option<Vec<u8>>, bool) {
        let value = match parse(frame) {
            Ok(value) => value,
            Err(_) => return (Some(error("null", -32700, "Parse error")), false),
        };
        let Value::Object(mut object) = value else {
            return (Some(error("null", -32600, "Invalid Request")), false);
        };
        let id = match object.remove("id") {
            None => None,
            Some(Value::Null) => Some("null".to_owned()),
            Some(Value::String(s)) => Some(quote(&s)),
            Some(Value::Number(n)) if valid_id_number(&n) => Some(n),
            Some(_) => return (Some(error("null", -32600, "Invalid Request")), false),
        };
        if object.remove("jsonrpc") != Some(Value::String("2.0".into())) {
            return (Some(error("null", -32600, "Invalid Request")), false);
        }
        let method = match object.remove("method") {
            Some(Value::String(s)) => s,
            _ => return (Some(error("null", -32600, "Invalid Request")), false),
        };
        let params = match object.remove("params") {
            None => BTreeMap::new(),
            Some(Value::Object(p)) => p,
            _ => return (id.map(|id| error(&id, -32602, "Invalid params")), false),
        };
        let (result, stop) = match method.as_str() {
            "hello" => match self.hello(params) {
                Some(s) => (Ok(s), false),
                None => (Err((-32602, "Invalid params")), false),
            },
            "stats" if params.is_empty() => match &self.settings {
                Some(s) => (Ok(settings_result(s)), false),
                None => (Err((-32000, "Call hello first")), false),
            },
            "shutdown" if params.is_empty() => (Ok("null".into()), true),
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

    fn hello(&mut self, mut params: BTreeMap<String, Value>) -> Option<String> {
        if self.settings.is_some() {
            return None;
        }
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        let threads = option_positive(&mut params, "threads", 1)?;
        let max_memory_mb = option_positive(&mut params, "maxMemoryMb", 64)?;
        if !params.is_empty() || threads > cores || !(64..=4096).contains(&max_memory_mb) {
            return None;
        }
        let settings = Settings {
            threads,
            max_memory_mb,
        };
        let result = settings_result(&settings);
        self.settings = Some(settings);
        Some(result)
    }
}

fn option_positive(
    params: &mut BTreeMap<String, Value>,
    key: &str,
    default: usize,
) -> Option<usize> {
    match params.remove(key) {
        None => Some(default),
        Some(Value::Number(n)) => usize::try_from(integer(&n)?).ok().filter(|&v| v > 0),
        _ => None,
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
        "{{\"threads\":{},\"maxMemoryMb\":{},\"inferenceAvailable\":false}}",
        s.threads, s.max_memory_mb
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
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0}'..='\u{1f}' => {
                write!(&mut out, "\\u{:04x}", ch as u32).unwrap();
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::Server;
    fn text(b: Option<Vec<u8>>) -> String {
        String::from_utf8(b.unwrap()).unwrap()
    }
    #[test]
    fn hello_stats_and_shutdown_are_serial_and_notifications_are_silent() {
        let mut s = Server::new();
        let (reply, stop) = s.handle(
            br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"threads":1,"maxMemoryMb":64}}"#,
        );
        assert!(!stop);
        assert!(text(reply).contains("\"inferenceAvailable\":false"));
        let (reply, _) = s.handle(br#"{"jsonrpc":"2.0","id":"x\"y","method":"stats"}"#);
        assert!(text(reply).contains("\"id\":\"x\\\"y\""));
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
            r#"{"maxMemoryMb":1}"#,
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
}
