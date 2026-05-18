// Pure integer arithmetic in a tight loop.
// Tests dispatch overhead and Smi (int32) handling.
// Operands stay in i32 range to keep V8 in Smi territory.

const N = 1_000_000;

function main() {
  let acc = 0;
  for (let i = 0; i < N; i++) {
    acc = (acc + i) | 0;
    acc = (acc * 3) | 0;
    acc = (acc - 7) | 0;
  }
  return acc;
}
