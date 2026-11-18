//! Bounded length-prefixed framing. Payloads are not yet parsed as JSON-RPC.
//!
//! Each frame is a four-byte big-endian length followed by exactly that many
//! bytes. EOF is clean only before the first header byte. Any malformed or
//! truncated frame requires the caller to close the stream, not resynchronise.

use std::io::{self, Read, Write};

pub const HARD_MAX_FRAME: usize = 1_048_576;

/// Read a single frame. `None` is a clean EOF between frames.
pub fn read_frame<R: Read>(reader: &mut R, max: usize) -> io::Result<Option<Vec<u8>>> {
    read_frame_started(reader, max, || Ok(()))
}

/// As `read_frame`, calling `on_start` after the first header byte is read.
/// The callback can arm a deadline without imposing one on an idle stream.
pub fn read_frame_started<R: Read>(
    reader: &mut R,
    max: usize,
    on_start: impl FnOnce() -> io::Result<()>,
) -> io::Result<Option<Vec<u8>>> {
    read_frame_checked(reader, max, on_start, |_| Ok(()))
}

/// As `read_frame_started`, with an admission check after reading and
/// validating the length but before allocating or reading the frame body.
/// The caller is responsible for accounting for all other request allocations.
pub fn read_frame_checked<R: Read>(
    reader: &mut R,
    max: usize,
    on_start: impl FnOnce() -> io::Result<()>,
    on_size: impl FnOnce(usize) -> io::Result<()>,
) -> io::Result<Option<Vec<u8>>> {
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
    on_start()?;
    reader.read_exact(&mut header[1..])?;
    let size = usize::try_from(u32::from_be_bytes(header))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame length unsupported"))?;
    if size == 0 || size > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length out of bounds",
        ));
    }
    on_size(size)?;
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
    use super::{read_frame, read_frame_checked, read_frame_started, write_frame, HARD_MAX_FRAME};
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

    #[test]
    fn signals_start_only_after_first_byte() {
        let mut starts = 0;
        assert_eq!(
            read_frame_started(&mut &b""[..], 16, || {
                starts += 1;
                Ok(())
            })
            .unwrap(),
            None
        );
        assert_eq!(starts, 0);
        assert!(read_frame_started(&mut &b"\0"[..], 16, || {
            starts += 1;
            Ok(())
        })
        .is_err());
        assert_eq!(starts, 1);
    }

    #[test]
    fn refuses_to_read_body_when_deadline_cannot_be_armed() {
        let mut input = &b"\0\0\0\x01x"[..];
        let error = read_frame_started(&mut input, 16, || Err(io::Error::other("unavailable")))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(input, &b"\0\0\x01x"[..]);
    }

    #[test]
    fn admission_refusal_happens_before_body_allocation_or_read() {
        let mut input = &b"\0\0\0\x03abc"[..];
        let mut observed = None;
        let result = read_frame_checked(
            &mut input,
            16,
            || Ok(()),
            |size| {
                observed = Some(size);
                Err(io::Error::other("not admitted"))
            },
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(observed, Some(3));
        assert_eq!(input, &b"abc"[..]);
        let mut oversized = &b"\0\0\0\x11"[..];
        assert!(read_frame_checked(
            &mut oversized,
            16,
            || Ok(()),
            |_| panic!("invalid size must not be admitted")
        )
        .is_err());
    }
}
