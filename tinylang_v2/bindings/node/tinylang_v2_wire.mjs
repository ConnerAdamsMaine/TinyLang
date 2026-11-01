/** Dependency-free TinyLang v2 wire codec for Node.js ArrayBuffer callers. */

export const HEADER_BYTES = 32;
export const VALUE_HEADER_BYTES = 16;
export const MAX_FRAME_BYTES = 64 * 1024 * 1024;
export const FLAG_RESPONSE = 1;

export const OPCODE_NOP = 0x0000;
export const OPCODE_ABI_INFO = 0x0001;
export const OPCODE_ECHO = 0x0002;
export const OPCODE_PROGRAM_LOAD = 0x1000;
export const OPCODE_VM_CREATE = 0x1001;
export const OPCODE_VM_INVOKE = 0x1002;
export const OPCODE_HANDLE_RELEASE = 0x1003;

export const VALUE_U32 = 4;
export const VALUE_U64 = 5;
export const VALUE_BYTES = 12;
export const VALUE_HANDLE = 14;

export const ARTIFACT_FORMAT_TLBC = 1;
export const ARTIFACT_FORMAT_COMPILER_BINARY = 2;

const U16_MAX = 0xffff;
const U32_MAX = 0xffff_ffff;
const U64_MAX = 0xffff_ffff_ffff_ffffn;

function bytesOf(value) {
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  throw new TypeError("expected an ArrayBuffer or typed-array view");
}

function checkedU64(value, name) {
  if (typeof value === "number" && !Number.isSafeInteger(value)) {
    throw new TypeError(`${name} must be a safe integer or BigInt`);
  }
  const integer = typeof value === "bigint" ? value : BigInt(value);
  if (integer < 0n || integer > U64_MAX) throw new RangeError(`${name} does not fit u64`);
  return integer;
}

function concatenate(parts) {
  const views = parts.map(bytesOf);
  const length = views.reduce((total, view) => total + view.byteLength, 0);
  const output = new Uint8Array(length);
  let offset = 0;
  for (const view of views) {
    output.set(view, offset);
    offset += view.byteLength;
  }
  return output;
}

export function valueRecord(tag, payload = new Uint8Array(), auxiliary = 0n) {
  if (!Number.isInteger(tag) || tag < 0 || tag > U16_MAX) {
    throw new RangeError("value tag does not fit u16");
  }
  const body = bytesOf(payload);
  if (body.byteLength > U32_MAX) throw new RangeError("value payload does not fit u32");
  const paddedLength = (VALUE_HEADER_BYTES + body.byteLength + 7) & ~7;
  const output = new Uint8Array(paddedLength);
  const view = new DataView(output.buffer);
  view.setUint16(0, tag, true);
  view.setUint32(4, body.byteLength, true);
  view.setBigUint64(8, checkedU64(auxiliary, "auxiliary"), true);
  output.set(body, VALUE_HEADER_BYTES);
  return output.buffer;
}

export function u32Value(value) {
  if (!Number.isInteger(value) || value < 0 || value > U32_MAX) {
    throw new RangeError("value does not fit u32");
  }
  const payload = new ArrayBuffer(4);
  new DataView(payload).setUint32(0, value, true);
  return valueRecord(VALUE_U32, payload);
}

export function u64Value(value) {
  const payload = new ArrayBuffer(8);
  new DataView(payload).setBigUint64(0, checkedU64(value, "value"), true);
  return valueRecord(VALUE_U64, payload);
}

export function handleValue(handle) {
  return valueRecord(VALUE_HANDLE, new Uint8Array(), checkedU64(handle, "handle"));
}

export function bytesValue(value) {
  return valueRecord(VALUE_BYTES, value);
}

export function makeRequest(opcode, payload = new Uint8Array(), correlationId = 0n) {
  if (!Number.isInteger(opcode) || opcode < 0 || opcode > U16_MAX) {
    throw new RangeError("opcode does not fit u16");
  }
  const body = bytesOf(payload);
  if (body.byteLength > U32_MAX || HEADER_BYTES + body.byteLength > MAX_FRAME_BYTES) {
    throw new RangeError("payload exceeds the v2 frame limit");
  }
  const output = new Uint8Array(HEADER_BYTES + body.byteLength);
  output.set([0x54, 0x4c, 0x56, 0x32]); // TLV2
  const view = new DataView(output.buffer);
  view.setUint16(4, 2, true);
  view.setUint16(6, 0, true);
  view.setUint16(8, HEADER_BYTES, true);
  view.setUint16(10, opcode, true);
  view.setUint32(20, body.byteLength, true);
  view.setBigUint64(24, checkedU64(correlationId, "correlationId"), true);
  output.set(body, HEADER_BYTES);
  return output.buffer;
}

export function makeVmCreateRequest(
  program,
  maxHeapBytes,
  maxHeapObjects,
  maxCallDepth,
  flags = 0,
  correlationId = 0n,
) {
  const payload = concatenate([
    handleValue(program),
    u64Value(maxHeapBytes),
    u32Value(maxHeapObjects),
    u32Value(maxCallDepth),
    u32Value(flags),
  ]);
  return makeRequest(OPCODE_VM_CREATE, payload, correlationId);
}

export function programLoadPayload(artifact, artifactFormat = ARTIFACT_FORMAT_TLBC) {
  if (
    artifactFormat !== ARTIFACT_FORMAT_TLBC &&
    artifactFormat !== ARTIFACT_FORMAT_COMPILER_BINARY
  ) {
    throw new RangeError("artifactFormat must be 1 (TLBC) or 2 (compiler binary)");
  }
  const body = bytesOf(artifact);
  const limit = artifactFormat === ARTIFACT_FORMAT_TLBC ? 16 * 1024 * 1024 : 8 * 1024 * 1024;
  if (body.byteLength === 0 || body.byteLength > limit) {
    throw new RangeError(`artifact length must be between 1 and ${limit} bytes`);
  }
  return concatenate([u32Value(artifactFormat), bytesValue(body)]).buffer;
}

export function makeProgramLoadRequest(
  artifact,
  correlationId = 0n,
  artifactFormat = ARTIFACT_FORMAT_TLBC,
) {
  const payload = programLoadPayload(artifact, artifactFormat);
  return makeRequest(OPCODE_PROGRAM_LOAD, payload, correlationId);
}

export function makeHandleReleaseRequest(handle, correlationId = 0n) {
  return makeRequest(OPCODE_HANDLE_RELEASE, handleValue(handle), correlationId);
}

export function parseFrame(frame) {
  const bytes = bytesOf(frame);
  if (bytes.byteLength < HEADER_BYTES || bytes.byteLength > MAX_FRAME_BYTES) {
    throw new RangeError("frame length is outside the v2 bounds");
  }
  if (bytes[0] !== 0x54 || bytes[1] !== 0x4c || bytes[2] !== 0x56 || bytes[3] !== 0x32) {
    throw new TypeError("frame magic is not TLV2");
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint16(4, true) !== 2 || view.getUint16(6, true) !== 0) {
    throw new RangeError("unsupported TinyLang wire version");
  }
  if (view.getUint16(8, true) !== HEADER_BYTES) {
    throw new TypeError("noncanonical header length");
  }
  const payloadLength = view.getUint32(20, true);
  if (HEADER_BYTES + payloadLength !== bytes.byteLength) {
    throw new TypeError("payload length does not match the frame length");
  }
  const flags = view.getUint32(12, true);
  const status = view.getUint32(16, true);
  if (flags !== 0 && flags !== FLAG_RESPONSE) {
    throw new TypeError("noncanonical frame flags");
  }
  if (flags === 0 && status !== 0) {
    throw new TypeError("request status must be zero");
  }
  return Object.freeze({
    opcode: view.getUint16(10, true),
    flags,
    status,
    payloadLength,
    correlationId: view.getBigUint64(24, true),
    payload: bytes.slice(HEADER_BYTES).buffer,
  });
}

export function parseHandleResponse(frame) {
  const bytes = bytesOf(frame);
  const parsed = parseFrame(bytes);
  if (
    parsed.flags !== FLAG_RESPONSE ||
    parsed.status !== 0 ||
    parsed.payloadLength !== VALUE_HEADER_BYTES
  ) {
    throw new TypeError("response is not a successful single-handle response");
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const offset = HEADER_BYTES;
  const handle = view.getBigUint64(offset + 8, true);
  if (
    view.getUint16(offset, true) !== VALUE_HANDLE ||
    view.getUint16(offset + 2, true) !== 0 ||
    view.getUint32(offset + 4, true) !== 0 ||
    handle === 0n
  ) {
    throw new TypeError("response payload is not a canonical Handle value");
  }
  return handle;
}
