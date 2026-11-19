use prism_local_core::{
    json::{self, Value},
    json_arena::{self, Arena, Error, Kind},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};

fn compare(arena: &Arena<'_>, index: usize, expected: &Value) {
    match expected {
        Value::Null => assert_eq!(arena.kind(index), Some(Kind::Null)),
        Value::Bool(value) => assert_eq!(arena.kind(index), Some(Kind::Bool(*value))),
        Value::Number(value) => assert_eq!(arena.number(index), Some(value.as_str())),
        Value::String(value) => assert_eq!(arena.string(index), Some(value.as_str())),
        Value::Array(values) => {
            assert_eq!(arena.kind(index), Some(Kind::Array));
            let mut child = arena.first_child(index);
            for value in values {
                let current = child.expect("array element");
                compare(arena, current, value);
                child = arena.next_sibling(current);
            }
            assert!(child.is_none());
        }
        Value::Object(values) => {
            assert_eq!(arena.kind(index), Some(Kind::Object));
            let mut child = arena.first_child(index);
            for (key, value) in values {
                let current = child.expect("object member");
                assert_eq!(arena.key(current), Some(key.as_str()));
                assert_eq!(arena.get(index, key), Some(current));
                compare(arena, current, value);
                child = arena.next_sibling(current);
            }
            assert!(child.is_none());
        }
    }
}

fn check(input: &[u8]) {
    let expected = json::parse(input);
    let (slots, bytes) = json_arena::allocation_sizes(input.len()).unwrap();
    let plan = checked_plan(
        4,
        0,
        &[
            Allocation {
                count: slots,
                element_bytes: 8,
            },
            Allocation {
                count: bytes,
                element_bytes: 1,
            },
        ],
    )
    .unwrap();
    let mut buffers = RequestBuffers::new(plan);
    let result = json_arena::parse(input, &mut buffers);
    match (expected, result) {
        (Ok(expected), Ok(arena)) => compare(&arena, arena.root(), &expected),
        (Err(_), Err(Error::Invalid)) => {}
        _ => panic!("arena and strict parser disagreed"),
    }
}

#[test]
fn matches_strict_json_on_values_duplicates_and_limits() {
    for input in [
        &b"null"[..],
        &b"[true,false,null,-12.5e+3]"[..],
        &br#"{"jsonrpc":"2.0","id":"x\"y","params":{"threads":2}}"#[..],
        &br#"{"a":1,"\u0061":2}"#[..],
        &br#"["\uD83D\uDE00","\u0000",{}]"#[..],
        &br#"[1,]"#[..],
        &br#"{"x":1,}"#[..],
        &b"01"[..],
        &b"\"\xff\""[..],
        &br#"{"":0,"\\":1,"\u0000":2,"nested":{"x":[1,2,3]}}"#[..],
    ] {
        check(input);
    }
    check(format!("{}0{}", "[".repeat(32), "]".repeat(32)).as_bytes());
    check(format!("{}0{}", "[".repeat(33), "]".repeat(33)).as_bytes());
    check(format!("{}0{}", "[".repeat(34), "]".repeat(34)).as_bytes());
    check(format!("\"{}\"", "a".repeat(65_537)).as_bytes());
    check("1".repeat(65_537).as_bytes());
    check(format!("[{}0]", "0,".repeat(16_382)).as_bytes());
    check(format!("[{}0]", "0,".repeat(16_383)).as_bytes());
    let members = (0..501)
        .rev()
        .map(|index| format!("\"key{index:04}\":{index}"))
        .collect::<Vec<_>>()
        .join(",");
    check(format!("{{{members}}}").as_bytes());
    check(format!("{{{members},\"key0500\":1}}").as_bytes());
}

#[test]
fn matches_strict_parser_on_bounded_deterministic_mutations() {
    let seeds: &[&[u8]] = &[
        br#"{"a":1,"b":[true,"\ud800\udc00",null]}"#,
        br#"["\"\\\/\n\u00e9",{},-12.5e+4]"#,
        br#"{"a":1,"\u0061":2}"#,
    ];
    let mut state = 0x243f_6a88_u32;
    for iteration in 0..4_096 {
        let mut bytes = seeds[iteration % seeds.len()].to_vec();
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

#[test]
fn insufficient_plan_is_refused_without_a_tree() {
    let plan = checked_plan(1, 0, &[]).unwrap();
    let mut buffers = RequestBuffers::new(plan);
    assert!(matches!(
        json_arena::parse(b"{}", &mut buffers),
        Err(Error::Resource)
    ));
    let (slots, _) = json_arena::allocation_sizes(2).unwrap();
    let plan = checked_plan(
        1,
        0,
        &[Allocation {
            count: slots,
            element_bytes: 8,
        }],
    )
    .unwrap();
    let mut buffers = RequestBuffers::new(plan);
    assert!(matches!(
        json_arena::parse(b"{}", &mut buffers),
        Err(Error::Resource)
    ));
    assert!(buffers.remaining_bytes() <= slots * 8);
}
