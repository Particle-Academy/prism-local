//! Inspect authenticated safetensors bytes against a trusted source schema.
//!
//! This is converter-side inspection, never an engine model loader. It checks
//! a deliberately narrow subset: F32 and I64 tensors of rank 1 through 4.
//! Tensor contents, architecture semantics, licensing and quantization require
//! separate validation. This module does not read files or perform downloads.

use prism_local_core::{frame::HARD_MAX_FRAME, json, json::Value, sha256};

const MAX_TENSORS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dtype {
    F32,
    I64,
}

impl Dtype {
    fn name(self) -> &'static str {
        match self {
            Self::F32 => "F32",
            Self::I64 => "I64",
        }
    }
    fn size(self) -> u64 {
        match self {
            Self::F32 => 4,
            Self::I64 => 8,
        }
    }
}

/// Trusted converter-owned expectations; never derive these from the source.
pub struct TensorSpec<'a> {
    pub name: &'a str,
    pub dtype: Dtype,
    pub shape: &'a [u64],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    DigestMismatch,
    InvalidSchema,
    InvalidSource,
}

/// Structurally inspected bytes, not a validated or converted model.
pub struct InspectedSource<'a> {
    tensors: Vec<&'a [u8]>,
}

impl<'a> InspectedSource<'a> {
    /// Tensor payload in trusted schema order, borrowing the hashed bytes.
    pub fn tensor(&self, index: usize) -> Option<&'a [u8]> {
        self.tensors.get(index).copied()
    }
}

/// Hash the exact immutable input, then inspect its header and data ranges.
///
/// The caller must impose a file-size limit before buffering. `expected_format`
/// specifies an exact one-field metadata object, or no metadata when `None`.
/// Successful inspection does not authorize conversion or inference.
pub fn inspect<'a>(
    bytes: &'a [u8],
    digest: &[u8; 32],
    specs: &[TensorSpec<'_>],
    expected_format: Option<&str>,
) -> Result<InspectedSource<'a>, Error> {
    if specs.is_empty() || specs.len() > MAX_TENSORS {
        return Err(Error::InvalidSchema);
    }
    for (i, spec) in specs.iter().enumerate() {
        if spec.name.is_empty()
            || spec.name == "__metadata__"
            || spec.shape.is_empty()
            || spec.shape.len() > 4
            || spec.shape.contains(&0)
            || specs[..i].iter().any(|prior| prior.name == spec.name)
        {
            return Err(Error::InvalidSchema);
        }
    }
    if sha256::digest(bytes).as_ref() != Some(digest) {
        return Err(Error::DigestMismatch);
    }
    let header_len = usize::try_from(u64::from_le_bytes(
        bytes
            .get(..8)
            .ok_or(Error::InvalidSource)?
            .try_into()
            .map_err(|_| Error::InvalidSource)?,
    ))
    .map_err(|_| Error::InvalidSource)?;
    if header_len == 0 || header_len > HARD_MAX_FRAME {
        return Err(Error::InvalidSource);
    }
    let data_start = 8_usize
        .checked_add(header_len)
        .ok_or(Error::InvalidSource)?;
    let header = bytes.get(8..data_start).ok_or(Error::InvalidSource)?;
    let data = bytes.get(data_start..).ok_or(Error::InvalidSource)?;
    let end = header
        .iter()
        .rposition(|&byte| byte != b' ')
        .ok_or(Error::InvalidSource)?
        + 1;
    if header.first() != Some(&b'{') || header.get(end - 1) != Some(&b'}') {
        return Err(Error::InvalidSource);
    }
    let Value::Object(mut entries) =
        json::parse(&header[..end]).map_err(|_| Error::InvalidSource)?
    else {
        return Err(Error::InvalidSource);
    };
    match (expected_format, entries.remove("__metadata__")) {
        (None, None) => {}
        (Some(expected), Some(Value::Object(metadata)))
            if metadata.len() == 1
                && matches!(metadata.get("format"), Some(Value::String(actual)) if actual == expected) =>
            {}
        _ => return Err(Error::InvalidSource),
    }
    if entries.len() != specs.len() {
        return Err(Error::InvalidSource);
    }
    let mut tensors = Vec::new();
    let mut ranges = Vec::new();
    tensors
        .try_reserve_exact(specs.len())
        .map_err(|_| Error::InvalidSource)?;
    ranges
        .try_reserve_exact(specs.len())
        .map_err(|_| Error::InvalidSource)?;
    for spec in specs {
        let Some(Value::Object(mut entry)) = entries.remove(spec.name) else {
            return Err(Error::InvalidSource);
        };
        if entry.remove("dtype") != Some(Value::String(spec.dtype.name().to_owned())) {
            return Err(Error::InvalidSource);
        }
        let Some(Value::Array(shape)) = entry.remove("shape") else {
            return Err(Error::InvalidSource);
        };
        if shape.len() != spec.shape.len() {
            return Err(Error::InvalidSource);
        }
        let mut elements = 1_u64;
        for (actual, &expected) in shape.iter().zip(spec.shape) {
            if number(actual)? != expected {
                return Err(Error::InvalidSource);
            }
            elements = elements.checked_mul(expected).ok_or(Error::InvalidSource)?;
        }
        let length = elements
            .checked_mul(spec.dtype.size())
            .ok_or(Error::InvalidSource)?;
        let Some(Value::Array(offsets)) = entry.remove("data_offsets") else {
            return Err(Error::InvalidSource);
        };
        if !entry.is_empty() || offsets.len() != 2 {
            return Err(Error::InvalidSource);
        }
        let start = number(&offsets[0])?;
        let end = number(&offsets[1])?;
        if start.checked_add(length) != Some(end) {
            return Err(Error::InvalidSource);
        }
        let start = usize::try_from(start).map_err(|_| Error::InvalidSource)?;
        let end = usize::try_from(end).map_err(|_| Error::InvalidSource)?;
        tensors.push(data.get(start..end).ok_or(Error::InvalidSource)?);
        ranges.push((start, end));
    }
    ranges.sort_unstable();
    let mut next = 0;
    for (start, end) in ranges {
        if start != next {
            return Err(Error::InvalidSource);
        }
        next = end;
    }
    if next != data.len() {
        return Err(Error::InvalidSource);
    }
    Ok(InspectedSource { tensors })
}

fn number(value: &Value) -> Result<u64, Error> {
    let Value::Number(number) = value else {
        return Err(Error::InvalidSource);
    };
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidSource);
    }
    number.parse().map_err(|_| Error::InvalidSource)
}
