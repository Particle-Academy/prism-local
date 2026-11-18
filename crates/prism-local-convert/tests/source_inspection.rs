use prism_local_convert::safetensors::{inspect, Dtype, Error, TensorSpec};
use prism_local_core::sha256;

fn source(header: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn schema() -> [TensorSpec<'static>; 2] {
    [
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
    ]
}

const HEADER: &str = r#"{"__metadata__":{"format":"pt"},"weight":{"dtype":"F32","shape":[2],"data_offsets":[0,8]},"position":{"dtype":"I64","shape":[2],"data_offsets":[8,24]}}"#;

#[test]
fn authenticates_and_returns_only_checked_slices_in_schema_order() {
    let data: Vec<u8> = (0..24).collect();
    let mut bytes = source(HEADER, &data);
    let digest = sha256::digest(&bytes).unwrap();
    let checked = inspect(&bytes, &digest, &schema(), Some("pt")).unwrap();
    assert_eq!(checked.tensor(0), Some(&data[8..24]));
    assert_eq!(checked.tensor(1), Some(&data[0..8]));
    assert_eq!(checked.tensor(2), None);
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    assert!(matches!(
        inspect(&bytes, &digest, &schema(), Some("pt")),
        Err(Error::DigestMismatch)
    ));
}

#[test]
fn rejects_untrusted_header_and_data_mutations_even_if_hash_matches() {
    let cases = [
        HEADER.replace("\"F32\"", "\"F16\""),
        HEADER.replace("\"shape\":[2]", "\"shape\":[3]"),
        HEADER.replace("[8,24]", "[0,16]"),
        HEADER.replace("[8,24]", "[18446744073709551615,0]"),
        HEADER.replace("[8,24]", "[9,25]"),
        HEADER.replace("\"format\":\"pt\"", "\"format\":\"pt\",\"other\":\"x\""),
        HEADER.replace("\"dtype\":\"F32\"", "\"dtype\":\"F32\",\"dtype\":\"F32\""),
        HEADER.replace("\"weight\"", "\"unknown\""),
        HEADER.replace("[0,8]", "[0,8,8]"),
        format!(" {HEADER}"),
        format!("{HEADER}\n"),
        HEADER.replace("[0,8]", "[-1,8]"),
    ];
    for header in cases {
        let bytes = source(&header, &[0; 24]);
        assert!(
            matches!(
                inspect(
                    &bytes,
                    &sha256::digest(&bytes).unwrap(),
                    &schema(),
                    Some("pt")
                ),
                Err(Error::InvalidSource)
            ),
            "{header}"
        );
    }
    for bytes in [
        source(HEADER, &[0; 23]),
        source(HEADER, &[0; 25]),
        source(&HEADER.replace("[8,24]", "[9,25]"), &[0; 25]),
        u64::MAX.to_le_bytes().to_vec(),
    ] {
        assert!(matches!(
            inspect(
                &bytes,
                &sha256::digest(&bytes).unwrap(),
                &schema(),
                Some("pt")
            ),
            Err(Error::InvalidSource)
        ));
    }
}

#[test]
fn rejects_shape_product_overflow_and_accepts_space_padding() {
    let padded = source(&format!("{HEADER}   "), &[0; 24]);
    assert!(inspect(
        &padded,
        &sha256::digest(&padded).unwrap(),
        &schema(),
        Some("pt")
    )
    .is_ok());
    let shape = [u64::MAX, 2];
    let spec = [TensorSpec {
        name: "x",
        dtype: Dtype::F32,
        shape: &shape,
    }];
    let bytes = source(
        r#"{"x":{"dtype":"F32","shape":[18446744073709551615,2],"data_offsets":[0,0]}}"#,
        &[],
    );
    assert!(matches!(
        inspect(&bytes, &sha256::digest(&bytes).unwrap(), &spec, None),
        Err(Error::InvalidSource)
    ));
}

#[test]
fn rejects_invalid_trusted_schemas() {
    let bytes = source(HEADER, &[0; 24]);
    let hash = sha256::digest(&bytes).unwrap();
    let duplicate = [
        TensorSpec {
            name: "weight",
            dtype: Dtype::F32,
            shape: &[2],
        },
        TensorSpec {
            name: "weight",
            dtype: Dtype::F32,
            shape: &[2],
        },
    ];
    assert!(matches!(
        inspect(&bytes, &hash, &duplicate, Some("pt")),
        Err(Error::InvalidSchema)
    ));
    assert!(matches!(
        inspect(&bytes, &hash, &[], Some("pt")),
        Err(Error::InvalidSchema)
    ));
    let zero = [TensorSpec {
        name: "weight",
        dtype: Dtype::F32,
        shape: &[0],
    }];
    assert!(matches!(
        inspect(&bytes, &hash, &zero, Some("pt")),
        Err(Error::InvalidSchema)
    ));
}
