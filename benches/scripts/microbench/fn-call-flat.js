// Call overhead: a tiny non-inlinable-via-cheating function called in a loop.
// V8 will inline the body if it can; we want to measure the call path so
// the function does just enough work that it shouldn't be optimised away.

const N = 500_000;

function tiny(x) {
  return x + 1;
}

function main() {
  let acc = 0;
  for (let i = 0; i < N; i++) {
    acc = tiny(acc);
  }
  return acc;
}
