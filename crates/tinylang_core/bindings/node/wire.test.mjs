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
  OPCODE_VM_INVOKE,
  OPCODE_VM_CREATE,
  VALUE_SEQUENCE,
  VALUE_UTF8,
  handleValue,
  makeRequest,
  makeVmInvokeRequest,
  makeVmCreateRequest,
  parseFrame,
  parseHandleResponse,
  parseUtf8Value,
  valueRecord,
  utf8Value,
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

test("UTF-8 values encode canonical UTF-8 bytes and zero padding", () => {
  const encoded = new Uint8Array(utf8Value("TinyLang λ 🦀"));
  const payload = new TextEncoder().encode("TinyLang λ 🦀");
  assert.equal(new DataView(encoded.buffer).getUint16(0, true), VALUE_UTF8);
  assert.equal(new DataView(encoded.buffer).getUint32(4, true), payload.byteLength);
  assert.deepEqual(encoded.subarray(16, 16 + payload.byteLength), payload);
  assert.ok(encoded.subarray(16 + payload.byteLength).every((byte) => byte === 0));
});

test("UTF-8 values reject non-strings and unpaired surrogates", () => {
  assert.throws(() => utf8Value(42), /string/);
  assert.throws(() => utf8Value("\ud800"), /Unicode scalar/);
});

test("UTF-8 value parser returns text and rejects noncanonical records", () => {
  const encoded = utf8Value("TinyLang λ 🦀");
  assert.equal(parseUtf8Value(encoded), "TinyLang λ 🦀");

  const wrongTag = new Uint8Array(encoded);
  new DataView(wrongTag.buffer).setUint16(0, 12, true);
  assert.throws(() => parseUtf8Value(wrongTag), /UTF-8/);
  const wrongPadding = new Uint8Array(utf8Value("a"));
  wrongPadding[wrongPadding.length - 1] = 1;
  assert.throws(() => parseUtf8Value(wrongPadding), /padding/);
  const invalidText = new Uint8Array(utf8Value("a"));
  invalidText[16] = 0xff;
  assert.throws(() => parseUtf8Value(invalidText), /UTF-8/);
  assert.throws(() => parseUtf8Value(new Uint8Array(utf8Value("a")).subarray(0, 17)), /length/);
});

test("UTF-8 value parser rejects truncated records with near-u32-maximum lengths", () => {
  for (const payloadLength of [0xffff_fff9, 0xffff_fffe, 0xffff_ffff]) {
    const record = new Uint8Array(16);
    const header = new DataView(record.buffer);
    header.setUint16(0, VALUE_UTF8, true);
    header.setUint32(4, payloadLength, true);
    assert.throws(() => parseUtf8Value(record), RangeError);
  }
});

test("VM_INVOKE builder emits the specified sequence request", () => {
  const argument = valueRecord(9, Uint8Array.of(42, 0, 0, 0, 0, 0, 0, 0));
  const encoded = makeVmInvokeRequest(0x0000_0001_0000_0001n, 3, 500, 128, [argument], 77n);
  const frame = parseFrame(encoded);
  assert.equal(frame.opcode, OPCODE_VM_INVOKE);
  assert.equal(frame.correlationId, 77n);
  const payload = new Uint8Array(frame.payload);
  assert.equal(new DataView(payload.buffer).getUint16(0, true), 14);
  assert.equal(new DataView(payload.buffer).getBigUint64(8, true), 0x0000_0001_0000_0001n);
  assert.equal(new DataView(payload.buffer).getUint32(32, true), 3);
  assert.equal(new DataView(payload.buffer).getBigUint64(56, true), 500n);
  assert.equal(new DataView(payload.buffer).getUint32(80, true), 128);
  assert.equal(new DataView(payload.buffer).getUint16(88, true), VALUE_SEQUENCE);
  assert.equal(new DataView(payload.buffer).getBigUint64(96, true), 1n);
  assert.deepEqual(payload.subarray(104), new Uint8Array(argument));
});

test("VM_INVOKE builder rejects invalid limits and arguments", () => {
  const args = [];
  assert.throws(() => makeVmInvokeRequest(0n, 0, 1, 24, args), /vm/);
  assert.throws(() => makeVmInvokeRequest(1n, -1, 1, 24, args), /exportId/);
  assert.throws(() => makeVmInvokeRequest(1n, 0, 0, 24, args), /fuel/);
  assert.throws(
    () => makeVmInvokeRequest(1n, 0, 1, 16 * 1024 * 1024 + 1, args),
    /maxResultBytes/,
  );
  assert.throws(() => makeVmInvokeRequest(1n, 0, 1, 24, {}), /array/);
});
