#![deny(unsafe_code)]

mod deadline;
#[allow(unsafe_code)]
mod process_cap;
mod rpc;
mod rpc_response;

use prism_local_core::{
    frame::{read_frame_budgeted, write_frame, HARD_MAX_FRAME},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};
use std::io::{self, Write};
use std::time::Instant;

#[global_allocator]
static ALLOCATOR: process_cap::ExitOnAllocationFailure = process_cap::ExitOnAllocationFailure;

fn main() -> io::Result<()> {
    let Some(payload_mib) = startup_budget() else {
        eprintln!("invalid process memory configuration");
        std::process::exit(process_cap::MEMORY_EXIT);
    };
    let Some(limit) = process_cap::limit_bytes(payload_mib) else {
        eprintln!("invalid process memory configuration");
        std::process::exit(process_cap::MEMORY_EXIT);
    };
    let Ok(_process_cap) = process_cap::install(limit) else {
        eprintln!("process memory limit unavailable");
        std::process::exit(process_cap::MEMORY_EXIT);
    };
    eprintln!(
        "processMemoryLimitMb={} memoryScope=platformHardLimit",
        limit / (1024 * 1024)
    );
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut server = rpc::Server::with_startup_budget(payload_mib);
    let watchdog = deadline::Watchdog::new()?;
    loop {
        let mut active_deadline = None;
        let plan = checked_plan(
            server.payload_memory_mib(),
            0,
            &[Allocation {
                count: rpc::REQUEST_PAYLOAD_BYTES,
                element_bytes: 1,
            }],
        )
        .map_err(|_| io::Error::other("invalid request payload budget"))?;
        let mut request_buffers = RequestBuffers::new(plan);
        let mut buffers = request_buffers
            .split(rpc::FRAME_PAYLOAD_BYTES)
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
            .split(rpc::ARENA_PAYLOAD_BYTES)
            .map_err(|_| io::Error::other("invalid JSON arena budget"))?;
        let mut response_buffers = request_buffers
            .split(rpc::RESPONSE_PAYLOAD_BYTES)
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

fn startup_budget() -> Option<u64> {
    let mut args = std::env::args_os();
    args.next()?;
    let budget = match args.next() {
        None => rpc::DEFAULT_MEMORY_MB,
        Some(name) if name == "--max-memory-mb" => {
            let value = args.next()?;
            let text = value.to_str()?;
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            text.parse().ok()?
        }
        Some(_) => return None,
    };
    if args.next().is_some()
        || budget < rpc::MIN_MEMORY_MB
        || process_cap::limit_bytes(budget).is_none()
    {
        return None;
    }
    Some(budget)
}

fn refuse_expired(deadline: Instant) {
    if Instant::now() > deadline {
        std::process::exit(124);
    }
}
