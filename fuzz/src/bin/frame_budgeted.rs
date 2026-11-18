use prism_local_core::{
    frame::{read_frame_budgeted, HARD_MAX_FRAME},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        if let Some((prefix, wire)) = bytes.split_at_checked(2) {
            let budget = u64::from(u16::from_le_bytes([prefix[0], prefix[1]]));
            if let Ok(plan) = checked_plan(
                1,
                0,
                &[Allocation {
                    count: budget,
                    element_bytes: 1,
                }],
            ) {
                let mut buffers = RequestBuffers::new(plan);
                let mut input = wire;
                let _ = read_frame_budgeted(&mut input, HARD_MAX_FRAME, || Ok(()), &mut buffers);
            }
        }
    }
    Ok(())
}
