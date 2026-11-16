// Runs the reference match in the WebAssembly build and compares its hash
// with the value pinned by the native tests. Exits non-zero on a mismatch.
//
// Build first, from the `sim/` folder:
//   cargo build -p wasm-check --target wasm32-unknown-unknown --release
// then:
//   node crates/wasm-check/check.mjs

import { readFile } from "node:fs/promises";

const wasmPath = new URL(
  "../../target/wasm32-unknown-unknown/release/wasm_check.wasm",
  import.meta.url,
);

const { instance } = await WebAssembly.instantiate(await readFile(wasmPath));
const { reference_match_hash, expected_reference_match_hash } = instance.exports;

// u64 values come back as BigInt; mask to read them as unsigned.
const hex = (n) => "0x" + BigInt.asUintN(64, n).toString(16).padStart(16, "0");
const actual = hex(reference_match_hash());
const expected = hex(expected_reference_match_hash());

if (actual === expected) {
  console.log(`WASM matches native: ${actual}`);
} else {
  console.error(`MISMATCH: WASM ${actual}, native ${expected}`);
  process.exit(1);
}
