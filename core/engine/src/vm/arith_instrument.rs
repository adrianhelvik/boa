//! Opportunity instrumentation for adaptive arithmetic opcodes.
//!
//! This module is **off by default**. It is compiled in only when the
//! `arith-instrument` cargo feature is enabled:
//!
//! ```text
//! cargo build --release -p boa_benches --bin bench-compare-runner \
//!     --features boa_engine/arith-instrument
//! ```
//!
//! Its purpose is to answer the cheap question from
//! `planning/js-performance-roadmap/01-measurement-methodology.md`:
//! *does the adaptive-arithmetic lever (PEP-659-style quickening of generic
//! Add/Sub/Mul/... to integer fast-path opcodes) actually have work to do on
//! real workloads?*
//!
//! For every dynamic execution of an instrumented binary-arith opcode we record,
//! keyed by the static site `(CodeBlock pointer, pc)`:
//!
//! - `total`     — how many times this site executed.
//! - `mono_i32`  — both operands were `Integer32` and the i32 fast op did **not**
//!                 overflow (i.e. perfectly specializable to an int fast-path op).
//! - `i32_ovf`   — both operands were `Integer32` but the result overflowed i32
//!                 (a specialized op would have to deopt here).
//! - `f64`       — both operands were numeric and at least one was `Float64`.
//! - `other`     — anything else (string concat, object, bigint, bool, undefined…).
//!
//! Counts live in a `thread_local` map; a `Drop` guard registered on first use
//! dumps a report to stderr when the thread exits (covers the bench runner's main
//! thread). Set `BOA_ARITH_DUMP=path` to also write the report to a file.

use std::cell::RefCell;
use std::collections::HashMap;

/// Per-operator-kind tag, so the report can break down by opcode family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ArithKind {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Ushr,
    Eq,
    NotEq,
    Gt,
    Ge,
    Lt,
    Le,
}

impl ArithKind {
    fn as_str(self) -> &'static str {
        match self {
            ArithKind::Add => "Add",
            ArithKind::Sub => "Sub",
            ArithKind::Mul => "Mul",
            ArithKind::Div => "Div",
            ArithKind::Mod => "Mod",
            ArithKind::Pow => "Pow",
            ArithKind::BitAnd => "BitAnd",
            ArithKind::BitOr => "BitOr",
            ArithKind::BitXor => "BitXor",
            ArithKind::Shl => "ShiftLeft",
            ArithKind::Shr => "ShiftRight",
            ArithKind::Ushr => "UnsignedShiftRight",
            ArithKind::Eq => "Eq",
            ArithKind::NotEq => "NotEq",
            ArithKind::Gt => "GreaterThan",
            ArithKind::Ge => "GreaterThanOrEq",
            ArithKind::Lt => "LessThan",
            ArithKind::Le => "LessThanOrEq",
        }
    }
}

/// Operand-pair classification for one dynamic execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandClass {
    /// Both `Integer32`, i32 op would not overflow -> monomorphic-i32 specializable.
    MonoI32,
    /// Both `Integer32` but the i32 op overflowed -> a specialized op must deopt.
    I32Overflow,
    /// Both numeric, at least one `Float64`.
    F64,
    /// Anything else (string, object, bigint, bool, undefined, mixed, symbol...).
    Other,
}

#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    total: u64,
    mono_i32: u64,
    i32_ovf: u64,
    f64: u64,
    other: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SiteKey {
    code_block: usize,
    pc: u32,
    kind: ArithKind,
}

struct Registry {
    sites: HashMap<SiteKey, Counts>,
}

impl Registry {
    fn new() -> Self {
        Self {
            sites: HashMap::new(),
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        let report = render_report(&self.sites);
        eprint!("{report}");
        if let Ok(path) = std::env::var("BOA_ARITH_DUMP") {
            drop(std::fs::write(&path, report));
        }
    }
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::new());
}

/// Record one dynamic execution of an instrumented arith opcode.
///
/// `code_block` is the raw pointer address of the executing `CodeBlock` (stable
/// site identity within a process), `pc` the program counter of the site, `kind`
/// the operator family, and `class` the operand classification computed by the
/// caller (which has access to the operand values before any clone/coercion).
#[inline]
pub(crate) fn record(code_block: usize, pc: u32, kind: ArithKind, class: OperandClass) {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        let entry = r
            .sites
            .entry(SiteKey {
                code_block,
                pc,
                kind,
            })
            .or_default();
        entry.total += 1;
        match class {
            OperandClass::MonoI32 => entry.mono_i32 += 1,
            OperandClass::I32Overflow => entry.i32_ovf += 1,
            OperandClass::F64 => entry.f64 += 1,
            OperandClass::Other => entry.other += 1,
        }
    });
}

#[allow(clippy::cast_precision_loss)]
fn render_report(sites: &HashMap<SiteKey, Counts>) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();

    let n_sites = sites.len();
    let mut grand = Counts::default();
    for c in sites.values() {
        grand.total += c.total;
        grand.mono_i32 += c.mono_i32;
        grand.i32_ovf += c.i32_ovf;
        grand.f64 += c.f64;
        grand.other += c.other;
    }

    let pct = |n: u64, d: u64| -> f64 {
        if d == 0 {
            0.0
        } else {
            100.0 * (n as f64) / (d as f64)
        }
    };

    out.push_str("\n===== BOA ARITH OPPORTUNITY REPORT =====\n");
    let _ = writeln!(out, "static arith sites: {n_sites}");
    let _ = writeln!(out, "total arith executions: {}", grand.total);
    let _ = writeln!(
        out,
        "  mono_i32  : {:>14} ({:5.2}%)",
        grand.mono_i32,
        pct(grand.mono_i32, grand.total)
    );
    let _ = writeln!(
        out,
        "  i32_ovf   : {:>14} ({:5.2}%)",
        grand.i32_ovf,
        pct(grand.i32_ovf, grand.total)
    );
    let _ = writeln!(
        out,
        "  f64       : {:>14} ({:5.2}%)",
        grand.f64,
        pct(grand.f64, grand.total)
    );
    let _ = writeln!(
        out,
        "  other     : {:>14} ({:5.2}%)",
        grand.other,
        pct(grand.other, grand.total)
    );
    let specializable = grand.mono_i32 + grand.i32_ovf;
    let _ = writeln!(
        out,
        "  int-typed (mono+ovf): {:>6} ({:5.2}%)",
        specializable,
        pct(specializable, grand.total)
    );

    // Per-kind breakdown.
    let mut by_kind: HashMap<ArithKind, Counts> = HashMap::new();
    for (k, c) in sites {
        let e = by_kind.entry(k.kind).or_default();
        e.total += c.total;
        e.mono_i32 += c.mono_i32;
        e.i32_ovf += c.i32_ovf;
        e.f64 += c.f64;
        e.other += c.other;
    }
    let mut kinds: Vec<(ArithKind, Counts)> = by_kind.into_iter().collect();
    kinds.sort_by(|a, b| b.1.total.cmp(&a.1.total));
    out.push_str("\n-- by opcode kind (total / mono_i32% / f64% / other%) --\n");
    for (k, c) in &kinds {
        let _ = writeln!(
            out,
            "  {:<20} {:>14}  mono {:5.2}%  ovf {:5.2}%  f64 {:5.2}%  other {:5.2}%",
            k.as_str(),
            c.total,
            pct(c.mono_i32, c.total),
            pct(c.i32_ovf, c.total),
            pct(c.f64, c.total),
            pct(c.other, c.total),
        );
    }

    // Hot-site concentration.
    let mut by_site: Vec<(&SiteKey, &Counts)> = sites.iter().collect();
    by_site.sort_by(|a, b| b.1.total.cmp(&a.1.total));

    out.push_str("\n-- hot-site concentration --\n");
    for top in [1usize, 5, 10, 20, 50] {
        let sum: u64 = by_site.iter().take(top).map(|(_, c)| c.total).sum();
        let _ = writeln!(
            out,
            "  top-{:<3} sites = {:5.2}% of all arith executions",
            top,
            pct(sum, grand.total)
        );
    }

    out.push_str("\n-- top 20 sites (kind cb@pc: total | mono / ovf / f64 / other) --\n");
    for (k, c) in by_site.iter().take(20) {
        let _ = writeln!(
            out,
            "  {:<18} {:#x}@{:<5}  {:>12} | m {:5.2}% o {:5.2}% f {:5.2}% x {:5.2}%",
            k.kind.as_str(),
            k.code_block,
            k.pc,
            c.total,
            pct(c.mono_i32, c.total),
            pct(c.i32_ovf, c.total),
            pct(c.f64, c.total),
            pct(c.other, c.total),
        );
    }
    out.push_str("===== END BOA ARITH OPPORTUNITY REPORT =====\n");

    out
}
