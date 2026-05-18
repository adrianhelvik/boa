// Monomorphic property writes (existing slot, same shape every iteration).
// Tests SetPropertyByName IC effectiveness.

const N = 200_000;

function Point() {
  this.x = 0;
  this.y = 0;
  this.z = 0;
}

const p = new Point();

function main() {
  for (let i = 0; i < N; i++) {
    p.x = i;
    p.y = i + 1;
    p.z = i + 2;
  }
  return p.x + p.y + p.z;
}
