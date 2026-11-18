#![forbid(unsafe_code)]

mod deadline;
mod rpc;

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
            1,
            0,
            &[Allocation {
                count: u64::try_from(HARD_MAX_FRAME)
                    .map_err(|_| io::Error::other("invalid frame cap"))?,
                element_bytes: 1,
            }],
        )
        .map_err(|_| io::Error::other("invalid frame budget"))?;
        let mut buffers = RequestBuffers::new(plan);
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
        let (response, stop) = server.handle(frame.as_slice());
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
