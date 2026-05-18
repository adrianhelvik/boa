// Pure float arithmetic in a tight loop.
// Forces double-precision math; cannot be Smi-optimized.

const N = 500_000;

function main() {
  let acc = 1.0;
  for (let i = 0; i < N; i++) {
    acc = acc * 1.0000001 + 1.5;
    acc = acc / 1.0000002;
  }
  return acc;
}
