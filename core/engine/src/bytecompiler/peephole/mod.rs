//! Peephole analysis over emitted bytecode.
//!
//! The bytecompiler currently has several "operand passthrough" call sites
//! (see [`crate::bytecompiler::ByteCompiler::compile_expr_operand_stable`])
//! whose job is to decide whether handing a binding's persistent register
//! directly to the next opcode is safe, or whether a snapshot Move is
//! required first. The point fix that landed for the
//! `o.x = (o = {}, 42)` family of bugs is a per-call-site override of that
//! decision, audited by hand and protected by regression tests under
//! `tests/operators.rs`.
//!
//! That pattern decays. Every new opcode that wants the same fast path
//! has to re-do the audit. The intent of this module is to move the
//! soundness reasoning into a single place: the bytecompiler always emits
//! the obvious `Move snapshot, src; Op …, snapshot, …` pattern, and a
//! post-pass elides the Move when it can prove the elision is safe. The
//! conditions, taken from ES §13.15.2 read as a property of the bytecode:
//!
//! 1. `snapshot` is not read after `Op`. Otherwise the elision would
//!    change the value observed by a later reader.
//! 2. `src` is not written between the Move and the Op. Otherwise the
//!    elision would feed a different value into `Op` than the Move
//!    captured.
//!
//! # Scope of this prototype
//!
//! The analysis is intentionally narrow and refuses to make decisions
//! about opcodes it does not have read/write metadata for. The whitelist
//! in [`operand_info`] currently covers exactly the property-access
//! opcodes that the bytecompiler's existing receiver-passthrough fast
//! paths target. Two reasons for keeping it small:
//!
//! - Boa has ~250 opcodes. A complete operand-info table is a large
//!   audit surface in itself and trading one form of bug-class hazard
//!   for another doesn't help. Better to grow the table as we add
//!   call sites that benefit, with each addition gated on tests.
//! - The macro [`generate_opcodes!`](crate::vm::opcode) does not
//!   currently expose per-field read/write metadata. Surfacing it would
//!   either require attribute annotations on every opcode definition
//!   (hundreds of edits, easy to get wrong) or a side table maintained
//!   in lock-step with the macro (this file). The side table is
//!   verifiable: every entry can be cross-referenced against the
//!   opcode's `Operation::operation` implementation, and adding a debug
//!   assertion that runs the analysis against every emitted CodeBlock
//!   in test builds would catch table drift.
//!
//! # What this module ships
//!
//! The analysis: [`find_safe_move_elisions`] scans a [`Bytecode`] block
//! and returns every adjacent `Move tmp, src` + `Op …, tmp, …` pair
//! that can be safely collapsed under the conditions above. Beyond the two
//! data-flow conditions it also enforces a control-flow condition: neither
//! instruction may be an incoming jump/handler target (the scan is linear
//! and has no CFG, so an edge bypassing the `Move` would be invisible).
//!
//! The rewriter ([`rewrite::elide_moves`]) deletes each dead `Move` and
//! retargets the consumer, remapping every absolute address (jumps, jump
//! tables, exception-handler ranges, source-map PCs) through a single
//! old→new offset map; a debug-only pass re-decodes the result to assert
//! structural integrity, and unit tests cover the analysis, both guards, and
//! a byte-level rewrite round-trip.
//!
//! It is **not** wired into [`ByteCompiler::finish`]. The rewrite is correct
//! but shows no measured runtime win: the bytecompiler's per-call-site
//! receiver-passthrough fast paths already avoid emitting most of the
//! redundant `Move`s, so the analysis finds few opportunities and running a
//! rewriter on every [`CodeBlock`](crate::vm::CodeBlock) is risk without
//! reward. Enable it from `finish` once a benchmark justifies it (and drop the
//! module-level `allow(dead_code)` below at that point).
//!
//! [`ByteCompiler::finish`]: crate::bytecompiler::ByteCompiler::finish

// Dormant subsystem: implemented and tested, but not yet called from a live
// path (see the module docs). Remove when wired into `ByteCompiler::finish`.
#![allow(dead_code)]

use crate::vm::{
    Handler,
    opcode::{Bytecode, InstructionIterator, Opcode, RegisterOperand},
};

mod operand_info;
mod rewrite;
#[cfg(test)]
mod tests;

pub(crate) use operand_info::{OperandRole, operand_info};
// The rewriter entry points are exercised by tests and ready for `finish` to
// call; unused on the non-test build path only because the pass is dormant.
#[allow(unused_imports)]
pub(crate) use rewrite::{Rewritten, elide_moves};

/// A safe Move-elision opportunity found by [`find_safe_move_elisions`].
///
/// The pair represents an adjacent `Move dst, src` + `Op …` where `dst`
/// appears at exactly one register-operand position in `Op` and the
/// elision conditions hold. A rewriter would replace the `Move` with a
/// padding instruction and rewrite the `Op`'s operand at `op_operand_idx`
/// from `dst` to `src`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Elision {
    /// PC of the `Move` instruction.
    pub(crate) move_pc: u32,
    /// PC of the `Op` immediately following the `Move`.
    pub(crate) op_pc: u32,
    /// Source register of the `Move` (the operand `Op` would read post-elision).
    pub(crate) src: RegisterOperand,
    /// Destination register of the `Move` (the temporary `Op` reads pre-elision).
    pub(crate) dst: RegisterOperand,
    /// Zero-based index, within `Op`'s register-operand list, of the slot
    /// that holds `dst`. A rewriter patches the four bytes at this
    /// position to encode `src` instead.
    pub(crate) op_operand_idx: usize,
}

impl PartialEq for Elision {
    fn eq(&self, other: &Self) -> bool {
        // `RegisterOperand` doesn't derive `Eq`; compare via its `u32`
        // projection. Used only by tests.
        self.move_pc == other.move_pc
            && self.op_pc == other.op_pc
            && u32::from(self.src) == u32::from(other.src)
            && u32::from(self.dst) == u32::from(other.dst)
            && self.op_operand_idx == other.op_operand_idx
    }
}
impl Eq for Elision {}

/// Scan the bytecode for safe Move-elision opportunities.
///
/// The returned list contains every `Move; Op` adjacent pair where:
///
/// - `Op` is in the [`operand_info`] whitelist (so we know every register
///   operand's read/write role).
/// - The `Move`'s destination appears at exactly one register-operand
///   position in `Op` as a *read*.
/// - The `Move`'s destination is not read anywhere later in the
///   bytecode until it is next *written* (so eliminating the write
///   into it cannot change any observable register value).
/// - The `Move`'s source is also in [`operand_info`]'s vocabulary for
///   every opcode in the forward scan, so the analysis can answer the
///   "next read of `dst`" question without falling back to a worst-case
///   assumption.
///
/// The function is read-only — it doesn't mutate `bytecode`. A separate
/// rewriter would consume the [`Elision`] list and produce a new
/// `Bytecode`; see the module docs for why that rewriter is not in this
/// commit.
pub(crate) fn find_safe_move_elisions(bytecode: &Bytecode, handlers: &[Handler]) -> Vec<Elision> {
    // First pass: collect (pc, opcode, instruction-end-pc, register operands)
    // for every instruction. The forward "next use of dst" scan needs
    // random-access into this list anyway, and decoding twice would be
    // wasteful.
    let decoded = decode_all(bytecode);

    // Every offset some control-flow edge can land on. This analysis walks
    // the bytecode linearly with no control-flow graph, so it cannot see an
    // incoming jump/handler edge that reaches `Op` (or `Move`) without going
    // through the `Move`. On such a path `src` need not equal the snapshot the
    // `Move` captured, so retargeting `Op` to read `src` would be unsound.
    // Refuse to elide when either instruction is an entry point.
    let targets = rewrite::jump_targets(bytecode, handlers);
    let is_target = |pc: u32| targets.binary_search(&pc).is_ok();

    let mut out = Vec::new();

    for window in decoded.windows(2) {
        let (move_pc, ref move_inst) = window[0];
        let (op_pc, ref op_inst) = window[1];

        // Pattern: `Move dst, src` followed by an opcode we have metadata
        // for. Bail on anything else.
        let DecodedInstruction::Move { dst, src } = *move_inst else {
            continue;
        };

        // Control-flow soundness: neither the `Move` nor the `Op` may be
        // reachable except by falling through the `Move`.
        if is_target(move_pc) || is_target(op_pc) {
            continue;
        }
        let DecodedInstruction::Other {
            opcode,
            ref operands,
        } = *op_inst
        else {
            continue;
        };

        // Find where (if anywhere) `dst` appears in `Op`'s operand list.
        // The elision is well-defined only if `dst` appears exactly once,
        // as a *read*. (If it appeared twice, or as a write, retargeting
        // a single position to `src` would change semantics.)
        let info = match operand_info(opcode) {
            Some(info) => info,
            None => continue,
        };
        if info.len() != operands.len() {
            // Metadata is out of sync with the macro. Bail rather than
            // make a guess.
            continue;
        }
        let mut occurrences = operands
            .iter()
            .zip(info.iter())
            .enumerate()
            .filter(|(_, (op, _))| u32::from(**op) == u32::from(dst));
        let Some((idx, (_, role))) = occurrences.next() else {
            continue;
        };
        if occurrences.next().is_some() {
            // `dst` appears in multiple slots; the elision becomes
            // ambiguous about which slot to retarget.
            continue;
        }
        if !matches!(role, OperandRole::Read) {
            // `dst` is being written by `Op` (or read+written). Eliding
            // the Move would change the semantics of that write.
            continue;
        }

        // Condition 1: `src` isn't written between Move and Op. Since
        // the two are adjacent, there's nothing in between by
        // construction. (Generalising to a non-adjacent search would
        // need to verify this explicitly.)

        // Condition 2: `dst` is not read anywhere after Op before being
        // next overwritten. Walk the rest of the bytecode, checking
        // every instruction in our metadata vocabulary. If we hit an
        // opcode we don't have metadata for, fail closed.
        let pc_index = decoded
            .iter()
            .position(|(pc, _)| *pc == op_pc)
            .expect("op_pc must be present in decoded");
        if !dst_dead_after(&decoded, pc_index, dst) {
            continue;
        }

        out.push(Elision {
            move_pc,
            op_pc,
            src,
            dst,
            op_operand_idx: idx,
        });
    }

    out
}

/// Decoded form of an instruction for analysis purposes.
///
/// We keep two cases: `Move` (the pattern we care about) and `Other`
/// (everything else, carrying just the register-operand list). All other
/// operand kinds (indices, immediates, addresses) are irrelevant to the
/// analysis and are dropped on the floor.
#[derive(Debug, Clone)]
enum DecodedInstruction {
    Move {
        dst: RegisterOperand,
        src: RegisterOperand,
    },
    Other {
        opcode: Opcode,
        /// Register operands in source-code order (matches what
        /// [`operand_info`] returns for `opcode`).
        operands: Vec<RegisterOperand>,
    },
}

fn decode_all(bytecode: &Bytecode) -> Vec<(u32, DecodedInstruction)> {
    let mut iter = InstructionIterator::new(bytecode);
    let mut out = Vec::new();
    while let Some((pc, opcode, instruction)) = iter.next() {
        let decoded = match opcode {
            Opcode::Move => match instruction {
                crate::vm::opcode::Instruction::Move { dst, src } => {
                    DecodedInstruction::Move { dst, src }
                }
                _ => unreachable!("opcode/instruction mismatch for Move"),
            },
            other => {
                // Extract the register operands in source order. Using
                // the `Instruction` enum pattern-match shape would
                // require enumerating every variant here, which mirrors
                // the macro work we explicitly chose not to do. Instead,
                // ask `operand_info` for the operand positions and read
                // them out of the bytecode by hand.
                let operands = match operand_info(other) {
                    Some(info) => extract_operands(bytecode, pc, info.len()),
                    None => Vec::new(),
                };
                DecodedInstruction::Other {
                    opcode: other,
                    operands,
                }
            }
        };
        out.push((pc as u32, decoded));
    }
    out
}

/// Read `count` consecutive [`RegisterOperand`]s starting one byte after
/// the opcode at `opcode_pc`. This is sound *only* for opcodes whose
/// layout begins with `count` register operands; the [`operand_info`]
/// whitelist enforces that.
fn extract_operands(bytecode: &Bytecode, opcode_pc: usize, count: usize) -> Vec<RegisterOperand> {
    const REG_BYTES: usize = size_of::<u32>();
    let mut out = Vec::with_capacity(count);
    let mut pos = opcode_pc + 1;
    for _ in 0..count {
        let bytes = &bytecode.bytes[pos..pos + REG_BYTES];
        let raw = u32::from_le_bytes(bytes.try_into().expect("4 bytes"));
        out.push(RegisterOperand::from(raw));
        pos += REG_BYTES;
    }
    out
}

/// True if every subsequent instruction either:
///   - doesn't mention `dst`, OR
///   - mentions `dst` strictly as a *write* (then `dst`'s post-Move
///     value can't be observed by any later reader).
///
/// Fails closed: if we encounter an instruction in `decoded` whose
/// opcode isn't in our metadata vocabulary, return `false`. That keeps
/// the analysis sound at the cost of missing elisions.
fn dst_dead_after(
    decoded: &[(u32, DecodedInstruction)],
    op_index: usize,
    dst: RegisterOperand,
) -> bool {
    for (_, inst) in &decoded[op_index + 1..] {
        match inst {
            DecodedInstruction::Move { dst: m_dst, src: m_src } => {
                if u32::from(*m_src) == u32::from(dst) {
                    // Subsequent read of `dst` — elision would change
                    // what value this Move snapshots.
                    return false;
                }
                if u32::from(*m_dst) == u32::from(dst) {
                    // `dst` is overwritten by this Move with no
                    // intervening read. Safe to elide the earlier write.
                    return true;
                }
            }
            DecodedInstruction::Other { opcode, operands } => {
                let Some(info) = operand_info(*opcode) else {
                    return false;
                };
                if info.len() != operands.len() {
                    return false;
                }
                let mut saw_write_only = false;
                for (op, role) in operands.iter().zip(info.iter()) {
                    if u32::from(*op) != u32::from(dst) {
                        continue;
                    }
                    match role {
                        OperandRole::Read => return false,
                        OperandRole::Write => {
                            saw_write_only = true;
                        }
                    }
                }
                if saw_write_only {
                    return true;
                }
            }
        }
    }
    // Reached end of bytecode without encountering a read of `dst` —
    // nothing will ever read it.
    true
}
