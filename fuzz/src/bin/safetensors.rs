use prism_local_convert::safetensors::{inspect, Dtype, TensorSpec};
use prism_local_core::sha256;

fn main() -> std::io::Result<()> {
    let specs = [
        TensorSpec {
            name: "position",
            dtype: Dtype::I64,
            shape: &[2],
        },
        TensorSpec {
            name: "weight",
            dtype: Dtype::F32,
            shape: &[2],
        },
    ];
    if let Some(bytes) = prism_local_fuzz::input()? {
        if let Some(hash) = sha256::digest(&bytes) {
            let _ = inspect(&bytes, &hash, &specs, Some("pt"));
        }
    }
    Ok(())
}
