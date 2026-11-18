#![forbid(unsafe_code)]

mod deadline;
mod rpc;
mod rpc_response;

use prism_local_core::{
    frame::{read_frame_budgeted, write_frame, HARD_MAX_FRAME},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};
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
        let plan = checked_plan(
            4,
            0,
            &[Allocation {
                count: 4 * 1024 * 1024,
                element_bytes: 1,
            }],
        )
        .map_err(|_| io::Error::other("invalid request payload budget"))?;
        let mut request_buffers = RequestBuffers::new(plan);
        let mut buffers = request_buffers
            .split(1024 * 1024)
            .map_err(|_| io::Error::other("invalid frame budget"))?;
        let frame = read_frame_budgeted(
            &mut input,
            HARD_MAX_FRAME,
            || {
                active_deadline = Some(watchdog.start(server.timeout())?);
                Ok(())
            },
            &mut buffers,
        )?;
        let Some(frame) = frame else { break };
        let expires = active_deadline.ok_or_else(|| io::Error::other("deadline was not armed"))?;
        refuse_expired(expires);
        let mut arena_buffers = request_buffers
            .split(2 * 1024 * 1024)
            .map_err(|_| io::Error::other("invalid JSON arena budget"))?;
        let mut response_buffers = request_buffers
            .split(1024 * 1024)
            .map_err(|_| io::Error::other("invalid response budget"))?;
        let (response, stop) = server
            .handle_budgeted(frame.as_slice(), &mut arena_buffers, &mut response_buffers)
            .map_err(|_| io::Error::other("response budget or allocation unavailable"))?;
        refuse_expired(expires);
        if let Some(response) = response {
            write_frame(&mut output, response.as_slice(), HARD_MAX_FRAME)?;
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
