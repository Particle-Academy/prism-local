//! Experimental fixed-storage JSON tree. Not yet used by the sidecar.
//!
//! Nodes and decoded strings are allocated through one precomputed request
//! plan. Caller-owned input and parser stack are not included in that plan.
//! Object members are sorted in-place to reject duplicates without a heap map.
//! Callers must enforce a deadline before using this on untrusted input.

use crate::{
    frame::HARD_MAX_FRAME,
    json::{decode_string_into, StringDecodeError, MAX_DEPTH, MAX_NODES, MAX_STRING},
    memory_plan::{Buffer, RequestBuffers},
};

const FIELDS: usize = 8;
const NONE: u64 = u64::MAX;
const NULL: u64 = 0;
const FALSE: u64 = 1;
const TRUE: u64 = 2;
const NUMBER: u64 = 3;
const STRING: u64 = 4;
const ARRAY: u64 = 5;
const OBJECT: u64 = 6;
const KIND: usize = 0;
const FIRST: usize = 1;
const LAST: usize = 2;
const NEXT: usize = 3;
const KEY_START: usize = 4;
const KEY_LEN: usize = 5;
const DATA_START: usize = 6;
const DATA_LEN: usize = 7;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Resource,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    Null,
    Bool(bool),
    Number,
    String,
    Array,
    Object,
}

/// Upper bounds before parsing: u64 slot count and decoded-text byte count.
/// Charge each slot at eight bytes and each text byte at one byte.
pub fn allocation_sizes(input_len: usize) -> Option<(u64, u64)> {
    if input_len > HARD_MAX_FRAME {
        return None;
    }
    let nodes = input_len.min(MAX_NODES);
    Some((
        u64::try_from(nodes.checked_mul(FIELDS)?).ok()?,
        u64::try_from(input_len).ok()?,
    ))
}

/// A parsed tree borrowing the original input and owning fixed arena buffers.
pub struct Arena<'a> {
    input: &'a [u8],
    slots: Buffer<u64>,
    text: Buffer<u8>,
    nodes: usize,
    text_used: usize,
}

/// Parse into buffers charged to the caller's precomputed request plan.
/// This does not account for the input buffer, recursion stack or other work.
pub fn parse<'a>(input: &'a [u8], buffers: &mut RequestBuffers) -> Result<Arena<'a>, Error> {
    let (slots, text_bytes) = allocation_sizes(input.len()).ok_or(Error::Invalid)?;
    let slots = buffers
        .allocate::<u64>(slots)
        .map_err(|_| Error::Resource)?;
    let text = buffers
        .allocate::<u8>(text_bytes)
        .map_err(|_| Error::Resource)?;
    let mut parser = Parser {
        arena: Arena {
            input,
            slots,
            text,
            nodes: 0,
            text_used: 0,
        },
        pos: 0,
    };
    let root = parser.value(0)?;
    parser.ws();
    if root != 0 || parser.pos != input.len() {
        return Err(Error::Invalid);
    }
    Ok(parser.arena)
}

impl<'a> Arena<'a> {
    pub fn root(&self) -> usize {
        0
    }
    pub fn kind(&self, index: usize) -> Option<Kind> {
        match self.field(index, KIND)? {
            NULL => Some(Kind::Null),
            FALSE => Some(Kind::Bool(false)),
            TRUE => Some(Kind::Bool(true)),
            NUMBER => Some(Kind::Number),
            STRING => Some(Kind::String),
            ARRAY => Some(Kind::Array),
            OBJECT => Some(Kind::Object),
            _ => None,
        }
    }
    pub fn first_child(&self, index: usize) -> Option<usize> {
        self.link(index, FIRST)
    }
    pub fn next_sibling(&self, index: usize) -> Option<usize> {
        self.link(index, NEXT)
    }
    pub fn key(&self, index: usize) -> Option<&str> {
        let start = self.field(index, KEY_START)?;
        if start == NONE {
            return None;
        }
        self.text_range(start, self.field(index, KEY_LEN)?)
    }
    pub fn string(&self, index: usize) -> Option<&str> {
        if self.kind(index)? != Kind::String {
            return None;
        }
        self.text_range(self.field(index, DATA_START)?, self.field(index, DATA_LEN)?)
    }
    pub fn number(&self, index: usize) -> Option<&str> {
        if self.kind(index)? != Kind::Number {
            return None;
        }
        let start = usize::try_from(self.field(index, DATA_START)?).ok()?;
        let len = usize::try_from(self.field(index, DATA_LEN)?).ok()?;
        std::str::from_utf8(self.input.get(start..start.checked_add(len)?)?).ok()
    }
    pub fn get(&self, index: usize, key: &str) -> Option<usize> {
        if self.kind(index)? != Kind::Object {
            return None;
        }
        let mut child = self.first_child(index);
        while let Some(current) = child {
            if self.key(current) == Some(key) {
                return Some(current);
            }
            child = self.next_sibling(current);
        }
        None
    }
    fn field(&self, index: usize, field: usize) -> Option<u64> {
        if index >= self.nodes || field >= FIELDS {
            return None;
        }
        self.slots
            .as_slice()
            .get(index.checked_mul(FIELDS)?.checked_add(field)?)
            .copied()
    }
    fn link(&self, index: usize, field: usize) -> Option<usize> {
        let value = self.field(index, field)?;
        if value == NONE {
            return None;
        }
        let value = usize::try_from(value).ok()?;
        (value < self.nodes).then_some(value)
    }
    fn text_range(&self, start: u64, length: u64) -> Option<&str> {
        let start = usize::try_from(start).ok()?;
        let length = usize::try_from(length).ok()?;
        let end = start.checked_add(length)?;
        if end > self.text_used {
            return None;
        }
        std::str::from_utf8(self.text.as_slice().get(start..end)?).ok()
    }
}

struct Parser<'a> {
    arena: Arena<'a>,
    pos: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(
            self.arena.input.get(self.pos),
            Some(b' ' | b'\n' | b'\r' | b'\t')
        ) {
            self.pos += 1;
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        if self.arena.input.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn node(&mut self, kind: u64) -> Result<usize, Error> {
        let index = self.arena.nodes;
        if index >= MAX_NODES {
            return Err(Error::Invalid);
        }
        let base = index.checked_mul(FIELDS).ok_or(Error::Resource)?;
        let end = base.checked_add(FIELDS).ok_or(Error::Resource)?;
        let fields = self
            .arena
            .slots
            .as_mut_slice()
            .get_mut(base..end)
            .ok_or(Error::Resource)?;
        fields[KIND] = kind;
        fields[FIRST] = NONE;
        fields[LAST] = NONE;
        fields[NEXT] = NONE;
        fields[KEY_START] = NONE;
        self.arena.nodes += 1;
        Ok(index)
    }
    fn set(&mut self, index: usize, field: usize, value: u64) -> Result<(), Error> {
        let offset = index
            .checked_mul(FIELDS)
            .and_then(|v| v.checked_add(field))
            .ok_or(Error::Resource)?;
        *self
            .arena
            .slots
            .as_mut_slice()
            .get_mut(offset)
            .ok_or(Error::Resource)? = value;
        Ok(())
    }
    fn add_child(&mut self, parent: usize, child: usize) -> Result<(), Error> {
        let last = self.arena.field(parent, LAST).ok_or(Error::Invalid)?;
        let child_u64 = u64::try_from(child).map_err(|_| Error::Resource)?;
        if last == NONE {
            self.set(parent, FIRST, child_u64)?;
        } else {
            self.set(
                usize::try_from(last).map_err(|_| Error::Invalid)?,
                NEXT,
                child_u64,
            )?;
        }
        self.set(parent, LAST, child_u64)
    }
    fn literal(&mut self, literal: &[u8], kind: u64) -> Result<usize, Error> {
        let end = self.pos.checked_add(literal.len()).ok_or(Error::Invalid)?;
        if self.arena.input.get(self.pos..end) != Some(literal) {
            return Err(Error::Invalid);
        }
        self.pos = end;
        self.node(kind)
    }
    fn value(&mut self, depth: usize) -> Result<usize, Error> {
        self.ws();
        if depth > MAX_DEPTH || self.arena.nodes >= MAX_NODES {
            return Err(Error::Invalid);
        }
        match self
            .arena
            .input
            .get(self.pos)
            .copied()
            .ok_or(Error::Invalid)?
        {
            b'n' => self.literal(b"null", NULL),
            b'f' => self.literal(b"false", FALSE),
            b't' => self.literal(b"true", TRUE),
            b'"' => {
                let node = self.node(STRING)?;
                let (start, len) = self.string()?;
                self.set(node, DATA_START, start)?;
                self.set(node, DATA_LEN, len)?;
                Ok(node)
            }
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(Error::Invalid),
        }
    }
    fn string(&mut self) -> Result<(u64, u64), Error> {
        let start = self.pos;
        if !self.take(b'"') {
            return Err(Error::Invalid);
        }
        loop {
            let byte = *self.arena.input.get(self.pos).ok_or(Error::Invalid)?;
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    self.arena.input.get(self.pos).ok_or(Error::Invalid)?;
                    self.pos += 1;
                }
                _ => {}
            }
        }
        let token = self
            .arena
            .input
            .get(start..self.pos)
            .ok_or(Error::Invalid)?;
        let text_start = self.arena.text_used;
        let target = self
            .arena
            .text
            .as_mut_slice()
            .get_mut(text_start..)
            .ok_or(Error::Resource)?;
        let len = decode_string_into(token, target).map_err(|error| match error {
            StringDecodeError::Invalid => Error::Invalid,
            StringDecodeError::OutputTooSmall => Error::Resource,
        })?;
        self.arena.text_used = text_start.checked_add(len).ok_or(Error::Resource)?;
        Ok((
            u64::try_from(text_start).map_err(|_| Error::Resource)?,
            u64::try_from(len).map_err(|_| Error::Resource)?,
        ))
    }
    fn array(&mut self, depth: usize) -> Result<usize, Error> {
        self.pos += 1;
        let parent = self.node(ARRAY)?;
        self.ws();
        if self.take(b']') {
            return Ok(parent);
        }
        loop {
            let child = self.value(depth + 1)?;
            self.add_child(parent, child)?;
            self.ws();
            if self.take(b']') {
                return Ok(parent);
            }
            if !self.take(b',') {
                return Err(Error::Invalid);
            }
        }
    }
    fn object(&mut self, depth: usize) -> Result<usize, Error> {
        self.pos += 1;
        let parent = self.node(OBJECT)?;
        self.ws();
        if self.take(b'}') {
            return Ok(parent);
        }
        let mut count = 0_usize;
        loop {
            self.ws();
            let (start, len) = self.string()?;
            self.ws();
            if !self.take(b':') {
                return Err(Error::Invalid);
            }
            let child = self.value(depth + 1)?;
            self.set(child, KEY_START, start)?;
            self.set(child, KEY_LEN, len)?;
            self.add_child(parent, child)?;
            count = count.checked_add(1).ok_or(Error::Resource)?;
            self.ws();
            if self.take(b'}') {
                self.sort_object(parent, count)?;
                return Ok(parent);
            }
            if !self.take(b',') {
                return Err(Error::Invalid);
            }
        }
    }
    fn sort_object(&mut self, parent: usize, count: usize) -> Result<(), Error> {
        if count <= 1 {
            return Ok(());
        }
        let head = self.arena.first_child(parent).ok_or(Error::Invalid)?;
        let (head, tail) = self.sort_members(head, count)?;
        self.set(
            parent,
            FIRST,
            u64::try_from(head).map_err(|_| Error::Resource)?,
        )?;
        self.set(
            parent,
            LAST,
            u64::try_from(tail).map_err(|_| Error::Resource)?,
        )
    }
    fn sort_members(&mut self, head: usize, count: usize) -> Result<(usize, usize), Error> {
        if count == 1 {
            return Ok((head, head));
        }
        let left_count = count / 2;
        let mut split = head;
        for _ in 1..left_count {
            split = self.arena.next_sibling(split).ok_or(Error::Invalid)?;
        }
        let right_head = self.arena.next_sibling(split).ok_or(Error::Invalid)?;
        self.set(split, NEXT, NONE)?;
        let (left_head, left_tail) = self.sort_members(head, left_count)?;
        let (right_head, right_tail) = self.sort_members(right_head, count - left_count)?;
        self.merge_members(left_head, left_tail, right_head, right_tail)
    }
    fn merge_members(
        &mut self,
        left_head: usize,
        left_tail: usize,
        right_head: usize,
        right_tail: usize,
    ) -> Result<(usize, usize), Error> {
        let mut left = Some(left_head);
        let mut right = Some(right_head);
        let mut head = None;
        let mut tail = None;
        while let (Some(a), Some(b)) = (left, right) {
            let order = self
                .arena
                .key(a)
                .ok_or(Error::Invalid)?
                .cmp(self.arena.key(b).ok_or(Error::Invalid)?);
            let selected = match order {
                std::cmp::Ordering::Less => {
                    left = self.arena.next_sibling(a);
                    a
                }
                std::cmp::Ordering::Greater => {
                    right = self.arena.next_sibling(b);
                    b
                }
                std::cmp::Ordering::Equal => return Err(Error::Invalid),
            };
            if let Some(previous) = tail {
                self.set(
                    previous,
                    NEXT,
                    u64::try_from(selected).map_err(|_| Error::Resource)?,
                )?;
            } else {
                head = Some(selected);
            }
            tail = Some(selected);
        }
        let (remaining, final_tail) = match (left, right) {
            (Some(value), None) => (value, left_tail),
            (None, Some(value)) => (value, right_tail),
            _ => return Err(Error::Invalid),
        };
        self.set(
            tail.ok_or(Error::Invalid)?,
            NEXT,
            u64::try_from(remaining).map_err(|_| Error::Resource)?,
        )?;
        Ok((head.ok_or(Error::Invalid)?, final_tail))
    }
    fn number(&mut self) -> Result<usize, Error> {
        let start = self.pos;
        self.take(b'-');
        if !self.take(b'0') {
            if !matches!(self.arena.input.get(self.pos), Some(b'1'..=b'9')) {
                return Err(Error::Invalid);
            }
            self.pos += 1;
            while matches!(self.arena.input.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if self.take(b'.') {
            self.digits()?;
        }
        if matches!(self.arena.input.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if !self.take(b'+') {
                self.take(b'-');
            }
            self.digits()?;
        }
        let len = self.pos.checked_sub(start).ok_or(Error::Invalid)?;
        if len > MAX_STRING {
            return Err(Error::Invalid);
        }
        let node = self.node(NUMBER)?;
        self.set(
            node,
            DATA_START,
            u64::try_from(start).map_err(|_| Error::Resource)?,
        )?;
        self.set(
            node,
            DATA_LEN,
            u64::try_from(len).map_err(|_| Error::Resource)?,
        )?;
        Ok(node)
    }
    fn digits(&mut self) -> Result<(), Error> {
        let start = self.pos;
        while matches!(self.arena.input.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == start {
            Err(Error::Invalid)
        } else {
            Ok(())
        }
    }
}
