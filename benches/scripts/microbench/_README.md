# Boa Microbenchmark Suite

Targeted microbenchmarks for measuring specific engine hot-paths in isolation.
Each `.js` file defines a `main()` function that the harness runs many times.

## Running with Boa (Criterion)

```
cargo bench --bench scripts -- microbench
```

Each script is run as a Criterion benchmark group named after its path.

## Running with reference engines (node / bun)

```
./tools/bench-compare.sh                  # all microbenches, all engines
./tools/bench-compare.sh property-access  # filter by name
```

The comparison tool runs the same JS in node/bun using a wrapper that
loops `main()` and reports elapsed time. Numbers are then printed
side-by-side.

## Design rules

- The work in `main()` should dominate over the loop overhead (target ≥1ms per
  call so timing noise is small).
- No `console.log` or other I/O inside `main()`.
- Return a value to defeat dead-code elimination in JIT engines.
- Setup (allocations, etc.) belongs OUTSIDE `main()` so it isn't part of the
  measurement.
- Files starting with `_` are ignored by the harness.
