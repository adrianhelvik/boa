// String concat via `+`. Each iteration grows the string by 1 char.
// Tests Boa's string representation: V8 uses cons strings for O(1) concat.

const N = 5_000;

function main() {
  let s = "";
  for (let i = 0; i < N; i++) {
    s = s + "x";
  }
  return s.length;
}
