// Monomorphic method call. Tests the IC for property load (to get the
// function) + the call dispatch itself.

const N = 200_000;

class Counter {
  constructor() {
    this.n = 0;
  }
  inc(x) {
    this.n = this.n + x;
    return this.n;
  }
}

const c = new Counter();

function main() {
  let last = 0;
  for (let i = 0; i < N; i++) {
    last = c.inc(1);
  }
  return last;
}
