//! Checked arithmetic for a future request-allocation plan.
//!
//! This does not allocate memory or enforce a process limit. Every allocation
//! path must be connected and audited before this can support a memory cap.

const MIB: u64 = 1024 * 1024;

/// A simultaneously live payload: `count` elements of `element_bytes` each.
#[derive(Clone, Copy)]
pub struct Allocation {
    pub count: u64,
    pub element_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Overflow,
    ExceedsBudget,
}

/// A conservative sum of the caller-specified simultaneously live payloads.
/// It excludes allocator overhead, runtime stacks and unlisted allocations.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub resident_bytes: u64,
    pub request_bytes: u64,
    pub total_bytes: u64,
}

/// Compute the full plan before a request begins, rejecting overflow and excess.
/// `resident_bytes` must include every already-live payload charged to the
/// limit. Every request allocation that can overlap must appear in `request`.
/// This arithmetic alone does not enforce the plan or make a request safe.
pub fn checked_plan(
    max_memory_mib: u64,
    resident_bytes: u64,
    request: &[Allocation],
) -> Result<Plan, Error> {
    if max_memory_mib == 0 {
        return Err(Error::Invalid);
    }
    let budget = max_memory_mib.checked_mul(MIB).ok_or(Error::Overflow)?;
    let mut request_bytes = 0_u64;
    for item in request {
        if item.element_bytes == 0 {
            return Err(Error::Invalid);
        }
        let bytes = item
            .count
            .checked_mul(item.element_bytes)
            .ok_or(Error::Overflow)?;
        request_bytes = request_bytes.checked_add(bytes).ok_or(Error::Overflow)?;
    }
    let total_bytes = resident_bytes
        .checked_add(request_bytes)
        .ok_or(Error::Overflow)?;
    if total_bytes > budget {
        return Err(Error::ExceedsBudget);
    }
    Ok(Plan {
        resident_bytes,
        request_bytes,
        total_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::{checked_plan, Allocation, Error, Plan};

    #[test]
    fn exact_limit_is_accepted_and_a_single_extra_byte_is_refused() {
        let items = [
            Allocation {
                count: 256,
                element_bytes: 1024,
            },
            Allocation {
                count: 1,
                element_bytes: 1,
            },
        ];
        let resident = 1024 * 1024 - 256 * 1024 - 1;
        assert_eq!(
            checked_plan(1, resident, &items),
            Ok(Plan {
                resident_bytes: resident,
                request_bytes: 256 * 1024 + 1,
                total_bytes: 1024 * 1024,
            })
        );
        assert_eq!(
            checked_plan(1, resident + 1, &items),
            Err(Error::ExceedsBudget)
        );
        assert_eq!(
            checked_plan(1, 1024 * 1024 + 1, &[]),
            Err(Error::ExceedsBudget)
        );
    }

    #[test]
    fn detects_all_overflow_paths_before_approving_a_plan() {
        assert_eq!(checked_plan(u64::MAX, 0, &[]), Err(Error::Overflow));
        assert_eq!(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: u64::MAX,
                    element_bytes: 2
                }]
            ),
            Err(Error::Overflow)
        );
        assert_eq!(
            checked_plan(
                1,
                0,
                &[
                    Allocation {
                        count: u64::MAX,
                        element_bytes: 1
                    },
                    Allocation {
                        count: 1,
                        element_bytes: 1
                    },
                ]
            ),
            Err(Error::Overflow)
        );
        assert_eq!(
            checked_plan(
                1,
                u64::MAX,
                &[Allocation {
                    count: 1,
                    element_bytes: 1
                }]
            ),
            Err(Error::Overflow)
        );
        assert_eq!(checked_plan(0, 0, &[]), Err(Error::Invalid));
        assert_eq!(
            checked_plan(
                1,
                0,
                &[Allocation {
                    count: 0,
                    element_bytes: 0
                }]
            ),
            Err(Error::Invalid)
        );
    }
}
