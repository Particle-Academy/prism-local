use prism_local_core::{
    json::{decode_string_into, parse, Value},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        if let Some((prefix, token)) = bytes.split_at_checked(2) {
            let budget = u64::from(u16::from_le_bytes([prefix[0], prefix[1]]));
            let plan = checked_plan(
                1,
                0,
                &[Allocation {
                    count: 65_536,
                    element_bytes: 1,
                }],
            )
            .unwrap();
            let mut buffers = RequestBuffers::new(plan);
            if let Ok(mut output) = buffers.allocate::<u8>(budget) {
                if let Ok(written) = decode_string_into(token, output.as_mut_slice()) {
                    assert!(
                        matches!(parse(token), Ok(Value::String(decoded)) if decoded.as_bytes() == &output.as_slice()[..written])
                    );
                }
            }
        }
    }
    Ok(())
}
