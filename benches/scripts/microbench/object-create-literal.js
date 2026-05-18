// Object construction via literal. Tests:
//  - allocation
//  - shape transition cache hits (same shape every time)
//  - the bytecode opcode(s) that build object literals

const N = 100_000;

function main() {
  let last;
  for (let i = 0; i < N; i++) {
    last = { x: i, y: i + 1, z: i + 2 };
  }
  return last.x + last.y + last.z;
}
