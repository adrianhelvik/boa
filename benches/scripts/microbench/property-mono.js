// Monomorphic property access: every call sees the same hidden class.
// This is the *best case* for inline caches — should be near-V8 speed
// if ICs are wired correctly.

const N = 200_000;
const obj = { x: 1, y: 2, z: 3 };

function read(o) {
  return o.x + o.y + o.z;
}

function main() {
  let sum = 0;
  for (let i = 0; i < N; i++) {
    sum += read(obj);
  }
  return sum;
}
