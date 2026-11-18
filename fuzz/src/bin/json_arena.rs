use prism_local_core::{
    json::{self, Value},
    json_arena::{self, Arena, Error, Kind},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
};

fn same(arena: &Arena<'_>, index: usize, expected: &Value) -> bool {
    match expected {
        Value::Null => arena.kind(index) == Some(Kind::Null),
        Value::Bool(value) => arena.kind(index) == Some(Kind::Bool(*value)),
        Value::Number(value) => arena.number(index) == Some(value.as_str()),
        Value::String(value) => arena.string(index) == Some(value.as_str()),
        Value::Array(values) => {
            if arena.kind(index) != Some(Kind::Array) {
                return false;
            }
            let mut child = arena.first_child(index);
            for value in values {
                let Some(current) = child else { return false };
                if !same(arena, current, value) {
                    return false;
                }
                child = arena.next_sibling(current);
            }
            child.is_none()
        }
        Value::Object(values) => {
            if arena.kind(index) != Some(Kind::Object) {
                return false;
            }
            let mut child = arena.first_child(index);
            for (key, value) in values {
                let Some(current) = child else {
                    return false;
                };
                if arena.key(current) != Some(key.as_str()) || !same(arena, current, value) {
                    return false;
                }
                child = arena.next_sibling(current);
            }
            child.is_none()
        }
    }
}

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        let plan = checked_plan(
            4,
            0,
            &[Allocation {
                count: 4 * 1024 * 1024,
                element_bytes: 1,
            }],
        )
        .unwrap();
        let mut buffers = RequestBuffers::new(plan);
        match (json::parse(&bytes), json_arena::parse(&bytes, &mut buffers)) {
            (Ok(expected), Ok(arena)) => assert!(same(&arena, arena.root(), &expected)),
            (Err(_), Err(Error::Invalid)) | (_, Err(Error::Resource)) => {}
            _ => panic!("JSON parsers disagree"),
        }
    }
    Ok(())
}
