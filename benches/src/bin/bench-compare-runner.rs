//! Boa-side counterpart to runner.mjs.
//!
//! Loads a JS script, evaluates it once, then calls `main()` `runs` times
//! after `warmup` warmup runs, and prints `elapsed_ns=<N>` on stdout.
//!
//! This mirrors the timing protocol in `tools/bench-compare/runner.mjs` so
//! Boa and node/bun numbers are directly comparable.

#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::{env, fs, path::Path, process, time::Instant};

use boa_engine::{
    Context, JsValue, Source, js_string, optimizer::OptimizerOptions, script::Script,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: runner-boa <script.js> [runs] [warmup]");
        process::exit(2);
    }

    let script_path = &args[1];
    let runs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100);
    let warmup: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);

    let code = fs::read_to_string(Path::new(script_path)).expect("read script");

    let context = &mut Context::default();
    context.set_optimizer_options(OptimizerOptions::empty());
    boa_runtime::register(
        boa_runtime::extensions::ConsoleExtension(boa_runtime::NullLogger),
        None,
        context,
    )
    .expect("register runtime");

    let script = Script::parse(Source::from_bytes(&code), None, context).unwrap();
    script.codeblock(context).unwrap();
    script.evaluate(context).unwrap();

    let function = context
        .global_object()
        .get(js_string!("main"), context)
        .unwrap_or_else(|_| panic!("no main in {script_path}"))
        .as_callable()
        .unwrap_or_else(|| panic!("main is not callable in {script_path}"))
        .clone();

    // Warmup.
    for _ in 0..warmup {
        function.call(&JsValue::undefined(), &[], context).unwrap();
    }

    let start = Instant::now();
    let mut acc: i32 = 0;
    for _ in 0..runs {
        let v = function.call(&JsValue::undefined(), &[], context).unwrap();
        // Mix in some bit of the result to defeat dead-code elimination
        // (mostly belt-and-suspenders; Boa doesn't have a JIT yet).
        acc ^= v.to_i32(context).unwrap_or(0);
    }
    let elapsed_ns = start.elapsed().as_nanos();

    println!(
        "elapsed_ns={elapsed_ns} runs={runs} ns_per_run={} acc={acc}",
        elapsed_ns / runs as u128
    );
}
