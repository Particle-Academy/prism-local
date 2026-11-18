use prism_local_core::{
    json::{parse, string_output_len, write_string, Value},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        if bytes.len() > 65_536 {
            return Ok(());
        }
        if let Ok(value) = std::str::from_utf8(&bytes) {
            if let Some(length) = string_output_len(value) {
                let plan = checked_plan(
                    1,
                    0,
                    &[Allocation {
                        count: 1_048_576,
                        element_bytes: 1,
                    }],
                )
                .unwrap();
                let mut buffers = RequestBuffers::new(plan);
                if let Ok(mut output) = buffers.allocate::<u8>(length as u64) {
                    let written = write_string(value, output.as_mut_slice()).expect("sized output");
                    assert!(
                        matches!(parse(&output.as_slice()[..written]), Ok(Value::String(decoded)) if decoded == value)
                    );
                }
            }
        }
    }
    Ok(())
}
