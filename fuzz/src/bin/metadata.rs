use prism_local_core::metadata::{check_bert, BertSpec};

fn main() -> std::io::Result<()> {
    let expected = BertSpec {
        layers: 12,
        hidden: 384,
        heads: 12,
        intermediate: 1536,
        vocab: 30522,
        max_positions: 512,
        token_types: 2,
        layer_norm_epsilon_bits: 1e-12_f32.to_bits(),
    };
    if let Some(bytes) = prism_local_fuzz::input()? {
        let _ = check_bert(&bytes, expected);
    }
    Ok(())
}
