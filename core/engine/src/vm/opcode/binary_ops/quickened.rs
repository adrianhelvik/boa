//! Quickened (type-specialized) variants of the arithmetic opcodes.
//!
//! These opcodes are written into the bytecode by the generic handlers after a
//! site has been observed to be monomorphic (PEP-659-style adaptive quickening).
//! Each specialized handler:
//!
//! 1. **Checks** that the operands still have the expected type.
//! 2. **Executes** the fast arithmetic path — no heap allocation, no clone, no
//!    full JS coercion ladder.
//! 3. **Deopts** on type mismatch or i32 overflow by rewriting the opcode byte
//!    *back* to the corresponding generic opcode and resetting `frame.pc` to the
//!    opcode's own PC (not `next_pc`).  The outer dispatch loop then re-executes
//!    the site using the generic handler, which restores correct JS semantics.
//!
//! ## Deopt correctness
//!
//! The deopt path:
//! - Rewrites `bytecode[pc]` to the generic opcode byte.
//! - Resets `frame.pc = pc as u32` (not `next_pc`).
//! - Returns `ControlFlow::Continue(())`.
//!
//! The outer loop reads `bytecode[pc]` again (now generic), decodes the same
//! operand bytes, and dispatches to the generic handler.  Jump targets are
//! unaffected because opcode bytes and operand bytes have the same positions
//! as before — only the one opcode byte changes.
//!
//! ## PC recovery
//!
//! By the time `operation` is called, `frame.pc` has already been advanced to
//! `next_pc` by the macro-generated handler wrapper.  We recover the original
//! opcode PC as `next_pc - QUICKENED_INSN_SIZE`.  This is always correct for
//! the quickened ops because they all encode exactly 3 × [`RegisterOperand`] =
//! 3 × 4 bytes of operands plus 1 opcode byte = 13 bytes total.
//!
//! ## Quickening threshold
//!
//! See [`crate::vm::opcode::binary_ops::macro_defined`] for the logic that
//! increments the quicken counter and triggers the specialization write.

use crate::{
    Context, JsValue, JsVariant,
    vm::opcode::{Opcode, Operation, RegisterOperand},
};

/// Number of monomorphic fast-path executions required before a generic
/// arithmetic opcode site is quickened to a specialized variant.
///
/// Matches the constant used in [`super::macro_defined`].
pub(crate) const QUICKEN_THRESHOLD: u8 = 8;

/// Total size of one quickened arithmetic instruction in bytes:
/// 1 (opcode) + 3 × 4 (RegisterOperand u32s).
const QUICKENED_INSN_SIZE: usize = 1 + 3 * 4;

// ---------------------------------------------------------------------------
// Helper: PC recovery and deopt
// ---------------------------------------------------------------------------

/// Recover the opcode PC of the currently executing quickened instruction.
///
/// `frame.pc` has been set to `next_pc` by the handler wrapper before
/// `operation` is invoked.  Since every quickened op is exactly
/// [`QUICKENED_INSN_SIZE`] bytes, the original opcode PC is `next_pc -
/// QUICKENED_INSN_SIZE`.
#[inline(always)]
fn opcode_pc(context: &Context) -> usize {
    (context.vm.frame().pc as usize) - QUICKENED_INSN_SIZE
}

/// Rewrite `bytecode[opcode_pc]` to `generic_opcode` and reset `frame.pc`
/// to `opcode_pc` so the outer dispatch loop re-executes the site via the
/// generic handler — which restores full JS semantics.
///
/// Also zeroes `quicken_state[opcode_pc]` so the generic handler's counter
/// starts fresh (allowing future re-specialization on a stable type stream).
#[inline(always)]
fn deopt(context: &mut Context, generic_opcode: Opcode) {
    let pc = opcode_pc(context);
    context
        .vm
        .frame()
        .code_block
        .bytecode
        .set_byte(pc, generic_opcode as u8);
    // Reset the warm-up counter so the site can re-specialize after a
    // transient type mismatch.
    if let Some(cell) = context.vm.frame().code_block.quicken_state.get(pc) {
        cell.set(0);
    }
    context.vm.frame_mut().pc = pc as u32;
}

// ---------------------------------------------------------------------------
// AddInt
// ---------------------------------------------------------------------------

/// Quickened `+` for i32 × i32 operands.
///
/// Deopts to [`Opcode::Add`] on:
/// - Either operand not `Integer32`
/// - `checked_add` overflow (result is still produced as `f64`)
#[derive(Debug, Clone, Copy)]
pub(crate) struct AddInt;

impl AddInt {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) {
        let lhs_val = context.vm.get_register(lhs.into());
        let rhs_val = context.vm.get_register(rhs.into());

        if let (JsVariant::Integer32(x), JsVariant::Integer32(y)) =
            (lhs_val.variant(), rhs_val.variant())
        {
            if let Some(result) = x.checked_add(y) {
                context.vm.set_register(dst.into(), JsValue::new(result));
                return;
            }
            // Overflow: produce correct f64 result then deopt.
            let f = f64::from(x) + f64::from(y);
            context.vm.set_register(dst.into(), JsValue::new(f));
            deopt(context, Opcode::Add);
            return;
        }

        // Type mismatch: deopt, result will be computed by generic handler.
        deopt(context, Opcode::Add);
    }
}

impl Operation for AddInt {
    const NAME: &'static str = "AddInt";
    const INSTRUCTION: &'static str = "INST - AddInt";
    const COST: u8 = 1;
}

// ---------------------------------------------------------------------------
// AddF64
// ---------------------------------------------------------------------------

/// Quickened `+` for numeric (i32 or f64) operands — optimised for the case
/// where at least one operand is `Float64`.
///
/// Deopts to [`Opcode::Add`] when either operand is non-numeric (string,
/// object, etc.), which would require the generic coercion path.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AddF64;

impl AddF64 {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) {
        let lhs_val = context.vm.get_register(lhs.into());
        let rhs_val = context.vm.get_register(rhs.into());

        let lx = match lhs_val.variant() {
            JsVariant::Integer32(i) => f64::from(i),
            JsVariant::Float64(f) => f,
            _ => {
                deopt(context, Opcode::Add);
                return;
            }
        };
        let ry = match rhs_val.variant() {
            JsVariant::Integer32(i) => f64::from(i),
            JsVariant::Float64(f) => f,
            _ => {
                deopt(context, Opcode::Add);
                return;
            }
        };

        context.vm.set_register(dst.into(), JsValue::new(lx + ry));
    }
}

impl Operation for AddF64 {
    const NAME: &'static str = "AddF64";
    const INSTRUCTION: &'static str = "INST - AddF64";
    const COST: u8 = 1;
}

// ---------------------------------------------------------------------------
// SubInt
// ---------------------------------------------------------------------------

/// Quickened `-` for i32 × i32 operands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SubInt;

impl SubInt {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) {
        let lhs_val = context.vm.get_register(lhs.into());
        let rhs_val = context.vm.get_register(rhs.into());

        if let (JsVariant::Integer32(x), JsVariant::Integer32(y)) =
            (lhs_val.variant(), rhs_val.variant())
        {
            if let Some(result) = x.checked_sub(y) {
                context.vm.set_register(dst.into(), JsValue::new(result));
                return;
            }
            let f = f64::from(x) - f64::from(y);
            context.vm.set_register(dst.into(), JsValue::new(f));
            deopt(context, Opcode::Sub);
            return;
        }

        deopt(context, Opcode::Sub);
    }
}

impl Operation for SubInt {
    const NAME: &'static str = "SubInt";
    const INSTRUCTION: &'static str = "INST - SubInt";
    const COST: u8 = 1;
}

// ---------------------------------------------------------------------------
// SubF64
// ---------------------------------------------------------------------------

/// Quickened `-` for numeric operands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SubF64;

impl SubF64 {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) {
        let lhs_val = context.vm.get_register(lhs.into());
        let rhs_val = context.vm.get_register(rhs.into());

        let lx = match lhs_val.variant() {
            JsVariant::Integer32(i) => f64::from(i),
            JsVariant::Float64(f) => f,
            _ => {
                deopt(context, Opcode::Sub);
                return;
            }
        };
        let ry = match rhs_val.variant() {
            JsVariant::Integer32(i) => f64::from(i),
            JsVariant::Float64(f) => f,
            _ => {
                deopt(context, Opcode::Sub);
                return;
            }
        };

        context.vm.set_register(dst.into(), JsValue::new(lx - ry));
    }
}

impl Operation for SubF64 {
    const NAME: &'static str = "SubF64";
    const INSTRUCTION: &'static str = "INST - SubF64";
    const COST: u8 = 1;
}

// ---------------------------------------------------------------------------
// MulInt
// ---------------------------------------------------------------------------

/// Quickened `*` for i32 × i32 operands.
///
/// Handles `-0` semantics: `x * y = 0` where either `x < 0` or `y < 0`
/// produces `-0` (stored as `f64`), not `0` (i32).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MulInt;

impl MulInt {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) {
        let lhs_val = context.vm.get_register(lhs.into());
        let rhs_val = context.vm.get_register(rhs.into());

        if let (JsVariant::Integer32(x), JsVariant::Integer32(y)) =
            (lhs_val.variant(), rhs_val.variant())
        {
            if let Some(result) = x.checked_mul(y) {
                // Handle `-0`: JS `0 * -n = -0`.
                if result == 0 && (x < 0 || y < 0) {
                    context.vm.set_register(dst.into(), JsValue::new(-0.0_f64));
                    return;
                }
                context.vm.set_register(dst.into(), JsValue::new(result));
                return;
            }
            let f = f64::from(x) * f64::from(y);
            context.vm.set_register(dst.into(), JsValue::new(f));
            deopt(context, Opcode::Mul);
            return;
        }

        deopt(context, Opcode::Mul);
    }
}

impl Operation for MulInt {
    const NAME: &'static str = "MulInt";
    const INSTRUCTION: &'static str = "INST - MulInt";
    const COST: u8 = 1;
}

// ---------------------------------------------------------------------------
// MulF64
// ---------------------------------------------------------------------------

/// Quickened `*` for numeric operands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MulF64;

impl MulF64 {
    #[inline]
    pub(crate) fn operation(
        (dst, lhs, rhs): (RegisterOperand, RegisterOperand, RegisterOperand),
        context: &mut Context,
    ) {
        let lhs_val = context.vm.get_register(lhs.into());
        let rhs_val = context.vm.get_register(rhs.into());

        let lx = match lhs_val.variant() {
            JsVariant::Integer32(i) => f64::from(i),
            JsVariant::Float64(f) => f,
            _ => {
                deopt(context, Opcode::Mul);
                return;
            }
        };
        let ry = match rhs_val.variant() {
            JsVariant::Integer32(i) => f64::from(i),
            JsVariant::Float64(f) => f,
            _ => {
                deopt(context, Opcode::Mul);
                return;
            }
        };

        context.vm.set_register(dst.into(), JsValue::new(lx * ry));
    }
}

impl Operation for MulF64 {
    const NAME: &'static str = "MulF64";
    const INSTRUCTION: &'static str = "INST - MulF64";
    const COST: u8 = 1;
}
