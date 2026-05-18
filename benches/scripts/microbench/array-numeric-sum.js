// Sum a numeric array. Tests:
//  - dense array element access (no IC needed; index op)
//  - Smi arithmetic in the accumulator

const SIZE = 10_000;
const arr = new Array(SIZE);
for (let i = 0; i < SIZE; i++) arr[i] = i;

const RUNS = 100;

function main() {
  let total = 0;
  for (let r = 0; r < RUNS; r++) {
    let sum = 0;
    for (let i = 0; i < SIZE; i++) {
      sum = (sum + arr[i]) | 0;
    }
    total = (total ^ sum) | 0;
  }
  return total;
}
