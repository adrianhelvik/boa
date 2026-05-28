//! Experimental Cranelift-based JIT tier (work in progress).
//!
//! Staged plan: `planning/js-performance-roadmap/09-cranelift-jit.md`.
//!
//! Status: **integration milestone**. The standalone spike (`core/jit`) proved
//! Cranelift can emit native code and call a host helper through a threaded
//! opaque pointer. This module proves the next keystone: a JIT-compiled function
//! can call into `boa_engine` and drive a *real* [`Context`] / VM state — the
//! foundation the baseline call-threading compiler (JIT-1) builds on.
//!
//! Nothing here is on the interpreter's hot path yet; it is exercised only by
//! tests and is gated behind the `jit` feature.

use crate::Context;
use crate::vm::CodeBlock;
use crate::vm::opcode::{JIT_OP_SHIMS, Opcode};

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

/// High bit of a shim's `u64` return value: set means the op broke (a
/// `CompletionRecord` was stashed in `vm.jit_pending`); clear means continue,
/// with the low bits holding the new `frame.pc`.
pub(crate) const JIT_BREAK_BIT: u64 = 1 << 63;

/// A JIT backend bound to the host machine.
///
/// Owns the [`JITModule`]; dropping it frees the emitted code, so callers must
/// keep it alive for as long as any compiled function pointer is in use. The
/// real tier will hold one of these per realm.
pub struct JitBackend {
    module: JITModule,
}

impl std::fmt::Debug for JitBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitBackend").finish_non_exhaustive()
    }
}

impl JitBackend {
    /// Build a JIT backend configured for the host ISA.
    ///
    /// # Panics
    /// Panics if the host platform is not supported by Cranelift.
    #[must_use]
    pub fn new() -> Self {
        let mut flags = settings::builder();
        flags
            .set("use_colocated_libcalls", "false")
            .expect("valid flag");
        flags.set("is_pic", "false").expect("valid flag");
        let isa_builder = cranelift_native::builder().expect("host ISA must be supported");
        let isa = isa_builder
            .finish(settings::Flags::new(flags))
            .expect("valid ISA flags");
        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        Self {
            module: JITModule::new(builder),
        }
    }

    /// Compile a function `extern "C" fn(*mut Context) -> i64` whose body is a
    /// single indirect call to the given host `helper`, threading the context
    /// pointer through.
    ///
    /// This is the in-engine analogue of the spike's `compile_call_helper`, but
    /// the helper now operates on a real [`Context`]. It is the minimal proof
    /// that JIT-emitted native code can invoke `boa_engine` runtime routines —
    /// exactly how every lowered bytecode op will reach VM state in JIT-1.
    ///
    /// # Panics
    /// Panics if Cranelift codegen fails.
    #[must_use]
    pub fn compile_ctx_thunk(
        &mut self,
        helper: extern "C" fn(*mut Context) -> i64,
    ) -> extern "C" fn(*mut Context) -> i64 {
        let ptr = self.module.target_config().pointer_type();
        let mut ctx = self.module.make_context();
        let mut fctx = FunctionBuilderContext::new();

        ctx.func.signature.params.push(AbiParam::new(ptr));
        ctx.func.signature.returns.push(AbiParam::new(types::I64));

        {
            let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let block = bcx.create_block();
            bcx.append_block_params_for_function_params(block);
            bcx.switch_to_block(block);
            bcx.seal_block(block);
            let ctx_arg = bcx.block_params(block)[0];

            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(ptr));
            sig.returns.push(AbiParam::new(types::I64));
            let sigref = bcx.import_signature(sig);

            let helper_addr = bcx.ins().iconst(ptr, helper as usize as i64);
            let call = bcx.ins().call_indirect(sigref, helper_addr, &[ctx_arg]);
            let result = bcx.inst_results(call)[0];
            bcx.ins().return_(&[result]);
            bcx.finalize();
        }

        let id = self
            .module
            .declare_function("ctx_thunk", Linkage::Export, &ctx.func.signature)
            .expect("declare");
        self.module.define_function(id, &mut ctx).expect("define");
        self.module.clear_context(&mut ctx);
        self.module.finalize_definitions().expect("finalize");

        let code = self.module.get_finalized_function(id);
        // SAFETY: the compiled function matches this signature, and `self`
        // (which owns the code) outlives the returned pointer by contract.
        unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut Context) -> i64>(code) }
    }

    /// Compile a [`CodeBlock`] to native code using the **safe-by-construction
    /// baseline lowering** (see `planning/js-performance-roadmap/09-cranelift-jit.md`).
    ///
    /// The emitted `extern "C" fn(*mut Context) -> u64` runs the function's
    /// bytecode by calling each opcode's `extern "C"` shim in program order.
    /// After each op it inspects the returned status:
    /// - high bit set ([`JIT_BREAK_BIT`]) → the op broke; return it (the caller
    ///   reads `vm.jit_pending`);
    /// - else the low bits are the new `frame.pc`: if it equals the statically
    ///   known linear-next pc, fall through to the next op; otherwise a jump was
    ///   taken or a frame was pushed (`Call`/`New`), so **deopt** — return the
    ///   status; the caller resumes the interpreter from `frame.pc`.
    ///
    /// This needs no opcode classification and no CFG: any control flow falls
    /// back to the interpreter, so the result is correct for *every* `CodeBlock`
    /// (straight-line leaf code runs entirely in native code; everything else
    /// deopts cleanly).
    ///
    /// # Panics
    /// Panics if Cranelift codegen fails.
    #[must_use]
    pub fn compile_codeblock(&mut self, code: &CodeBlock) -> extern "C" fn(*mut Context) -> u64 {
        let ptr = self.module.target_config().pointer_type();

        // Walk the bytecode into (pc, opcode index, linear-next pc) triples.
        let bytes = &code.bytecode.bytes;
        let mut ops: Vec<(usize, usize, usize)> = Vec::new();
        let mut pc = 0usize;
        while pc < bytes.len() {
            let opcode = Opcode::decode(bytes[pc]);
            let (_instruction, next) = code.bytecode.next_instruction(pc);
            ops.push((pc, opcode as usize, next));
            pc = next;
        }

        let mut cctx = self.module.make_context();
        let mut fctx = FunctionBuilderContext::new();
        cctx.func.signature.params.push(AbiParam::new(ptr));
        cctx.func.signature.returns.push(AbiParam::new(types::I64));

        {
            let mut bcx = FunctionBuilder::new(&mut cctx.func, &mut fctx);

            // The shared shim signature: extern "C" fn(*mut Context, u32) -> u64.
            let mut shim_sig = self.module.make_signature();
            shim_sig.params.push(AbiParam::new(ptr));
            shim_sig.params.push(AbiParam::new(types::I32));
            shim_sig.returns.push(AbiParam::new(types::I64));
            let shim_sigref = bcx.import_signature(shim_sig);

            let entry = bcx.create_block();
            bcx.append_block_params_for_function_params(entry);
            let op_blocks: Vec<_> = ops.iter().map(|_| bcx.create_block()).collect();
            let break_block = bcx.create_block();
            let deopt_block = bcx.create_block();
            bcx.append_block_param(deopt_block, types::I64);

            bcx.switch_to_block(entry);
            let ctx_val = bcx.block_params(entry)[0];
            if let Some(first) = op_blocks.first() {
                bcx.ins().jump(*first, &[]);
            } else {
                let zero = bcx.ins().iconst(types::I64, 0);
                bcx.ins().jump(deopt_block, &[zero.into()]);
            }

            for (i, &(op_pc, op_idx, linear_next)) in ops.iter().enumerate() {
                bcx.switch_to_block(op_blocks[i]);

                // Bake the specific shim's address and call it directly.
                let shim_addr = JIT_OP_SHIMS[op_idx] as usize as i64;
                let shim_addr_val = bcx.ins().iconst(ptr, shim_addr);
                let pc_arg = bcx.ins().iconst(types::I32, op_pc as i64);
                let call =
                    bcx.ins()
                        .call_indirect(shim_sigref, shim_addr_val, &[ctx_val, pc_arg]);
                let status = bcx.inst_results(call)[0];

                // Break? (high bit set)
                #[allow(clippy::cast_possible_wrap)]
                let break_bit = bcx.ins().iconst(types::I64, JIT_BREAK_BIT as i64);
                let masked = bcx.ins().band(status, break_bit);
                let cont = bcx.create_block();
                bcx.ins().brif(masked, break_block, &[], cont, &[]);

                // Continue: did pc advance to the linear next?
                bcx.switch_to_block(cont);
                let lin = bcx.ins().iconst(types::I64, linear_next as i64);
                let is_linear = bcx.ins().icmp(IntCC::Equal, status, lin);
                if let Some(next_block) = op_blocks.get(i + 1) {
                    bcx.ins()
                        .brif(is_linear, *next_block, &[], deopt_block, &[status.into()]);
                } else {
                    // No more ops: a well-formed function broke before here; if we
                    // reach this, deopt to the interpreter at the reported pc.
                    bcx.ins().jump(deopt_block, &[status.into()]);
                }
            }

            // break_block → return the break sentinel.
            bcx.switch_to_block(break_block);
            #[allow(clippy::cast_possible_wrap)]
            let sentinel = bcx.ins().iconst(types::I64, JIT_BREAK_BIT as i64);
            bcx.ins().return_(&[sentinel]);

            // deopt_block → return the pc-carrying status.
            bcx.switch_to_block(deopt_block);
            let status = bcx.block_params(deopt_block)[0];
            bcx.ins().return_(&[status]);

            bcx.seal_all_blocks();
            bcx.finalize();
        }

        let id = self
            .module
            .declare_function("jit_codeblock", Linkage::Export, &cctx.func.signature)
            .expect("declare");
        self.module.define_function(id, &mut cctx).expect("define");
        self.module.clear_context(&mut cctx);
        self.module.finalize_definitions().expect("finalize");

        let code_ptr = self.module.get_finalized_function(id);
        // SAFETY: the compiled function matches this signature, and `self` owns
        // the code for as long as the returned pointer is used.
        unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut Context) -> u64>(code_ptr) }
    }
}

impl Default for JitBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsValue;

    /// A host helper that drives real VM state: it pushes a value onto the VM
    /// stack and returns a sentinel. Reaching `context.vm.stack` proves the
    /// JIT-threaded pointer is a usable, real `Context`.
    extern "C" fn probe_push(ctx: *mut Context) -> i64 {
        // SAFETY: the test passes a pointer to a live `Context` and does not
        // alias it for the duration of this call.
        let context = unsafe { &mut *ctx };
        context.vm.stack.push(JsValue::new(7i32));
        42
    }

    #[test]
    fn jit_compiles_real_codeblock() {
        // Lower a real function's bytecode end-to-end. Reaching the end without
        // panicking proves the safe baseline compiler handles real opcode shapes
        // (operands, control flow, calls) — control flow simply lowers to deopt
        // edges. This does not execute the code (that needs frame setup / tiering,
        // the next step); it exercises the bytecode → Cranelift lowering.
        let mut context = Context::default();
        let src = "function add(a, b) { return a + b; }";
        let script = crate::Script::parse(
            crate::Source::from_bytes(src),
            None,
            &mut context,
        )
        .expect("parse");
        let code = script.codeblock(&mut context).expect("codeblock");
        let mut backend = JitBackend::new();
        let _compiled = backend.compile_codeblock(&code);
    }

    #[test]
    fn jit_drives_real_context() {
        let mut context = Context::default();
        let mut backend = JitBackend::new();
        let thunk = backend.compile_ctx_thunk(probe_push);

        // Run the JIT-compiled native code against the real Context.
        let reported = thunk(std::ptr::from_mut(&mut context));

        // The JIT'd code called our helper (returns the sentinel)...
        assert_eq!(reported, 42);
        // ...and the helper mutated the real VM stack (value is observable).
        let top = context.vm.stack.pop();
        assert_eq!(top.as_i32(), Some(7));
    }
}
