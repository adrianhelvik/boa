//! Read/write metadata for opcode register operands.
//!
//! Each entry returns the role of each register operand (in source-code
//! order) of a given opcode. This is the soundness foundation for the
//! peephole pass: without it we can't tell whether a register reference
//! is a use we must preserve or a def we can move past.
//!
//! # Maintenance protocol
//!
//! - **Every opcode that takes a [`RegisterOperand`] field is either
//!   listed here, or the peephole pass treats it as a hard "stop"
//!   (fail-closed).** Adding an opcode to the whitelist requires
//!   checking the opcode's `Operation::operation` against its doc
//!   header (`// - Registers: Input: …  Output: …`).
//! - **The order of [`OperandRole`]s in the returned slice must match
//!   the field declaration order in the opcode's `generate_opcodes!`
//!   block.** That's the order operands are encoded by
//!   `Argument::encode` for tuples, and the order the peephole pass
//!   reads them out of the bytecode.
//! - **Index/immediate/address operands are excluded.** Only
//!   `RegisterOperand` fields go in.
//!
//! The whitelist is intentionally small. Each entry has a comment
//! tying it back to the opcode definition.

use crate::vm::opcode::Opcode;

/// The role of a register operand for the peephole analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandRole {
    /// The opcode reads from this register.
    Read,
    /// The opcode writes to this register (and does *not* read it as
    /// part of the same operation — read/write registers must be
    /// modelled by the analysis user as `Read`, since eliding a
    /// preceding write into them would still corrupt the read).
    Write,
}

/// Return the [`OperandRole`] of every [`RegisterOperand`] field of
/// `opcode`, in source-code order.
///
/// `None` means "we don't have metadata for this opcode". The peephole
/// analysis treats that as a hard stop.
pub(crate) fn operand_info(opcode: Opcode) -> Option<&'static [OperandRole]> {
    use OperandRole::{Read, Write};
    Some(match opcode {
        // ─────────────────────────────────────────────────────────────
        // The class-A bug class. These are the opcodes the
        // bytecompiler's receiver-passthrough fast paths emit, and the
        // primary motivation for the peephole pass.
        // ─────────────────────────────────────────────────────────────

        // GetPropertyByName { dst, value, ic_index }
        //   Registers: Input: value, Output: dst
        Opcode::GetPropertyByName => &[Write, Read],
        // GetPropertyByNameWithThis { dst, receiver, value, ic_index }
        //   Registers: Input: receiver, value; Output: dst
        Opcode::GetPropertyByNameWithThis => &[Write, Read, Read],
        // SetPropertyByName { value, object, ic_index }
        //   Registers: Input: object, value
        Opcode::SetPropertyByName => &[Read, Read],
        // SetPropertyByNameWithThis { value, receiver, object, ic_index }
        //   Registers: Input: object, receiver, value
        Opcode::SetPropertyByNameWithThis => &[Read, Read, Read],
        // DefineOwnPropertyByName { object, value, name_index }
        //   Registers: Input: object, value
        Opcode::DefineOwnPropertyByName => &[Read, Read],

        // ─────────────────────────────────────────────────────────────
        // Small operand-level utilities used by the bytecompiler's
        // expression lowering. Useful for the dead-after analysis to
        // see past these without bailing.
        //
        // (Move is handled directly by the analysis, not here.)
        // ─────────────────────────────────────────────────────────────

        // SetAccumulator { src }  Input: src
        Opcode::SetAccumulator => &[Read],
        // SetRegisterFromAccumulator { dst }  Output: dst
        Opcode::SetRegisterFromAccumulator => &[Write],
        // PopIntoRegister { dst }  Output: dst
        Opcode::PopIntoRegister => &[Write],
        // PushFromRegister { src }  Input: src
        Opcode::PushFromRegister => &[Read],

        // ─────────────────────────────────────────────────────────────
        // Anything we don't recognize stops the analysis. Adding more
        // entries is purely additive: each unlocks more elision sites
        // but is gated on a manual read/write audit.
        // ─────────────────────────────────────────────────────────────
        _ => return None,
    })
}
