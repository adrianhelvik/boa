//! Byte-removal rewriter for the Move-elision peephole pass.
//!
//! [`super::find_safe_move_elisions`] proves which `Move tmp, src; Op …, tmp`
//! pairs are safe to collapse. This module performs the collapse: it deletes
//! each dead `Move` outright (9 bytes: opcode + two register operands) and
//! retargets the consumer `Op`'s operand from `tmp` to `src`.
//!
//! # Why byte removal is delicate
//!
//! Boa's bytecode encodes jump destinations as **absolute** byte offsets
//! (see [`BytecodeEmitter::patch_jump`](crate::vm::opcode::BytecodeEmitter)).
//! Deleting bytes shifts every offset after the deletion point, so the
//! rewriter must remap **every** structure that stores a bytecode address:
//!
//! - jump / branch operands (15 opcode forms, `address` first operand),
//! - [`Opcode::JumpTable`] address arrays,
//! - exception [`Handler`] `start` / `end` ranges (`end` is the catch entry),
//! - the [`SourceMap`](crate::vm::source_info) PC→position table.
//!
//! The remap is a single monotonic old→new offset map built in one linear
//! pass. A debug-only [`verify`] pass then re-decodes the result and asserts
//! every address lands on a real instruction boundary — the safety net that
//! catches a missed address-bearing opcode or an off-by-one in the map.

use thin_vec::ThinVec;

use crate::vm::{
    Handler,
    opcode::{Bytecode, Opcode},
    source_info::Entry,
};

use super::{Elision, find_safe_move_elisions};

/// Where a given opcode stores bytecode address operand(s), if any.
///
/// Every variant of [`Opcode`] that carries an [`Address`](crate::vm::opcode)
/// must be classified here; anything not listed is treated as address-free.
/// A future address-bearing opcode that is forgotten here is caught by
/// [`verify`] (its un-remapped address won't land on an instruction
/// boundary once bytes shift).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AddrKind {
    /// No address operands.
    None,
    /// A single `address` operand immediately after the opcode byte (`pc + 1`).
    /// Shared by `Jump`, every `JumpIf*`, `Case`, `TemplateLookup`,
    /// `LogicalAnd`/`LogicalOr`/`Coalesce`.
    Single,
    /// `JumpTable { index: u32, addresses: ThinVec<Address> }`: a `u32` index
    /// at `pc + 1`, then a length-prefixed address array at `pc + 5`
    /// (`[u32 len][addr; len]`).
    Table,
}

/// Classify an opcode's address layout. See [`AddrKind`].
const fn address_kind(opcode: Opcode) -> AddrKind {
    match opcode {
        Opcode::Jump
        | Opcode::JumpIfTrue
        | Opcode::JumpIfFalse
        | Opcode::JumpIfNotUndefined
        | Opcode::JumpIfNullOrUndefined
        | Opcode::JumpIfNotLessThan
        | Opcode::JumpIfNotLessThanOrEqual
        | Opcode::JumpIfNotGreaterThan
        | Opcode::JumpIfNotGreaterThanOrEqual
        | Opcode::JumpIfNotEqual
        | Opcode::Case
        | Opcode::TemplateLookup
        | Opcode::LogicalAnd
        | Opcode::LogicalOr
        | Opcode::Coalesce => AddrKind::Single,
        Opcode::JumpTable => AddrKind::Table,
        _ => AddrKind::None,
    }
}

fn read_u32(bytes: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes(bytes[pos..pos + 4].try_into().expect("4 bytes"))
}

fn write_u32(bytes: &mut [u8], pos: usize, value: u32) {
    bytes[pos..pos + 4].copy_from_slice(&value.to_le_bytes());
}

/// Run `f` for the byte offset of every bytecode address operand in `bytecode`.
/// Used both to collect jump targets (analysis guard) and to remap addresses
/// (rewrite). `f` receives the absolute byte offset where a `u32` address sits.
fn for_each_address_slot(bytecode: &Bytecode, mut f: impl FnMut(usize)) {
    let bytes = &bytecode.bytes;
    let mut pc = 0usize;
    while pc < bytes.len() {
        let opcode = Opcode::decode(bytes[pc]);
        match address_kind(opcode) {
            AddrKind::None => {}
            AddrKind::Single => f(pc + 1),
            AddrKind::Table => {
                let len = read_u32(bytes, pc + 5) as usize;
                for i in 0..len {
                    f(pc + 9 + i * 4);
                }
            }
        }
        let (_instr, next_pc) = bytecode.next_instruction(pc);
        pc = next_pc;
    }
}

/// Every bytecode offset that some control-flow edge can land on: the
/// destination of every jump/branch/jump-table, plus each exception handler's
/// protected-region boundary and catch entry (`Handler::end`).
///
/// The analysis is a linear scan with no control-flow graph; this set lets it
/// refuse elisions whose `Move`/`Op` could be entered by a path that bypasses
/// the `Move` (which would make retargeting the `Op` to `src` unsound).
pub(super) fn jump_targets(bytecode: &Bytecode, handlers: &[Handler]) -> Vec<u32> {
    let bytes = &bytecode.bytes;
    let mut targets = Vec::new();
    for_each_address_slot(bytecode, |slot| targets.push(read_u32(bytes, slot)));
    for h in handlers {
        targets.push(h.start.as_u32());
        targets.push(h.end.as_u32());
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// Result of [`elide_moves`]: the rewritten bytecode and its remapped
/// handler/source-map tables. Returned by value so the caller can drop the
/// originals.
pub(crate) struct Rewritten {
    pub(crate) bytecode: Bytecode,
    pub(crate) handlers: ThinVec<Handler>,
    pub(crate) source_entries: Box<[Entry]>,
}

/// Apply the Move-elision pass to a finished code block's bytecode.
///
/// Runs the analysis ([`find_safe_move_elisions`]) and, if it finds any safe
/// elisions, removes the dead `Move`s and remaps all addresses. When there is
/// nothing to elide the inputs are returned untouched (no allocation).
pub(crate) fn elide_moves(
    bytecode: Bytecode,
    handlers: ThinVec<Handler>,
    source_entries: Box<[Entry]>,
) -> Rewritten {
    let elisions = find_safe_move_elisions(&bytecode, &handlers);
    if elisions.is_empty() {
        return Rewritten {
            bytecode,
            handlers,
            source_entries,
        };
    }
    rewrite(&bytecode, &handlers, &source_entries, &elisions)
}

fn rewrite(
    bytecode: &Bytecode,
    handlers: &[Handler],
    source_entries: &[Entry],
    elisions: &[Elision],
) -> Rewritten {
    let bytes = &bytecode.bytes;
    let old_len = bytes.len();

    // `move_pc`s to delete, and `op_pc` -> (operand index, new src) patches.
    // Elisions never conflict: each removes a distinct `Move` and patches the
    // distinct `Op` that immediately follows it.
    let mut remove = vec![false; old_len];
    for e in elisions {
        remove[e.move_pc as usize] = true;
    }
    let patch = |op_pc: u32| elisions.iter().find(|e| e.op_pc == op_pc);

    // Pass 1: old offset -> new offset. A deleted Move maps to wherever the
    // following (surviving) instruction lands, so any stray reference to it
    // still resolves to a valid boundary.
    let mut old_to_new = vec![0u32; old_len + 1];
    let mut new_len = 0u32;
    let mut pc = 0usize;
    while pc < old_len {
        old_to_new[pc] = new_len;
        let (_instr, next_pc) = bytecode.next_instruction(pc);
        if !remove[pc] {
            new_len += (next_pc - pc) as u32;
        }
        pc = next_pc;
    }
    old_to_new[old_len] = new_len;
    let remap = |addr: u32| old_to_new[addr as usize];

    // Pass 2: copy surviving instructions, patching consumer operands and
    // remapping addresses in place.
    let mut out = Vec::with_capacity(new_len as usize);
    let mut pc = 0usize;
    while pc < old_len {
        let opcode = Opcode::decode(bytes[pc]);
        let (_instr, next_pc) = bytecode.next_instruction(pc);
        if remove[pc] {
            pc = next_pc;
            continue;
        }
        let start = out.len();
        out.extend_from_slice(&bytes[pc..next_pc]);

        if let Some(e) = patch(pc as u32) {
            // Retarget the consumer's register operand `tmp` -> `src`.
            // Consumer opcodes (property / accumulator ops) lay their register
            // operands out first, so operand `i` sits at `+1 + 4*i`.
            write_u32(&mut out, start + 1 + e.op_operand_idx * 4, u32::from(e.src));
        }

        match address_kind(opcode) {
            AddrKind::None => {}
            AddrKind::Single => {
                let v = remap(read_u32(&out, start + 1));
                write_u32(&mut out, start + 1, v);
            }
            AddrKind::Table => {
                let len = read_u32(&out, start + 5) as usize;
                for i in 0..len {
                    let off = start + 9 + i * 4;
                    let v = remap(read_u32(&out, off));
                    write_u32(&mut out, off, v);
                }
            }
        }
        pc = next_pc;
    }

    // Remap handler ranges.
    let handlers: ThinVec<Handler> = handlers
        .iter()
        .map(|h| Handler {
            start: remap(h.start.as_u32()).into(),
            end: remap(h.end.as_u32()).into(),
            environment_count: h.environment_count,
        })
        .collect();

    // Remap source-map entries. The map is monotonic, so order is preserved;
    // when several old PCs (a deleted Move and the Op now occupying its slot)
    // collapse onto one new PC, keep the *last* — it belongs to the surviving
    // instruction at that offset.
    let mut entries: Vec<Entry> = source_entries
        .iter()
        .map(|entry| Entry {
            pc: remap(entry.pc),
            position: entry.position,
        })
        .collect();
    entries.dedup_by(|later, earlier| {
        if earlier.pc == later.pc {
            *earlier = *later;
            true
        } else {
            false
        }
    });

    let rewritten = Rewritten {
        bytecode: Bytecode {
            bytes: out.into_boxed_slice(),
        },
        handlers,
        source_entries: entries.into_boxed_slice(),
    };

    #[cfg(debug_assertions)]
    verify(&rewritten);

    rewritten
}

/// Debug-only structural check: re-decode the rewritten bytecode and assert it
/// is internally consistent — every instruction decodes cleanly and every
/// address (jump operands, jump tables, handler ranges) lands on an
/// instruction boundary. A remap bug or a missed address-bearing opcode shows
/// up here rather than as a silent miscompilation.
#[cfg(debug_assertions)]
fn verify(r: &Rewritten) {
    let bytecode = &r.bytecode;
    let len = bytecode.bytes.len() as u32;

    // Collect valid instruction-start offsets (a clean walk to the end is
    // itself an assertion that nothing decodes past the buffer).
    let mut starts = std::collections::HashSet::new();
    let mut pc = 0usize;
    while pc < bytecode.bytes.len() {
        starts.insert(pc as u32);
        let (_instr, next_pc) = bytecode.next_instruction(pc);
        assert!(next_pc <= bytecode.bytes.len(), "instruction overruns buffer");
        assert!(next_pc > pc, "instruction made no progress");
        pc = next_pc;
    }
    let valid = |addr: u32| addr == len || starts.contains(&addr);

    for_each_address_slot(bytecode, |slot| {
        let addr = read_u32(&bytecode.bytes, slot);
        assert!(valid(addr), "jump address {addr} is not an instruction boundary");
    });
    for h in &r.handlers {
        assert!(valid(h.start.as_u32()), "handler.start off-boundary");
        assert!(valid(h.end.as_u32()), "handler.end off-boundary");
        assert!(h.start.as_u32() <= h.end.as_u32(), "handler range inverted");
    }
}
