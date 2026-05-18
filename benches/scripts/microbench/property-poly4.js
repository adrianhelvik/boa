// 4-shape polymorphic — saturates Boa's PIC_CAPACITY = 4.

const N = 50_000;
const o1 = { x: 1, y: 2, z: 3 };
const o2 = { a: 0, x: 4, y: 5, z: 6 };
const o3 = { x: 7, b: 0, y: 8, z: 9 };
const o4 = { x: 10, y: 11, c: 0, z: 12 };
const arr = [o1, o2, o3, o4];

function read(o) {
  return o.x + o.y + o.z;
}

function main() {
  let sum = 0;
  for (let i = 0; i < N; i++) {
    sum += read(arr[i & 3]);
  }
  return sum;
}
