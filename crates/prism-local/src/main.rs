#![forbid(unsafe_code)]

mod deadline;
mod rpc;

use prism_local_core::frame::{read_frame_started, write_frame, HARD_MAX_FRAME};
use std::io::{self, Write};
use std::time::Instant;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut server = rpc::Server::new();
    let watchdog = deadline::Watchdog::new()?;
    loop {
        let mut active_deadline = None;
        let frame = read_frame_started(&mut input, HARD_MAX_FRAME, || {
            active_deadline = Some(watchdog.start(server.timeout())?);
            Ok(())
        })?;
        let Some(frame) = frame else { break };
        let expires = active_deadline.ok_or_else(|| io::Error::other("deadline was not armed"))?;
        refuse_expired(expires);
        let (response, stop) = server.handle(&frame);
        refuse_expired(expires);
        if let Some(response) = response {
            write_frame(&mut output, &response, HARD_MAX_FRAME)?;
            output.flush()?;
        }
        refuse_expired(expires);
        watchdog.finish()?;
        if stop {
            break;
        }
    }
    Ok(())
}

fn refuse_expired(deadline: Instant) {
    if Instant::now() > deadline {
        std::process::exit(124);
    }
}
