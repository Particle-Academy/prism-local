//! Integer operations shared by all future execution paths.

/// An exact dot product for signed 8-bit inputs, rejecting length mismatch or overflow.
///
/// Callers must not split a dot product across workers. i64 allows more terms than
/// any supported model will require while still detecting unbounded input overflow.
pub fn dot_i8(left: &[i8], right: &[i8]) -> Option<i64> {
    if left.len() != right.len() {
        return None;
    }
    left.iter().zip(right).try_fold(0_i64, |sum, (&a, &b)| {
        sum.checked_add(i64::from(a) * i64::from(b))
    })
}

#[cfg(test)]
mod tests {
    use super::dot_i8;

    #[test]
    fn signed_extremes_and_empty_vectors() {
        assert_eq!(dot_i8(&[-128, 127, -128], &[-128, 127, 127]), Some(16257));
        assert_eq!(dot_i8(&[], &[]), Some(0));
        assert_eq!(dot_i8(&[1], &[]), None);
    }

    #[test]
    fn does_not_saturate_intermediate_results() {
        let v = vec![-128; 200_000];
        assert_eq!(dot_i8(&v, &v), Some(3_276_800_000));
    }
}
