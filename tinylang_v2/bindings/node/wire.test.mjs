import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  FLAG_RESPONSE,
  HEADER_BYTES,
  OPCODE_NOP,
  OPCODE_PROGRAM_LOAD,
  OPCODE_VM_CREATE,
  handleValue,
  makeRequest,
  makeVmCreateRequest,
  parseFrame,
  parseHandleResponse,
} from "./tinylang_v2_wire.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = join(here, "..", "..", "tests", "vectors");

test("NOP ArrayBuffer exactly matches the committed Rust vector", () => {
  const encoded = makeRequest(OPCODE_NOP, new Uint8Array(), 0x0102_0304_0506_0708n);
  assert.deepEqual(Buffer.from(encoded), readFileSync(join(vectors, "valid_nop.bin")));
  const frame = parseFrame(encoded);
  assert.equal(frame.opcode, OPCODE_NOP);
  assert.equal(frame.flags, 0);
  assert.equal(frame.status, 0);
  assert.equal(frame.payloadLength, 0);
  assert.equal(frame.correlationId, 0x0102_0304_0506_0708n);
  assert.equal(encoded.byteLength, HEADER_BYTES);
});

test("VM_CREATE uses five canonical little-endian value records", () => {
  const encoded = makeVmCreateRequest(
    0x0000_0001_0000_0001n,
    1024n,
    32,
    8,
    0,
    0x5152_5354_5556_5758n,
  );
  assert.deepEqual(
    Buffer.from(encoded),
    readFileSync(join(vectors, "valid_vm_create_unknown_program.bin")),
  );
  const frame = parseFrame(encoded);
  assert.equal(frame.opcode, OPCODE_VM_CREATE);
  assert.equal(frame.payloadLength, 112);
});

test("parser rejects a mismatched payload length", () => {
  const encoded = new Uint8Array(makeRequest(OPCODE_NOP));
  new DataView(encoded.buffer).setUint32(20, 1, true);
  assert.throws(() => parseFrame(encoded), /payload length/);
});

test("u64 fields reject lossy JavaScript Numbers", () => {
  assert.throws(
    () => makeVmCreateRequest(Number.MAX_SAFE_INTEGER + 1, 1024n, 32, 8),
    /safe integer or BigInt/,
  );
});

test("parser rejects reserved frame flags", () => {
  for (const flags of [2, 3, 0x8000_0000]) {
    const encoded = makeRequest(OPCODE_NOP);
    new DataView(encoded).setUint32(12, flags, true);
    assert.throws(() => parseFrame(encoded), /flags/);
  }
});

test("parser rejects a nonzero request status but permits error responses", () => {
  const encoded = makeRequest(OPCODE_NOP);
  const view = new DataView(encoded);
  view.setUint32(16, 4, true);
  assert.throws(() => parseFrame(encoded), /status/);
  view.setUint32(12, FLAG_RESPONSE, true);
  assert.equal(parseFrame(encoded).status, 4);
});

test("handle response parser rejects a request containing a Handle", () => {
  const encoded = makeRequest(OPCODE_PROGRAM_LOAD, handleValue(1n));
  assert.throws(() => parseHandleResponse(encoded), /response/);
});

test("handle response parser preserves full u64 values in offset views", () => {
  const encoded = makeRequest(
    OPCODE_PROGRAM_LOAD,
    handleValue(0xfedc_ba98_7654_3210n),
  );
  new DataView(encoded).setUint32(12, FLAG_RESPONSE, true);
  const storage = new Uint8Array(encoded.byteLength + 16);
  storage.set(new Uint8Array(encoded), 8);
  assert.equal(
    parseHandleResponse(storage.subarray(8, 8 + encoded.byteLength)),
    0xfedc_ba98_7654_3210n,
  );
});
