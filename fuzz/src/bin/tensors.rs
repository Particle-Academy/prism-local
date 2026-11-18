use prism_local_core::tensors::{check, check_values, Dtype, TensorSpec, ValueRule};

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
        let _ = check(&bytes, 1024, &specs);
        if let Some((directory, data)) = bytes.split_at_checked(80) {
            let _ = check_values(
                directory,
                data,
                &specs,
                &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero],
            );
        }
    }
    Ok(())
}
