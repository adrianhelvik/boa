// Megamorphic access: 16 distinct shapes, beyond Boa's PIC_CAPACITY.
// IC should give up and fall through to slow lookup.

const N = 30_000;
const objs = [];
for (let i = 0; i < 16; i++) {
  const o = { x: i, y: i + 1, z: i + 2 };
  // Insert 'pad' before/after x to create distinct shape per object.
  for (let j = 0; j <= i; j++) {
    o["p" + j] = j;
  }
  objs.push(o);
}

function read(o) {
  return o.x + o.y + o.z;
}

function main() {
  let sum = 0;
  for (let i = 0; i < N; i++) {
    sum += read(objs[i & 15]);
  }
  return sum;
}
