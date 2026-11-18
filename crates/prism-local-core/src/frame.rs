//! Bounded length-prefixed framing. Payloads are not yet parsed as JSON-RPC.
//!
//! Each frame is a four-byte big-endian length followed by exactly that many
//! bytes. EOF is clean only before the first header byte. Any malformed or
//! truncated frame requires the caller to close the stream, not resynchronise.

use std::io::{self, Read, Write};

pub const HARD_MAX_FRAME: usize = 1_048_576;

/// Read a single frame. `None` is a clean EOF between frames.
pub fn read_frame<R: Read>(reader: &mut R, max: usize) -> io::Result<Option<Vec<u8>>> {
    check_limit(max)?;
    let mut header = [0_u8; 4];
    loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    reader.read_exact(&mut header[1..])?;
    let size = usize::try_from(u32::from_be_bytes(header))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame length unsupported"))?;
    if size == 0 || size > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length out of bounds",
        ));
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(size)
        .map_err(|_| io::Error::other("frame allocation failed"))?;
    payload.resize(size, 0);
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// Write a complete frame. An I/O error leaves the stream unusable.
pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8], max: usize) -> io::Result<()> {
    check_limit(max)?;
    if payload.is_empty() || payload.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame length out of bounds",
        ));
    }
    let size = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame length unsupported"))?;
    writer.write_all(&size.to_be_bytes())?;
    writer.write_all(payload)
}

fn check_limit(max: usize) -> io::Result<()> {
    if max == 0 || max > HARD_MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid frame cap",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{read_frame, write_frame, HARD_MAX_FRAME};
    use std::io::{self, Cursor};

    #[test]
    fn reads_multiple_frames_and_clean_eof() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"first", 16).unwrap();
        write_frame(&mut wire, b"next", 16).unwrap();
        let mut input = Cursor::new(wire);
        assert_eq!(read_frame(&mut input, 16).unwrap().unwrap(), b"first");
        assert_eq!(read_frame(&mut input, 16).unwrap().unwrap(), b"next");
        assert_eq!(read_frame(&mut input, 16).unwrap(), None);
    }

    #[test]
    fn rejects_truncated_oversized_empty_and_invalid_caps() {
        for mut bytes in [&b"\0"[..], &b"\0\0\0\x03ab"[..]] {
            assert_eq!(
                read_frame(&mut bytes, 16).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
        for mut bytes in [&b"\0\0\0\0"[..], &b"\xff\xff\xff\xff"[..]] {
            assert_eq!(
                read_frame(&mut bytes, 16).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
        assert_eq!(
            read_frame(&mut &b""[..], HARD_MAX_FRAME + 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut output = Vec::new();
        assert!(write_frame(&mut output, b"", 16).is_err());
        assert!(output.is_empty());
    }
}
