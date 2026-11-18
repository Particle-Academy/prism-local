#![forbid(unsafe_code)]

mod rpc;

use prism_local_core::frame::{read_frame, write_frame, HARD_MAX_FRAME};
use std::io::{self, Write};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut server = rpc::Server::new();
    while let Some(frame) = read_frame(&mut input, HARD_MAX_FRAME)? {
        let (response, stop) = server.handle(&frame);
        if let Some(response) = response {
            write_frame(&mut output, &response, HARD_MAX_FRAME)?;
            output.flush()?;
        }
        if stop {
            break;
        }
    }
    Ok(())
}
