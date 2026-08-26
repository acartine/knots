#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const REPOSITORY = "acartine/knots";
const WORKFLOW = ".github/workflows/knots-v2-control-epoch.yml";
const sha = /^[0-9a-f]{40}$/;
const digest = /^[0-9a-f]{64}$/;
const writer = /^[0-9a-f]{64}$/;
const nonce = /^[0-9a-f]{32}$/;

function fail(message) {
  throw new Error(message);
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, canonical(value[key])]),
    );
  }
  return value;
}

function canonicalBytes(value) {
  return `${JSON.stringify(canonical(value))}\n`;
}

function parseJson(value, name) {
  try {
    return JSON.parse(value);
  } catch {
    fail(`${name} is not valid JSON`);
  }
}

function parseEpoch(value) {
  if (!/^[1-9][0-9]*$/.test(String(value))) fail("epoch must be positive");
  const epoch = Number(value);
  if (!Number.isSafeInteger(epoch)) fail("epoch exceeds the safe integer range");
  return epoch;
}

function validateVector(value) {
  if (!value || Array.isArray(value) || typeof value !== "object") {
    fail("writer vector must be an object");
  }
  const entries = Object.entries(value);
  if (entries.length === 0) fail("writer vector must not be empty");
  for (const [writerId, sequence] of entries) {
    if (!writer.test(writerId) || !Number.isSafeInteger(sequence) || sequence < 1) {
      fail("writer vector contains an invalid writer or sequence");
    }
  }
  return Object.fromEntries(entries.sort(([left], [right]) => left.localeCompare(right)));
}

function validateArchives(value) {
  if (!Array.isArray(value)) fail("archives must be an array");
  const archives = value.map((entry) => {
    if (!entry || typeof entry !== "object"
        || !/^refs\/heads\/knots-v2-archive\/[0-9a-f]{64}\/[0-9a-f]{32}$/.test(entry.ref)
        || !sha.test(entry.oid)) {
      fail("archive entry is invalid");
    }
    return { ref: entry.ref, oid: entry.oid };
  });
  return archives.sort((left, right) => left.ref.localeCompare(right.ref));
}

function validateGeneration(namespace, ref, oid) {
  const pattern = namespace === "production"
    ? /^refs\/heads\/knots-v2-canonical\/[0-9a-f]{64}\/[0-9a-f]{32}$/
    : /^refs\/heads\/knots-v2-canary\/immutable\/[a-z0-9-]+$/;
  if (!pattern.test(ref) || !sha.test(oid)) fail("generation ref or OID is invalid");
}

function dominates(next, previous) {
  let advanced = false;
  for (const [writerId, sequence] of Object.entries(previous)) {
    if (!(writerId in next) || next[writerId] < sequence) return false;
    if (next[writerId] > sequence) advanced = true;
  }
  for (const writerId of Object.keys(next)) {
    if (!(writerId in previous)) advanced = true;
  }
  return advanced;
}

function validatePrevious(previous, namespace, epoch, vector) {
  if (previous === null) {
    if (epoch !== 1) fail("the first authoritative epoch must be one");
    return;
  }
  validateManifest(previous);
  if (previous.namespace !== namespace || epoch !== previous.epoch + 1) {
    fail("epoch does not immediately follow the latest authority");
  }
  if (!dominates(vector, previous.writer_vector)) {
    fail("writer vector does not strictly dominate the latest authority");
  }
}

function buildManifest(input, previous) {
  if (input.repository !== REPOSITORY
      || input.workflow !== WORKFLOW
      || !/^refs\/heads\/knots-v2-authority-code\/[0-9a-f]{32}$/.test(input.ref)
      || !sha.test(input.sourceSha)
      || !nonce.test(input.nonce)
      || !/^[1-9][0-9]*$/.test(input.runId)) {
    fail("trusted workflow identity is invalid");
  }
  if (!["canary", "production"].includes(input.namespace)) fail("invalid namespace");
  const epoch = parseEpoch(input.epoch);
  const vector = validateVector(input.writerVector);
  const archives = validateArchives(input.archives);
  validateGeneration(input.namespace, input.generationRef, input.generationOid);
  validatePrevious(previous, input.namespace, epoch, vector);
  return {
    schema_version: 1,
    repository: REPOSITORY,
    namespace: input.namespace,
    epoch,
    workflow: WORKFLOW,
    source_ref: input.ref,
    source_sha: input.sourceSha,
    run_id: input.runId,
    nonce: input.nonce,
    generation: { ref: input.generationRef, oid: input.generationOid },
    archives,
    writer_vector: vector,
  };
}

function validateManifest(manifest) {
  if (!manifest || manifest.schema_version !== 1 || manifest.repository !== REPOSITORY
      || !["canary", "production"].includes(manifest.namespace)
      || manifest.workflow !== WORKFLOW
      || !/^refs\/heads\/knots-v2-authority-code\/[0-9a-f]{32}$/.test(manifest.source_ref)
      || !sha.test(manifest.source_sha) || !/^[1-9][0-9]*$/.test(manifest.run_id)
      || !nonce.test(manifest.nonce)) {
    fail("control epoch manifest identity is invalid");
  }
  parseEpoch(manifest.epoch);
  validateGeneration(manifest.namespace, manifest.generation.ref, manifest.generation.oid);
  validateArchives(manifest.archives);
  validateVector(manifest.writer_vector);
  return manifest;
}

function refPrefix(namespace) {
  return namespace === "production"
    ? "refs/heads/knots-v2-control/epochs"
    : "refs/heads/knots-v2-canary/control/epochs";
}

function prepare() {
  const previousState = parseJson(
    readFileSync(process.env.PREVIOUS_STATE_PATH, "utf8"),
    "previous state",
  );
  const manifest = buildManifest({
    repository: process.env.GITHUB_REPOSITORY,
    workflow: WORKFLOW,
    ref: process.env.GITHUB_REF,
    sourceSha: process.env.GITHUB_SHA,
    runId: process.env.GITHUB_RUN_ID,
    nonce: process.env.CONTROL_NONCE,
    namespace: process.env.NAMESPACE,
    epoch: process.env.EPOCH,
    generationRef: process.env.GENERATION_REF,
    generationOid: process.env.GENERATION_OID,
    archives: parseJson(process.env.ARCHIVES_JSON, "archives"),
    writerVector: parseJson(process.env.WRITER_VECTOR_JSON, "writer vector"),
  }, previousState.manifest ?? null);
  const bytes = canonicalBytes(manifest);
  const manifestDigest = createHash("sha256").update(bytes).digest("hex");
  const epoch = String(manifest.epoch).padStart(20, "0");
  const controlRef = `${refPrefix(manifest.namespace)}/${epoch}`
    + `/${manifest.run_id}-${manifest.nonce}-${manifestDigest}`;
  writeFileSync(process.env.MANIFEST_PATH, bytes, { mode: 0o600 });
  writeFileSync(process.env.GITHUB_OUTPUT,
    `control_ref=${controlRef}\nmanifest_sha256=${manifestDigest}\n`, { flag: "a" });
}

function verifyFile() {
  const path = process.argv[3];
  const manifest = validateManifest(parseJson(readFileSync(path, "utf8"), "manifest"));
  const bytes = canonicalBytes(manifest);
  if (readFileSync(path, "utf8") !== bytes) fail("manifest is not canonical JSON");
  const manifestDigest = createHash("sha256").update(bytes).digest("hex");
  const epoch = String(manifest.epoch).padStart(20, "0");
  const expected = `${refPrefix(manifest.namespace)}/${epoch}`
    + `/${manifest.run_id}-${manifest.nonce}-${manifestDigest}`;
  if (process.env.CONTROL_REF !== expected) fail("control ref does not bind the manifest");
  process.stdout.write(`${JSON.stringify(manifest)}\n`);
}

function verifyChain() {
  const entries = parseJson(readFileSync(process.argv[3], "utf8"), "authority chain");
  if (!Array.isArray(entries)) fail("authority chain must be an array");
  if (entries.length === 0) {
    process.stdout.write('{"control_ref":null,"control_oid":null,"manifest":null}\n');
    return;
  }
  entries.sort((left, right) => left.manifest.epoch - right.manifest.epoch);
  let previous = null;
  for (const entry of entries) {
    const manifest = validateManifest(entry.manifest);
    validatePrevious(previous, manifest.namespace, manifest.epoch, manifest.writer_vector);
    previous = manifest;
  }
  process.stdout.write(`${JSON.stringify(entries.at(-1))}\n`);
}

function selfTest() {
  const key = "a".repeat(64);
  const base = {
    repository: REPOSITORY, workflow: WORKFLOW,
    ref: `refs/heads/knots-v2-authority-code/${"e".repeat(32)}`,
    sourceSha: "b".repeat(40), runId: "1", namespace: "canary", epoch: "1",
    nonce: "d".repeat(32),
    generationRef: "refs/heads/knots-v2-canary/immutable/example",
    generationOid: "c".repeat(40), archives: [], writerVector: { [key]: 1 },
  };
  const first = buildManifest(base, null);
  const second = buildManifest({ ...base, runId: "2", epoch: "2",
    writerVector: { [key]: 2 } }, first);
  if (second.epoch !== 2) fail("self-test did not advance");
  for (const invalid of [
    { ...base, epoch: "2" },
    { ...base, runId: "2", epoch: "2", writerVector: { [key]: 1 }, previous: first },
  ]) {
    let rejected = false;
    try { buildManifest(invalid, invalid.previous ?? null); } catch { rejected = true; }
    if (!rejected) fail("self-test accepted an invalid epoch");
  }
  process.stdout.write("control epoch self-test passed\n");
}

try {
  const command = process.argv[2];
  if (command === "prepare") prepare();
  else if (command === "verify-file") verifyFile();
  else if (command === "verify-chain") verifyChain();
  else if (command === "self-test") selfTest();
  else fail("usage: control epoch command <prepare|verify-file|verify-chain|self-test>");
} catch (error) {
  process.stderr.write(`error: ${error.message}\n`);
  process.exit(1);
}
