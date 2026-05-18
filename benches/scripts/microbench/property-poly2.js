// 2-shape polymorphic access. Both shapes have x, y, z but with extra props
// inserted in different positions to force distinct shapes.

const N = 100_000;
const a = { x: 1, y: 2, z: 3 };
const b = { extra: 0, x: 10, y: 20, z: 30 };

function read(o) {
  return o.x + o.y + o.z;
}

function main() {
  let sum = 0;
  for (let i = 0; i < N; i++) {
    sum += read(a);
    sum += read(b);
  }
  return sum;
}
