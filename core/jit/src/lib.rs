//! Cranelift-based JIT tier for Boa.
//!
//! This crate is a **spike** (stage 0 of `planning/js-performance-roadmap/09-cranelift-jit.md`):
//! its only job right now is to prove that Cranelift compiles and runs native
//! code inside Boa's build on the target platform, and to validate the
//! calling-convention approach for reaching VM state.
//!
//! Nothing here is wired into `boa_engine` yet.

#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

/// Errors that can occur while building the JIT backend.
#[derive(Debug)]
pub enum JitError {
    /// Failed to build the target ISA (unsupported platform, bad flags).
    Isa(String),
}

/// Build a fresh JIT module configured for the host machine.
fn host_module() -> Result<JITModule, JitError> {
    let mut flags = settings::builder();
    // PIC off is fine for a process-local JIT; colocated libcalls off keeps
    // the relocation model simple for the spike.
    flags
        .set("use_colocated_libcalls", "false")
        .map_err(|e| JitError::Isa(e.to_string()))?;
    flags
        .set("is_pic", "false")
        .map_err(|e| JitError::Isa(e.to_string()))?;
    let isa_builder = cranelift_native::builder().map_err(|e| JitError::Isa(e.to_string()))?;
    let isa = isa_builder
        .finish(settings::Flags::new(flags))
        .map_err(|e| JitError::Isa(e.to_string()))?;
    let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    Ok(JITModule::new(builder))
}

/// Spike #1: JIT-compile `fn(i64, i64) -> i64 { a + b }` and return a callable
/// function pointer.
///
/// Proves: Cranelift IR construction, codegen for the host ISA, JIT finalize,
/// and that the produced machine code runs and returns the right value.
///
/// # Safety / lifetime
/// Leaks the [`JITModule`] (the code lives for the process). That is acceptable
/// for the spike; the real JIT will own modules per realm and free them on deopt.
#[must_use]
pub fn compile_add_i64() -> extern "C" fn(i64, i64) -> i64 {
    let mut module = host_module().expect("host should support Cranelift");
    let mut ctx = module.make_context();
    let mut fctx = FunctionBuilderContext::new();

    let int = types::I64;
    ctx.func.signature.params.push(AbiParam::new(int));
    ctx.func.signature.params.push(AbiParam::new(int));
    ctx.func.signature.returns.push(AbiParam::new(int));

    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let block = bcx.create_block();
        bcx.append_block_params_for_function_params(block);
        bcx.switch_to_block(block);
        bcx.seal_block(block);
        let a = bcx.block_params(block)[0];
        let b = bcx.block_params(block)[1];
        let sum = bcx.ins().iadd(a, b);
        bcx.ins().return_(&[sum]);
        bcx.finalize();
    }

    let id = module
        .declare_function("add_i64", Linkage::Export, &ctx.func.signature)
        .expect("declare");
    module.define_function(id, &mut ctx).expect("define");
    module.clear_context(&mut ctx);
    module.finalize_definitions().expect("finalize");

    let code = module.get_finalized_function(id);
    // Intentionally leak the module so `code` stays valid for the process.
    std::mem::forget(module);
    // SAFETY: signature matches the one we declared above.
    unsafe { std::mem::transmute::<*const u8, extern "C" fn(i64, i64) -> i64>(code) }
}

/// Spike #2: JIT-compile a function that takes an opaque `*mut u8` (stand-in for
/// `*mut Context`) and calls an `extern "C"` host helper with it, returning the
/// helper's result.
///
/// This validates the keystone of the real design: JIT code reaching VM state by
/// calling existing Rust functions with a threaded context pointer. The helper
/// address is baked in as a constant and called indirectly.
#[must_use]
pub fn compile_call_helper(
    helper: extern "C" fn(*mut u8) -> i64,
) -> extern "C" fn(*mut u8) -> i64 {
    let mut module = host_module().expect("host should support Cranelift");
    let ptr = module.target_config().pointer_type();
    let mut ctx = module.make_context();
    let mut fctx = FunctionBuilderContext::new();

    ctx.func.signature.params.push(AbiParam::new(ptr)); // ctx pointer
    ctx.func.signature.returns.push(AbiParam::new(types::I64));

    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let block = bcx.create_block();
        bcx.append_block_params_for_function_params(block);
        bcx.switch_to_block(block);
        bcx.seal_block(block);
        let ctx_arg = bcx.block_params(block)[0];

        // Signature of the helper we call indirectly.
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(ptr));
        sig.returns.push(AbiParam::new(types::I64));
        let sigref = bcx.import_signature(sig);

        // Bake the helper address in as a constant and call it indirectly.
        let helper_addr = bcx.ins().iconst(ptr, helper as usize as i64);
        let call = bcx.ins().call_indirect(sigref, helper_addr, &[ctx_arg]);
        let result = bcx.inst_results(call)[0];
        bcx.ins().return_(&[result]);
        bcx.finalize();
    }

    let id = module
        .declare_function("call_helper", Linkage::Export, &ctx.func.signature)
        .expect("declare");
    module.define_function(id, &mut ctx).expect("define");
    module.clear_context(&mut ctx);
    module.finalize_definitions().expect("finalize");

    let code = module.get_finalized_function(id);
    std::mem::forget(module);
    // SAFETY: signature matches.
    unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut u8) -> i64>(code) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_add_runs_native_code() {
        let add = compile_add_i64();
        assert_eq!(add(2, 3), 5);
        assert_eq!(add(-10, 4), -6);
        assert_eq!(add(i64::from(i32::MAX), 1), i64::from(i32::MAX) + 1);
    }

    extern "C" fn double_first_byte(p: *mut u8) -> i64 {
        // SAFETY: the test passes a valid pointer to a live `u8`.
        let v = unsafe { *p };
        i64::from(v) * 2
    }

    #[test]
    fn jit_calls_host_helper_with_context_pointer() {
        let f = compile_call_helper(double_first_byte);
        let mut state: u8 = 21;
        let out = f(std::ptr::from_mut(&mut state).cast());
        assert_eq!(out, 42);
    }
}
