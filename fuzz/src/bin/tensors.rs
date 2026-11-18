use prism_local_core::tensors::{
    check_values_with_scratch, check_with_scratch, Dtype, TensorSpec, ValueRule,
};

fn main() -> std::io::Result<()> {
    let specs = [
        TensorSpec {
            id: 1,
            dtype: Dtype::I8,
            dims: [2, 4, 0, 0],
        },
        TensorSpec {
            id: 2,
            dtype: Dtype::F32,
            dims: [4, 0, 0, 0],
        },
    ];
    if let Some(bytes) = prism_local_fuzz::input()? {
        let mut scratch = [0_u64; 4];
        let _ = check_with_scratch(&bytes, 1024, &specs, &mut scratch);
        if let Some((directory, data)) = bytes.split_at_checked(80) {
            let _ = check_values_with_scratch(
                directory,
                data,
                &specs,
                &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero],
                &mut scratch,
            );
        }
    }
    Ok(())
}
