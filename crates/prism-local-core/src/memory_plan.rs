//! Checked arithmetic for a future request-allocation plan.
//!
//! Optional fixed-capacity payload buffers use a checked plan. This does not
//! enforce a process limit: every allocation path must be connected and audited
//! before it can support a request memory cap.

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
    Allocation,
}

/// A conservative sum of the caller-specified simultaneously live payloads.
/// It excludes allocator overhead, runtime stacks and unlisted allocations.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    resident_bytes: u64,
    request_bytes: u64,
    total_bytes: u64,
}

impl Plan {
    pub fn resident_bytes(&self) -> u64 {
        self.resident_bytes
    }
    pub fn request_bytes(&self) -> u64 {
        self.request_bytes
    }
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
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

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for i8 {}
    impl Sealed for u16 {}
    impl Sealed for u64 {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}
    impl Sealed for f32 {}
}

/// Primitive numeric payload types with allocation-free zero defaults.
pub trait Element: sealed::Sealed + Copy + Default {}
impl Element for u8 {}
impl Element for i8 {}
impl Element for u16 {}
impl Element for u64 {}
impl Element for i32 {}
impl Element for i64 {}
impl Element for f32 {}

/// Fixed-size storage: no operation exposes the backing vector or can grow it.
pub struct Buffer<T: Element> {
    values: Vec<T>,
}

impl<T: Element> Buffer<T> {
    pub fn as_slice(&self) -> &[T] {
        &self.values
    }
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }
}

/// Allocates only payload buffers charged against a consumed request plan.
///
/// Charges are conservative and never returned, even if a buffer is dropped.
/// Actual vector capacity is charged; an allocator that gives more capacity
/// than remains in the plan causes refusal. Allocator overhead, transient
/// allocation, thread stacks and other allocations are not accounted for.
/// Multiple allocators have no shared limit, and this is not an RSS limit.
pub struct RequestBuffers {
    remaining_bytes: u64,
}

impl RequestBuffers {
    pub fn new(plan: Plan) -> Self {
        Self {
            remaining_bytes: plan.request_bytes,
        }
    }

    pub fn remaining_bytes(&self) -> u64 {
        self.remaining_bytes
    }

    /// Reserve a disjoint portion of this plan for another allocation scope.
    /// The reservation is never returned, even if the child is dropped.
    pub fn split(&mut self, bytes: u64) -> Result<Self, Error> {
        if bytes > self.remaining_bytes {
            return Err(Error::ExceedsBudget);
        }
        self.remaining_bytes -= bytes;
        Ok(Self {
            remaining_bytes: bytes,
        })
    }

    pub fn allocate<T: Element>(&mut self, count: u64) -> Result<Buffer<T>, Error> {
        let size = u64::try_from(std::mem::size_of::<T>()).map_err(|_| Error::Overflow)?;
        let requested = count.checked_mul(size).ok_or(Error::Overflow)?;
        if requested > self.remaining_bytes {
            return Err(Error::ExceedsBudget);
        }
        let count = usize::try_from(count).map_err(|_| Error::Overflow)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        let capacity = u64::try_from(values.capacity()).map_err(|_| Error::Overflow)?;
        let charged = capacity.checked_mul(size).ok_or(Error::Overflow)?;
        if charged > self.remaining_bytes {
            return Err(Error::ExceedsBudget);
        }
        values.resize(count, T::default());
        self.remaining_bytes -= charged;
        Ok(Buffer { values })
    }
}

#[cfg(test)]
mod tests {
    use super::{checked_plan, Allocation, Error, Plan, RequestBuffers};

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

    #[test]
    fn fixed_buffers_cannot_spend_more_than_the_precomputed_payload_plan() {
        let request = [Allocation {
            count: 1024 * 1024,
            element_bytes: 1,
        }];
        let plan = checked_plan(1, 0, &request).unwrap();
        assert_eq!(plan.resident_bytes(), 0);
        assert_eq!(plan.request_bytes(), 1024 * 1024);
        assert_eq!(plan.total_bytes(), 1024 * 1024);
        let mut allocator = RequestBuffers::new(plan);
        let mut floats = allocator.allocate::<f32>(4).unwrap();
        floats.as_mut_slice()[0] = 2.5;
        assert_eq!(floats.as_slice(), &[2.5, 0.0, 0.0, 0.0]);
        let before = allocator.remaining_bytes();
        assert!(matches!(
            allocator.allocate::<u8>(before + 1),
            Err(Error::ExceedsBudget)
        ));
        assert!(matches!(
            allocator.allocate::<i64>(u64::MAX),
            Err(Error::Overflow)
        ));
        assert_eq!(allocator.remaining_bytes(), before);
        let bytes = allocator.allocate::<u8>(8).unwrap();
        assert_eq!(bytes.as_slice().len(), 8);
        assert_eq!(
            allocator.remaining_bytes(),
            before - bytes.values.capacity() as u64
        );
        let after = allocator.remaining_bytes();
        drop(floats);
        drop(bytes);
        assert_eq!(allocator.remaining_bytes(), after);
        assert!(matches!(
            allocator.allocate::<u8>(after + 1),
            Err(Error::ExceedsBudget)
        ));
    }

    #[test]
    fn child_scopes_cannot_oversubscribe_or_refund_the_parent() {
        let plan = checked_plan(
            1,
            0,
            &[Allocation {
                count: 1_024,
                element_bytes: 1,
            }],
        )
        .unwrap();
        let mut parent = RequestBuffers::new(plan);
        {
            let mut frame = parent.split(256).unwrap();
            let _arena = parent.split(512).unwrap();
            assert_eq!(parent.remaining_bytes(), 256);
            assert!(matches!(parent.split(257), Err(Error::ExceedsBudget)));
            assert_eq!(parent.remaining_bytes(), 256);
            let buffer = frame.allocate::<u8>(32).unwrap();
            assert_eq!(buffer.as_slice().len(), 32);
        }
        assert_eq!(parent.remaining_bytes(), 256);
    }
}
