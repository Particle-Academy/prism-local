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

#[derive(Debug, PartialEq, Eq)]
pub enum RowDotError {
    InvalidShape,
    InvalidThreads,
    Allocation,
    Worker,
    Overflow,
}

/// Experimental row-parallel dot products, with no cross-worker reductions.
///
/// Each row is accumulated sequentially with `dot_i8`. This call's threads
/// cannot exceed available parallelism, but concurrent calls have no shared
/// worker cap. It allocates an output vector without request budgeting and is
/// not an inference or resource-enforcement interface.
pub fn dot_rows_i8(matrix: &[i8], input: &[i8], threads: usize) -> Result<Vec<i64>, RowDotError> {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    if threads == 0 || threads > cores {
        return Err(RowDotError::InvalidThreads);
    }
    if input.is_empty() || matrix.is_empty() || !matrix.len().is_multiple_of(input.len()) {
        return Err(RowDotError::InvalidShape);
    }
    let rows = matrix.len() / input.len();
    let workers = threads.min(rows);
    let rows_per_worker = rows.div_ceil(workers);
    let bytes_per_worker = rows_per_worker
        .checked_mul(input.len())
        .ok_or(RowDotError::Overflow)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows)
        .map_err(|_| RowDotError::Allocation)?;
    output.resize(rows, 0);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(workers)
            .map_err(|_| RowDotError::Allocation)?;
        let mut error = None;
        for (out, source) in output
            .chunks_mut(rows_per_worker)
            .zip(matrix.chunks(bytes_per_worker))
        {
            match std::thread::Builder::new().spawn_scoped(scope, move || {
                for (slot, row) in out.iter_mut().zip(source.chunks_exact(input.len())) {
                    *slot = dot_i8(row, input).ok_or(RowDotError::Overflow)?;
                }
                Ok(())
            }) {
                Ok(handle) => handles.push(handle),
                Err(_) => {
                    error = Some(RowDotError::Worker);
                    break;
                }
            }
        }
        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(failure)) => error = Some(failure),
                Err(_) => error = Some(RowDotError::Worker),
            }
        }
        error.map_or(Ok(()), Err)
    })?;
    Ok(output)
}

/// Scalar approximation for finite, normal inputs in [-80, 80], plus zero.
/// This is an experimental foundation; inference and cross-CPU equivalence
/// require separate quality and floating-point-environment checks.
pub fn exp_f32(x: f32) -> Option<f32> {
    if !supported(x) || !(-80.0..=80.0).contains(&x) {
        return None;
    }
    let k = (x * 1.442_695_1_f32).round() as i32;
    let kf = k as f32;
    let reduced = (-kf).mul_add(0.693_145_75_f32, x);
    let r = (-kf).mul_add(1.428_606_8e-6_f32, reduced);
    let mut p = 1.0_f32 / 40_320.0;
    for coefficient in [
        1.0 / 5_040.0,
        1.0 / 720.0,
        1.0 / 120.0,
        1.0 / 24.0,
        1.0 / 6.0,
        0.5,
        1.0,
        1.0,
    ] {
        p = p.mul_add(r, coefficient);
    }
    let scale = f32::from_bits(((k + 127) as u32) << 23);
    Some(p * scale)
}

/// Scalar erf approximation. Nonfinite and nonzero subnormal inputs are refused.
pub fn erf_f32(x: f32) -> Option<f32> {
    if !supported(x) {
        return None;
    }
    if x == 0.0 {
        return Some(x);
    }
    let a = x.abs();
    if a >= 8.0 {
        return Some(x.signum());
    }
    if a < 0.0001 {
        return Some(x * std::f32::consts::FRAC_2_SQRT_PI);
    }
    let t = 1.0 / 0.327_591_1_f32.mul_add(a, 1.0);
    let mut p = 1.061_405_4_f32;
    for c in [-1.453_152_f32, 1.421_413_8, -0.284_496_72, 0.254_829_6] {
        p = p.mul_add(t, c);
    }
    let y = (-(p * t)).mul_add(exp_f32(-(a * a))?, 1.0);
    Some(y.copysign(x))
}

/// Scalar tanh approximation. Nonfinite and nonzero subnormal inputs are refused.
pub fn tanh_f32(x: f32) -> Option<f32> {
    if !supported(x) {
        return None;
    }
    if x == 0.0 {
        return Some(x);
    }
    let a = x.abs();
    if a >= 9.0 {
        return Some(x.signum());
    }
    if a < 0.0001 {
        return Some(x);
    }
    let e = exp_f32(-2.0 * a)?;
    Some(((1.0 - e) / (1.0 + e)).copysign(x))
}

fn supported(x: f32) -> bool {
    x.is_finite() && (x == 0.0 || x.is_normal())
}

#[cfg(test)]
mod tests {
    use super::{dot_i8, dot_rows_i8, erf_f32, exp_f32, tanh_f32, RowDotError};

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

    #[test]
    fn row_dot_products_keep_exact_results_across_worker_counts() {
        let input = [-128_i8, 127, 17, -9, 0];
        let matrix = [
            -128, 127, -128, 127, 127, 0, 0, 0, 0, 0, 127, -128, 17, 9, -1, 1, 2, 3, 4, 5, -1, -2,
            -3, -4, -5, 4, 3, 2, 1, 0,
        ];
        let expected: Vec<_> = matrix
            .chunks_exact(input.len())
            .map(|row| dot_i8(row, &input).unwrap())
            .collect();
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        for workers in 1..=cores.min(6) {
            assert_eq!(dot_rows_i8(&matrix, &input, workers), Ok(expected.clone()));
        }
        let long_input = vec![-128_i8; 200_000];
        let long_matrix = vec![-128_i8; 400_000];
        for workers in 1..=cores.min(2) {
            assert_eq!(
                dot_rows_i8(&long_matrix, &long_input, workers),
                Ok(vec![3_276_800_000, 3_276_800_000])
            );
        }
        assert_eq!(
            dot_rows_i8(&matrix, &input, 0),
            Err(RowDotError::InvalidThreads)
        );
        assert_eq!(
            dot_rows_i8(&matrix, &input, cores.saturating_add(1)),
            Err(RowDotError::InvalidThreads)
        );
        assert_eq!(
            dot_rows_i8(&matrix[..29], &input, 1),
            Err(RowDotError::InvalidShape)
        );
        assert_eq!(dot_rows_i8(&matrix, &[], 1), Err(RowDotError::InvalidShape));
    }

    #[test]
    fn approximations_match_decimal_reference() {
        let fixture = include_str!("../tests/fixtures/math-reference.csv");
        for line in fixture.lines().skip(1) {
            let mut fields = line.split(',');
            let name = fields.next().unwrap();
            let x: f32 = fields.next().unwrap().parse().unwrap();
            let expected: f32 = fields.next().unwrap().parse().unwrap();
            assert!(fields.next().is_none());
            let actual = match name {
                "exp" => exp_f32(x).unwrap(),
                "erf" => erf_f32(x).unwrap(),
                "tanh" => tanh_f32(x).unwrap(),
                _ => panic!("unknown fixture function"),
            };
            let tolerance = if name == "exp" {
                expected.abs() * 0.000_003
            } else {
                0.000_000_3
            };
            assert!(
                (actual - expected).abs() <= tolerance,
                "{name}({x}): {actual} vs {expected}"
            );
        }
    }

    #[test]
    fn approximations_refuse_unsupported_inputs_and_preserve_signed_zero() {
        for x in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::from_bits(1),
            -f32::from_bits(1),
        ] {
            assert_eq!(exp_f32(x), None);
            assert_eq!(erf_f32(x), None);
            assert_eq!(tanh_f32(x), None);
        }
        assert_eq!(exp_f32(81.0), None);
        assert_eq!(exp_f32(-81.0), None);
        assert_eq!(exp_f32(-0.0), Some(1.0));
        assert_eq!(erf_f32(-0.0).unwrap().to_bits(), (-0.0_f32).to_bits());
        assert_eq!(tanh_f32(-0.0).unwrap().to_bits(), (-0.0_f32).to_bits());
    }
}
