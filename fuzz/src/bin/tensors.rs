use prism_local_core::tensors::{check, Dtype, TensorSpec};

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
    }
    Ok(())
}
