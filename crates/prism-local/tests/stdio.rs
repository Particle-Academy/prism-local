use prism_local_core::frame::{read_frame, write_frame, HARD_MAX_FRAME};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn sidecar_answers_framed_requests_and_exits_on_shutdown() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-local"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":1,"method":"hello"}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    input.flush().unwrap();
    drop(input);
    let mut output = child.stdout.take().unwrap();
    let hello = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    let shutdown = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    assert!(std::str::from_utf8(&hello)
        .unwrap()
        .contains("\"inferenceAvailable\":false"));
    assert_eq!(shutdown, br#"{"jsonrpc":"2.0","id":2,"result":null}"#);
    assert!(read_frame(&mut output, HARD_MAX_FRAME).unwrap().is_none());
    assert!(child.wait().unwrap().success());
}
