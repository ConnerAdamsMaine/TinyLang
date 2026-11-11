import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import * as sdk from "./tinylang_v2_wire.mjs";

test("default format1 PROGRAM_LOAD remains byte-for-byte compatible", () => {
  const request = sdk.makeProgramLoadRequest(
    Uint8Array.of(0xaa, 0xbb, 0xcc),
    0x0102_0304_0506_0708n,
  );
  const expected = Buffer.from(
    "544c563202000000200000100000000000000000300000000807060504030201" +
      "040000000400000000000000000000000100000000000000" +
      "0c000000030000000000000000000000aabbcc0000000000",
    "hex",
  );
  assert.deepEqual(Buffer.from(request), expected);
});

test("explicit format2 PROGRAM_LOAD has canonical lengths and zero padding", () => {
  const request = sdk.makeProgramLoadRequest(Uint8Array.of(0xaa, 0xbb, 0xcc), 9n, 2);
  const frame = sdk.parseFrame(request);
  assert.equal(frame.opcode, sdk.OPCODE_PROGRAM_LOAD);
  assert.equal(frame.correlationId, 9n);
  assert.equal(frame.payloadLength, 48);
  assert.deepEqual(
    Buffer.from(frame.payload),
    Buffer.from(
      "040000000400000000000000000000000200000000000000" +
        "0c000000030000000000000000000000aabbcc0000000000",
      "hex",
    ),
  );
});

test("payload helper accepts named formats and agrees with request builder", () => {
  for (const format of [sdk.ARTIFACT_FORMAT_TLBC, sdk.ARTIFACT_FORMAT_COMPILER_BINARY]) {
    const artifact = Buffer.from("artifact");
    const payload = sdk.programLoadPayload(artifact, format);
    assert.equal(new DataView(payload).getUint32(16, true), format);
    assert.deepEqual(
      Buffer.from(sdk.parseFrame(sdk.makeProgramLoadRequest(artifact, 0n, format)).payload),
      Buffer.from(payload),
    );
  }
  assert.deepEqual(
    sdk.programLoadPayload(Buffer.from("artifact")),
    sdk.programLoadPayload(Buffer.from("artifact"), 1),
  );
});

test("rejects invalid artifact format options without coercion", () => {
  for (const format of [0, 3, -1, 2 ** 32, 1.5, 2n, true, false, "2", null, {}]) {
    assert.throws(
      () => sdk.makeProgramLoadRequest(Buffer.from("artifact"), 0n, format),
      /artifactFormat/,
    );
  }
});

test("rejects empty and over-limit artifacts in either format", () => {
  for (const [format, limit] of [[1, 16 * 1024 * 1024], [2, 8 * 1024 * 1024]]) {
    for (const artifact of [Buffer.alloc(0), Buffer.alloc(limit + 1)]) {
      assert.throws(() => sdk.makeProgramLoadRequest(artifact, 0n, format), /artifact.*length/);
    }
  }
});

test("limit-sized artifacts and offset Buffer views preserve exact bytes", () => {
  for (const [format, limit] of [[1, 16 * 1024 * 1024], [2, 8 * 1024 * 1024]]) {
    const artifact = Buffer.alloc(limit);
    const request = sdk.makeProgramLoadRequest(artifact, 0n, format);
    assert.equal(request.byteLength, 72 + limit);
    assert.equal(new DataView(request).getUint32(60, true), limit);
    assert.deepEqual(Buffer.from(request).subarray(72), artifact);
  }
  const storage = Buffer.from("prefixABCtail");
  const request = sdk.makeProgramLoadRequest(storage.subarray(6, 9), 0n, 2);
  assert.equal(new DataView(request).getUint32(60, true), 3);
  assert.deepEqual(Buffer.from(request).subarray(72), Buffer.from("4142430000000000", "hex"));
});

test("compiler-produced version-5 artifact survives Node format2 encoding", {
  skip: process.env.TINYLANG_V2_COMPILER_ARTIFACT === undefined
    ? "set TINYLANG_V2_COMPILER_ARTIFACT to the sdk_artifact example output"
    : false,
}, () => {
  const artifact = readFileSync(process.env.TINYLANG_V2_COMPILER_ARTIFACT);
  assert.equal(artifact.subarray(0, 8).toString("ascii"), "TINYLANG");
  assert.equal(artifact.readUInt16LE(8), 5);
  const encoded = sdk.makeProgramLoadRequest(
    artifact,
    0xfedc_ba98_7654_3210n,
    sdk.ARTIFACT_FORMAT_COMPILER_BINARY,
  );
  const frame = sdk.parseFrame(encoded);
  const records = Buffer.from(frame.payload);
  assert.equal(records.readUInt32LE(16), 2);
  assert.equal(records.readUInt32LE(28), artifact.length);
  assert.deepEqual(records.subarray(40, 40 + artifact.length), artifact);
  assert.deepEqual(records.subarray(40 + artifact.length), Buffer.alloc((-artifact.length) & 7));
  assert.equal(frame.correlationId, 0xfedc_ba98_7654_3210n);
  console.log(`NODE compiler artifact: version=5 bytes=${artifact.length} format=2 codec=OK native-FFI=UNAVAILABLE`);
});
