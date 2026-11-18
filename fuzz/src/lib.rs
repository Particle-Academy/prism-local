//! Bounded stdin adapter for external fuzzing tools.
use std::io::{self, Read};

const MAX_INPUT: u64 = 1_048_576;

pub fn input() -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Ok(None);
    }
    Ok(Some(bytes))
}
