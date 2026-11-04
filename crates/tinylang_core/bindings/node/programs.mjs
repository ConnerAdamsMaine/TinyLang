/** Load simple TLBC programs through the v2 ABI from a JavaScript host. */

import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  makeHandleReleaseRequest,
  makeProgramLoadRequest,
  makeVmCreateRequest,
  parseFrame,
  parseHandleResponse,
} from "./tinylang_v2_wire.mjs";

class ExactReader {
  constructor(stream) {
    this.buffer = Buffer.alloc(0);
    this.pending = [];
    this.ended = false;
    stream.on("data", (chunk) => {
      this.buffer = Buffer.concat([this.buffer, chunk]);
      this.drain();
    });
    stream.on("end", () => {
      this.ended = true;
      this.drain();
    });
  }

  read(length) {
    return new Promise((resolveRead, rejectRead) => {
      this.pending.push({ length, resolveRead, rejectRead });
      this.drain();
    });
  }

  drain() {
    while (this.pending.length > 0) {
      const request = this.pending[0];
      if (this.buffer.length >= request.length) {
        this.pending.shift();
        const value = this.buffer.subarray(0, request.length);
        this.buffer = this.buffer.subarray(request.length);
        request.resolveRead(value);
      } else if (this.ended) {
        this.pending.shift();
        request.rejectRead(new Error("ABI bridge closed with an incomplete packet"));
      } else {
        break;
      }
    }
  }
}

class AbiBridge {
  constructor(executable) {
    this.child = spawn(executable, [], { stdio: ["pipe", "pipe", "pipe"] });
    this.reader = new ExactReader(this.child.stdout);
    this.stderr = "";
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (value) => {
      this.stderr += value;
    });
    this.exit = new Promise((resolveExit) => this.child.once("exit", resolveExit));
  }

  async call(request, responseCapacity) {
    const bytes = Buffer.from(request);
    const packet = Buffer.alloc(8 + bytes.length);
    packet.writeUInt32LE(bytes.length, 0);
    packet.writeUInt32LE(responseCapacity, 4);
    bytes.copy(packet, 8);
    if (!this.child.stdin.write(packet)) await once(this.child.stdin, "drain");
    const header = await this.reader.read(16);
    const responseLength = header.readUInt32LE(12);
    return {
      callStatus: header.readUInt32LE(0),
      written: header.readBigUInt64LE(4),
      response: await this.reader.read(responseLength),
    };
  }

  async close() {
    this.child.stdin.end();
    const code = await this.exit;
    if (code !== 0) throw new Error(`ABI bridge exited ${code}: ${this.stderr}`);
  }
}

function checkedResponse(result, expectedLength) {
  if (result.callStatus !== 0 || result.written !== BigInt(expectedLength)) {
    throw new Error(`raw call failed: status=${result.callStatus} written=${result.written}`);
  }
  const frame = parseFrame(result.response);
  if (frame.status !== 0) throw new Error(`operation failed: response_status=${frame.status}`);
  return result.response;
}

async function runCase(bridge, path, correlation) {
  const artifact = readFileSync(path);
  const load = checkedResponse(
    await bridge.call(makeProgramLoadRequest(artifact, correlation), 48),
    48,
  );
  const program = parseHandleResponse(load);
  const create = checkedResponse(
    await bridge.call(makeVmCreateRequest(program, 1024n, 32, 8, 0, correlation + 1n), 48),
    48,
  );
  const vm = parseHandleResponse(create);
  checkedResponse(await bridge.call(makeHandleReleaseRequest(program, correlation + 2n), 48), 32);
  checkedResponse(await bridge.call(makeHandleReleaseRequest(vm, correlation + 3n), 48), 32);
  console.log(
    `JS-TL ${path.split(/[\\/]/).at(-1)}: load=OK vm_create=OK lifecycle=OK ` +
      "execute=UNAVAILABLE(no exported function)",
  );
}

const here = dirname(fileURLToPath(import.meta.url));
const crate = resolve(here, "..", "..");
const workspace = resolve(crate, "..", "..");
const suffix = process.platform === "win32" ? ".exe" : "";
const bridgePath = process.argv[2] ?? join(workspace, "target", "debug", "examples", `abi_stdio_bridge${suffix}`);
const suppliedArtifacts = process.argv.slice(3);
const artifacts =
  suppliedArtifacts.length > 0
    ? suppliedArtifacts
    : [
        join(crate, "tests", "artifacts", "minimal_halt.tlbc"),
        join(crate, "tests", "artifacts", "push_print_halt.tlbc"),
        join(crate, "tests", "artifacts", "arithmetic_print.tlbc"),
      ];

const bridge = new AbiBridge(bridgePath);
try {
  for (const [index, artifact] of artifacts.entries()) {
    await runCase(bridge, artifact, 0x3000n + BigInt(index * 4));
  }
} finally {
  await bridge.close();
}
