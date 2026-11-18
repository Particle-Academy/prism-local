use prism_local_core::frame::{read_frame, write_frame, HARD_MAX_FRAME};
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn invalid_startup_limits_fail_closed() {
    for args in [
        vec!["--max-memory-mb", "3"],
        vec!["--max-memory-mb", "18446744073709551615"],
        vec!["--max-memory-mb"],
        vec!["--unknown"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_prism-local"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(125));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn hello_cannot_raise_the_startup_memory_budget() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-local"))
        .args(["--max-memory-mb", "4"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"maxMemoryMb":5}}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":2,"method":"hello"}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    drop(input);
    let mut output = child.stdout.take().unwrap();
    let denied = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    assert!(std::str::from_utf8(&denied).unwrap().contains("-32602"));
    let accepted = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    let accepted = std::str::from_utf8(&accepted).unwrap();
    assert!(accepted.contains("\"maxMemoryMb\":4"));
    assert!(accepted.contains("\"processMemoryLimitMb\":260"));
    assert!(child.wait().unwrap().success());
}

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

#[test]
fn frame_limit_accepts_boundary_and_refuses_an_oversized_header() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-local"))
        .args(["--max-memory-mb", "4"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut frame =
        br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"maxMemoryMb":4}}"#.to_vec();
    frame.resize(HARD_MAX_FRAME, b' ');
    write_frame(&mut input, &frame, HARD_MAX_FRAME).unwrap();
    input.flush().unwrap();
    let mut output = child.stdout.take().unwrap();
    let reply = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    let reply = std::str::from_utf8(&reply).unwrap();
    assert!(reply.contains("\"maxMemoryMb\":4"));
    assert!(reply.contains("\"memoryScope\":\"managedPayload\""));
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":2,"method":"stats"}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    input.flush().unwrap();
    drop(input);
    let stats = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    assert!(std::str::from_utf8(&stats)
        .unwrap()
        .contains("\"maxMemoryMb\":4"));
    assert!(child.wait().unwrap().success());

    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-local"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(&((HARD_MAX_FRAME + 1) as u32).to_be_bytes())
        .unwrap();
    input.flush().unwrap();
    let limit = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if Instant::now() >= limit {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("oversized frame body was awaited");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn deadline_kills_stalled_partial_frame_but_does_not_expire_while_idle() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-local"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"deadlineMs":500}}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    input.flush().unwrap();
    let hello = read_frame(&mut output, HARD_MAX_FRAME).unwrap().unwrap();
    assert!(std::str::from_utf8(&hello)
        .unwrap()
        .contains("\"deadlineMs\":500"));
    std::thread::sleep(Duration::from_millis(700));
    assert!(
        child.try_wait().unwrap().is_none(),
        "idle process timed out"
    );
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":2,"method":"stats"}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    input.flush().unwrap();
    assert!(read_frame(&mut output, HARD_MAX_FRAME).unwrap().is_some());
    input.write_all(&[0]).unwrap();
    input.flush().unwrap();
    assert_expired(&mut child);
}

#[test]
fn deadline_kills_stalled_partial_payload() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_prism-local"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    write_frame(
        &mut input,
        br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"deadlineMs":500}}"#,
        HARD_MAX_FRAME,
    )
    .unwrap();
    input.flush().unwrap();
    assert!(read_frame(&mut output, HARD_MAX_FRAME).unwrap().is_some());
    input.write_all(&[0, 0, 0, 5, b'{']).unwrap();
    input.flush().unwrap();
    assert_expired(&mut child);
}

fn assert_expired(child: &mut Child) {
    let limit = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(124));
            break;
        }
        if Instant::now() >= limit {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("partial frame did not reach its deadline");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
