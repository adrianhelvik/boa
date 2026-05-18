// Recursive Fibonacci. Tests call/return overhead and deep stacks.

function fib(n) {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

function main() {
  return fib(25);
}
