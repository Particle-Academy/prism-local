#![forbid(unsafe_code)]

//! Foundations for the local engine. Model loading and inference are not yet implemented.

pub mod frame;
pub mod golden;
pub mod json;
pub mod json_arena;
pub mod math;
pub mod memory_plan;
pub mod metadata;
pub mod model_checks;
pub mod plm;
pub mod provenance;
pub mod sha256;
pub mod tensors;
pub mod vocab;

#[cfg(test)]
mod test_support {
    pub fn mutate(seed: &[u8], mut check: impl FnMut(&[u8])) {
        if seed.is_empty() {
            check(seed);
            return;
        }
        let mut state = 0x9e37_79b9_u32;
        for iteration in 0..2048 {
            let mut bytes = seed.to_vec();
            for _ in 0..4 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let index = state as usize % bytes.len();
                bytes[index] ^= (state >> 24) as u8;
            }
            if iteration % 8 == 0 {
                bytes.truncate(state as usize % bytes.len());
            }
            check(&bytes);
        }
    }
}
