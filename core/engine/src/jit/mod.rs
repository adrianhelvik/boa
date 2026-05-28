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

use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

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
