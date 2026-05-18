// Increment a global var in a tight loop. Tests global variable access
// path (which differs from local variables; goes through environment
// scope lookup unless cached).

var counter = 0;
const N = 200_000;

function bump() {
  counter = counter + 1;
}

function main() {
  counter = 0;
  for (let i = 0; i < N; i++) {
    bump();
  }
  return counter;
}
