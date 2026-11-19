//! Std-only, in-process, coverage-guided fuzzer for prism-local parsers.
//! Dev tooling: built with plain `rustc` by `fuzz/engine/build.sh`, never a
//! workspace member and never shipped. Coverage comes from LLVM
//! SanitizerCoverage (trace-pc-guard + trace-cmp) compiled into the fuzz
//! library and the parser crates; the runtime hooks are below.
//!
//! usage: engine <target> <seed-dir> <work-dir> <seconds> [rng-seed]
//!        engine <target> <corpus-dir> <file-filter> lines   (Windows; needs pc-table)
//! Exit status: 1 when any input panicked or ran longer than one second.
//! For the `canary` target the meaning is inverted: 0 only if it was found.
#![allow(static_mut_refs)]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- runtime

const MAX_GUARDS: usize = 1 << 20;
static NEXT_GUARD: AtomicUsize = AtomicUsize::new(1);
static mut COUNTERS: [u8; MAX_GUARDS] = [0; MAX_GUARDS];
static RECORD_CMP: AtomicBool = AtomicBool::new(false);
const CMP_SLOTS: usize = 4096;
static mut CMP_TABLE: [(u64, u64, u8); CMP_SLOTS] = [(0, 0, 0); CMP_SLOTS];
static CMP_NEXT: AtomicUsize = AtomicUsize::new(0);

// COFF has no linker-defined section bounds, so the runtime supplies them
// as 8-byte markers exactly like compiler-rt: LLVM passes marker+8 as the
// table start on COFF. ELF linkers define __start_/__stop_ themselves.
#[cfg(windows)]
#[link_section = ".SCOV$GA"]
#[no_mangle]
pub static mut __start___sancov_guards: u64 = 0;
#[cfg(windows)]
#[link_section = ".SCOV$GZ"]
#[no_mangle]
pub static mut __stop___sancov_guards: u64 = 0;
#[cfg(windows)]
#[link_section = ".SCOVP$A"]
#[no_mangle]
pub static __start___sancov_pcs: u64 = 0;
#[cfg(windows)]
#[link_section = ".SCOVP$Z"]
#[no_mangle]
pub static __stop___sancov_pcs: u64 = 0;

static mut PCS: (usize, usize) = (0, 0);
static mut GUARD_BASE: usize = 0;

/// PC table (with -sanitizer-coverage-pc-table): one (pc, flags) pair per guard.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_pcs_init(beg: *const usize, end: *const usize) {
    unsafe {
        if PCS.0 == 0 {
            PCS = (beg as usize, end as usize);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_pc_guard_init(mut start: *mut u32, end: *mut u32) {
    if start == end || unsafe { *start } != 0 {
        return;
    }
    unsafe {
        if GUARD_BASE == 0 {
            GUARD_BASE = NEXT_GUARD.load(Ordering::Relaxed);
        }
    }
    while start < end {
        let id = NEXT_GUARD.fetch_add(1, Ordering::Relaxed);
        unsafe { *start = if id < MAX_GUARDS { id as u32 } else { 0 } };
        start = unsafe { start.add(1) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_pc_guard(guard: *mut u32) {
    let id = unsafe { *guard } as usize;
    if id != 0 && id < MAX_GUARDS {
        unsafe {
            let c = &mut COUNTERS[id];
            *c = c.saturating_add(1);
        }
    }
}

#[inline(always)]
fn cmp(a: u64, b: u64, width: u8) {
    if a == b || !RECORD_CMP.load(Ordering::Relaxed) {
        return;
    }
    let slot = CMP_NEXT.fetch_add(1, Ordering::Relaxed) % CMP_SLOTS;
    unsafe { CMP_TABLE[slot] = (a, b, width) };
}

#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_cmp1(a: u8, b: u8) { cmp(a as u64, b as u64, 1) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_cmp2(a: u16, b: u16) { cmp(a as u64, b as u64, 2) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_cmp4(a: u32, b: u32) { cmp(a as u64, b as u64, 4) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_cmp8(a: u64, b: u64) { cmp(a, b, 8) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_const_cmp1(a: u8, b: u8) { cmp(a as u64, b as u64, 1) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_const_cmp2(a: u16, b: u16) { cmp(a as u64, b as u64, 2) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_const_cmp4(a: u32, b: u32) { cmp(a as u64, b as u64, 4) }
#[no_mangle]
pub extern "C" fn __sanitizer_cov_trace_const_cmp8(a: u64, b: u64) { cmp(a, b, 8) }
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_switch(val: u64, cases: *const u64) {
    if !RECORD_CMP.load(Ordering::Relaxed) {
        return;
    }
    let n = unsafe { *cases } as usize;
    let bits = unsafe { *cases.add(1) };
    let width = (bits / 8).clamp(1, 8) as u8;
    for i in 0..n.min(16) {
        cmp(val, unsafe { *cases.add(2 + i) }, width);
    }
}

// ---------------------------------------------------------------- targets

use prism_local_fuzz::targets;

// ---------------------------------------------------------------- engine

const MAX_LEN: usize = 1 << 20;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn bucket(c: u8) -> u8 {
    match c {
        0 => 0, 1 => 1, 2 => 2, 3 => 3, 4..=7 => 4, 8..=15 => 5, 16..=31 => 6, 32..=127 => 7, _ => 8,
    }
}

thread_local! { static PANIC_MSG: RefCell<String> = const { RefCell::new(String::new()) }; }

enum Outcome { Ok, Panic(String) }

struct Exec { features: Vec<u32>, outcome: Outcome, elapsed: Duration }

fn execute(target: fn(&[u8]), input: &[u8], guards: usize) -> Exec {
    unsafe { COUNTERS[..guards].fill(0) };
    let start = Instant::now();
    let result = panic::catch_unwind(AssertUnwindSafe(|| target(input)));
    let elapsed = start.elapsed();
    let mut features = Vec::new();
    for (id, &c) in unsafe { COUNTERS[..guards].iter().enumerate() } {
        if c != 0 {
            features.push((id as u32) * 16 + bucket(c) as u32);
        }
    }
    let outcome = match result {
        Ok(()) => Outcome::Ok,
        Err(_) => Outcome::Panic(PANIC_MSG.with(|m| m.borrow().clone())),
    };
    Exec { features, outcome, elapsed }
}

const INTERESTING: [u64; 20] = [
    0, 1, 2, 7, 8, 16, 32, 63, 64, 0x7f, 0x80, 0xff, 0x100, 0x7fff, 0x8000, 0xffff,
    0x7fff_ffff, 0x8000_0000, 0xffff_ffff, u64::MAX,
];

fn write_le(buf: &mut Vec<u8>, at: usize, v: u64, w: usize) {
    for i in 0..w {
        if at + i < buf.len() {
            buf[at + i] = (v >> (8 * i)) as u8;
        }
    }
}

fn mutate(rng: &mut Rng, base: &[u8], corpus: &[Vec<u8>], dict: &[(u64, u64, u8)]) -> Vec<u8> {
    let mut b = base.to_vec();
    let rounds = 1 + rng.below(8);
    for _ in 0..rounds {
        if b.is_empty() {
            b.push(rng.next() as u8);
            continue;
        }
        let len = b.len();
        match rng.below(13) {
            0 => { let i = rng.below(len); b[i] ^= 1 << rng.below(8); }
            1 => { let i = rng.below(len); b[i] = rng.next() as u8; }
            2 | 3 => {
                let w = [1, 2, 4, 8][rng.below(4)];
                let at = rng.below(len);
                let mut v = INTERESTING[rng.below(INTERESTING.len())];
                if rng.below(2) == 0 { v = v.wrapping_neg() }
                write_le(&mut b, at, v, w);
            }
            4 => {
                let w = [1, 2, 4, 8][rng.below(4)];
                let at = rng.below(len);
                let mut cur = 0u64;
                for i in 0..w { if at + i < len { cur |= (b[at + i] as u64) << (8 * i) } }
                let delta = 1 + rng.below(35) as u64;
                let v = if rng.below(2) == 0 { cur.wrapping_add(delta) } else { cur.wrapping_sub(delta) };
                write_le(&mut b, at, v, w);
            }
            5 => { let at = rng.below(len); let n = 1 + rng.below((len - at).min(64)); b.drain(at..at + n); }
            6 if len < MAX_LEN => {
                let at = rng.below(len + 1);
                let n = 1 + rng.below(64);
                let fill: Vec<u8> = if rng.below(2) == 0 {
                    (0..n).map(|_| rng.next() as u8).collect()
                } else { vec![b[rng.below(len)]; n] };
                b.splice(at..at, fill);
            }
            7 => {
                let from = rng.below(len);
                let n = 1 + rng.below((len - from).min(256));
                let chunk = b[from..from + n].to_vec();
                let at = rng.below(len + 1);
                if rng.below(2) == 0 && b.len() + n <= MAX_LEN { b.splice(at..at, chunk); }
                else { for (i, c) in chunk.into_iter().enumerate() { if at + i < b.len() { b[at + i] = c } } }
            }
            8 if !corpus.is_empty() => {
                let other = &corpus[rng.below(corpus.len())];
                if !other.is_empty() {
                    let cut = rng.below(b.len());
                    let from = rng.below(other.len());
                    b.truncate(cut);
                    b.extend_from_slice(&other[from..other.len().min(from + MAX_LEN - cut)]);
                }
            }
            9 | 10 if !dict.is_empty() => {
                let (a, c, w) = dict[rng.below(dict.len())];
                let w = w as usize;
                // Replace an occurrence of one operand with the other (CMP-guided), else write at random.
                let (find, put) = if rng.below(2) == 0 { (a, c) } else { (c, a) };
                let needle: Vec<u8> = (0..w).map(|i| (find >> (8 * i)) as u8).collect();
                let pos = if len >= w { b.windows(w).position(|x| x == &needle[..]) } else { None };
                let at = pos.unwrap_or_else(|| rng.below(len));
                write_le(&mut b, at, put, w);
            }
            11 => { let n = rng.below(len); b.truncate(n.max(1)); }
            _ => {
                let at = rng.below(len);
                let n = 1 + rng.below((len - at).min(16));
                let v = rng.next() as u8;
                for x in &mut b[at..at + n] { *x = v }
            }
        }
    }
    b.truncate(MAX_LEN);
    b
}

fn hex(bytes: &[u8]) -> String {
    let d = prism_local_core::sha256::digest(bytes).unwrap_or([0; 32]);
    d[..8].iter().map(|x| format!("{x:02x}")).collect()
}


// ---------------------------------------------------------------- line report

#[cfg(windows)]
mod sym {
    use std::ffi::{c_void, CStr};
    #[repr(C)]
    struct Line64 { size: u32, key: *mut c_void, line: u32, file: *const i8, address: u64 }
    #[link(name = "dbghelp")]
    unsafe extern "system" {
        fn SymSetOptions(options: u32) -> u32;
        fn SymInitialize(process: *mut c_void, path: *const i8, invade: i32) -> i32;
        fn SymGetLineFromAddr64(process: *mut c_void, addr: u64, disp: *mut u32, line: *mut Line64) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetCurrentProcess() -> *mut c_void; }
    /// Search for the PDB beside the executable, not in the current directory.
    pub fn init() {
        let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_string_lossy().into_owned())).unwrap_or_default();
        let path = std::ffi::CString::new(dir).unwrap_or_default();
        unsafe { SymSetOptions(0x10 | 0x2); SymInitialize(GetCurrentProcess(), path.as_ptr(), 1); }
    }
    pub fn line(pc: usize) -> Option<(String, u32)> {
        let mut l = Line64 { size: std::mem::size_of::<Line64>() as u32, key: std::ptr::null_mut(), line: 0, file: std::ptr::null(), address: 0 };
        let mut d = 0u32;
        unsafe {
            if SymGetLineFromAddr64(GetCurrentProcess(), pc as u64, &mut d, &mut l) == 0 || l.file.is_null() { return None; }
            Some((CStr::from_ptr(l.file).to_string_lossy().into_owned(), l.line))
        }
    }
}

/// Replays a corpus and prints, per source line of files matching `filter`,
/// whether every coverage point on it was reached ("hit"), some ("part") or none ("MISS").
#[cfg(windows)]
fn line_report(target: fn(&[u8]), dir: &Path, filter: &str) {
    let guards = NEXT_GUARD.load(Ordering::Relaxed);
    let mut union = vec![false; guards];
    let mut files = 0;
    for e in fs::read_dir(dir).expect("corpus dir").flatten() {
        let b = fs::read(e.path()).unwrap();
        unsafe { COUNTERS[..guards].fill(0) };
        let _ = panic::catch_unwind(AssertUnwindSafe(|| target(&b)));
        for (i, &c) in unsafe { COUNTERS[..guards].iter().enumerate() } { if c != 0 { union[i] = true } }
        files += 1;
    }
    let (beg, end) = unsafe { PCS };
    assert!(beg != 0, "no PC table: build with -sanitizer-coverage-pc-table");
    let entries = (end - beg) / 16;
    let base = unsafe { GUARD_BASE };
    sym::init();
    let (mut resolved, mut unresolved, mut matched) = (0, 0, 0);
    let mut lines: std::collections::BTreeMap<(String, u32), (u32, u32)> = Default::default();
    for id in base..guards {
        let k = id - base;
        if k >= entries { break; }
        let pc = unsafe { *((beg + k * 16) as *const usize) };
        if pc == 0 { continue; }
        let Some((f, l)) = sym::line(pc) else { unresolved += 1; continue };
        resolved += 1;
        {
            if f.replace(std::path::MAIN_SEPARATOR, "/").contains(filter) {
                matched += 1;
                let e = lines.entry((f, l)).or_insert((0, 0));
                e.1 += 1;
                if union[id] { e.0 += 1 }
            }
        }
    }
    let (mut hit, mut part, mut miss) = (0, 0, 0);
    for ((f, l), (h, t)) in &lines {
        let tag = if h == t { hit += 1; "hit " } else if *h == 0 { miss += 1; "MISS" } else { part += 1; "part" };
        let short = f.rsplit(['/', '\\']).next().unwrap_or(f);
        println!("{tag} {short}:{l} ({h}/{t})");
    }
    println!("guards={} pcs_resolved={resolved} unresolved={unresolved} matched={matched}", guards - base);
    println!("corpus={files} lines: hit={hit} part={part} miss={miss}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("usage: engine <target> <seed-dir> <work-dir> <seconds> [rng-seed]");
        std::process::exit(2);
    }
    let target = targets::lookup(&args[1]).unwrap_or_else(|| { eprintln!("unknown target"); std::process::exit(2) });
    #[cfg(windows)]
    if args[4] == "lines" {
        line_report(target, Path::new(&args[2]), &args[3]);
        return;
    }
    let seeds = PathBuf::from(&args[2]);
    let work = PathBuf::from(&args[3]);
    let budget = Duration::from_secs(args[4].parse().expect("seconds"));
    let seed: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0x9E37_79B9_7F4A_7C15) | 1;
    let corpus_dir = work.join("corpus");
    let crash_dir = work.join("crashes");
    fs::create_dir_all(&corpus_dir).unwrap();
    fs::create_dir_all(&crash_dir).unwrap();

    panic::set_hook(Box::new(|info| {
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        PANIC_MSG.with(|m| *m.borrow_mut() = format!("{loc}: {msg}"));
    }));

    let guards = NEXT_GUARD.load(Ordering::Relaxed);
    let mut rng = Rng(seed);
    let mut seen: HashSet<u32> = HashSet::new();
    let mut corpus: Vec<Vec<u8>> = Vec::new();
    let mut crashes: HashMap<String, usize> = HashMap::new();
    let mut slowest = Duration::ZERO;
    let mut log = String::new();

    let mut initial: Vec<Vec<u8>> = Vec::new();
    for dir in [&seeds, &corpus_dir] {
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Ok(b) = fs::read(e.path()) { if b.len() <= MAX_LEN { initial.push(b) } }
            }
        }
    }
    if initial.is_empty() { initial.push(Vec::new()); }

    let mut slow_inputs = 0_u64;
    let record = |input: &[u8], ex: &Exec, crashes: &mut HashMap<String, usize>, crash_dir: &Path| {
        if let Outcome::Panic(msg) = &ex.outcome {
            let n = crashes.entry(msg.clone()).or_insert(0);
            if *n == 0 {
                let name = format!("crash-{}", hex(input));
                fs::write(crash_dir.join(&name), input).unwrap();
                fs::write(crash_dir.join(format!("{name}.txt")), msg).unwrap();
                eprintln!("CRASH {msg} -> {name}");
            }
            *n += 1;
        }
    };

    let started = Instant::now();
    let mut execs: u64 = 0;
    let mut last_new_exec: u64 = 0;
    let mut last_new_time = Duration::ZERO;
    for input in initial {
        let ex = execute(target, &input, guards);
        execs += 1;
        record(&input, &ex, &mut crashes, &crash_dir);
        let new = ex.features.iter().filter(|f| seen.insert(**f)).count();
        if new > 0 || corpus.is_empty() {
            fs::write(corpus_dir.join(hex(&input)), &input).unwrap();
            corpus.push(input);
        }
    }
    let seed_features = seen.len();
    let mut next_report = Duration::from_secs(30);
    RECORD_CMP.store(true, Ordering::Relaxed);
    while started.elapsed() < budget {
        let dict: Vec<(u64, u64, u8)> = unsafe { CMP_TABLE.iter().copied().filter(|x| x.2 != 0).collect() };
        for _ in 0..1000 {
            let base = &corpus[rng.below(corpus.len())];
            let input = mutate(&mut rng, base, &corpus, &dict);
            let ex = execute(target, &input, guards);
            execs += 1;
            slowest = slowest.max(ex.elapsed);
            if ex.elapsed > Duration::from_secs(1) {
                fs::write(crash_dir.join(format!("slow-{}", hex(&input))), &input).unwrap();
                slow_inputs += 1;
            }
            record(&input, &ex, &mut crashes, &crash_dir);
            let mut new = 0;
            for f in &ex.features { if seen.insert(*f) { new += 1 } }
            if new > 0 && matches!(ex.outcome, Outcome::Ok) {
                fs::write(corpus_dir.join(hex(&input)), &input).unwrap();
                corpus.push(input);
                last_new_exec = execs;
                last_new_time = started.elapsed();
            }
        }
        if started.elapsed() >= next_report {
            let hit: HashSet<u32> = seen.iter().map(|f| f / 16).collect();
            let line = format!(
                "t={}s execs={} rate={}/s corpus={} features={} edges={}/{} last_new_at={}s crashes={}\n",
                started.elapsed().as_secs(), execs, execs / started.elapsed().as_secs().max(1),
                corpus.len(), seen.len(), hit.len(), guards - 1, last_new_time.as_secs(), crashes.len());
            eprint!("{line}");
            log.push_str(&line);
            next_report += Duration::from_secs(30);
        }
    }
    let hit: HashSet<u32> = seen.iter().map(|f| f / 16).collect();
    let summary = format!(
        "{{\"target\":\"{}\",\"seconds\":{},\"execs\":{},\"rate\":{},\"corpus\":{},\"seedFeatures\":{},\"features\":{},\"edgesHit\":{},\"guardsInImage\":{},\"lastNewExec\":{},\"lastNewSecond\":{},\"execsSinceNew\":{},\"slowestMicros\":{},\"slowInputs\":{},\"distinctCrashes\":{},\"crashes\":{:?}}}\n",
        args[1], started.elapsed().as_secs(), execs, execs / started.elapsed().as_secs().max(1),
        corpus.len(), seed_features, seen.len(), hit.len(), guards - 1, last_new_exec,
        last_new_time.as_secs(), execs - last_new_exec, slowest.as_micros(), slow_inputs, crashes.len(), crashes);
    fs::write(work.join("stats.json"), &summary).unwrap();
    fs::write(work.join("progress.log"), &log).unwrap();
    print!("{summary}");
    let failed = !crashes.is_empty() || slow_inputs > 0;
    let expect_crash = args[1] == "canary";
    std::process::exit(if failed == expect_crash { 0 } else { 1 });
}
