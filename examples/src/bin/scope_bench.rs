/// Measures parse + scope-analysis time for scripts with N var declarations,
/// to confirm the O(n²) → O(n) fix produces linear scaling.
///
/// Run with: cargo run --release --example scope_bench
use std::time::{Duration, Instant};

use boa_engine::{Context, Source, script::Script};

fn generate_var_decls(n: usize) -> String {
    let decls: Vec<String> = (0..n).map(|i| format!("a{i}=0")).collect();
    format!("var {};", decls.join(","))
}

fn generate_fn_with_refs(n: usize) -> String {
    // A function with N local var declarations + N references (one per var).
    let decls: Vec<String> = (0..n).map(|i| format!("var v{i}={i}")).collect();
    let refs: Vec<String> = (0..n).map(|i| format!("x+=v{i}")).collect();
    format!(
        "var x=0; function f() {{ {}; {}; return x; }} f();",
        decls.join(";"),
        refs.join(";")
    )
}

/// Time N repetitions of parse only (no execution) for a source string.
fn bench_parse(ctx: &mut Context, source: &str, iters: u32) -> Duration {
    let src_bytes = source.as_bytes();
    let start = Instant::now();
    for _ in 0..iters {
        drop(Script::parse(Source::from_bytes(src_bytes), None, ctx));
    }
    start.elapsed() / iters
}

fn main() {
    let mut ctx = Context::default();

    println!("=== Var declarations only (N vars in global scope) ===");
    println!("{:<8} {:>12} {:>10}", "N", "avg_us", "ratio");
    let mut prev_us = None::<f64>;
    for &n in &[1000usize, 2000, 4000, 8000] {
        let src = generate_var_decls(n);
        let iters = if n <= 2000 { 200 } else { 50 };
        let avg = bench_parse(&mut ctx, &src, iters);
        let us = avg.as_micros() as f64;
        let ratio = prev_us.map_or("  -".to_string(), |p| format!("{:.2}x", us / p));
        println!("{:<8} {:>12.1} {:>10}", n, us, ratio);
        prev_us = Some(us);
    }

    println!();
    println!("=== Function with N locals + N references ===");
    println!("{:<8} {:>12} {:>10}", "N", "avg_us", "ratio");
    prev_us = None;
    for &n in &[500usize, 1000, 2000, 4000] {
        let src = generate_fn_with_refs(n);
        let iters = if n <= 1000 { 100 } else { 30 };
        let avg = bench_parse(&mut ctx, &src, iters);
        let us = avg.as_micros() as f64;
        let ratio = prev_us.map_or("  -".to_string(), |p| format!("{:.2}x", us / p));
        println!("{:<8} {:>12.1} {:>10}", n, us, ratio);
        prev_us = Some(us);
    }
}
