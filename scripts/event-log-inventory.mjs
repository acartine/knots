#!/usr/bin/env node

import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";

const STREAMS = Object.freeze(["events", "index", "snapshots", "manifests"]);
const STREAM_PATHS = Object.freeze({ manifests: "compaction" });

function positiveInteger(value, option) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isSafeInteger(parsed) || parsed < 1) {
    throw new Error(`${option} must be a positive integer`);
  }
  return parsed;
}

export function parseInventoryArgs(argv) {
  const options = { root: null, runs: 5 };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!["--root", "--runs"].includes(argument)) {
      throw new Error(`unknown argument: ${argument}`);
    }
    const value = argv[index + 1];
    if (value === undefined) throw new Error(`${argument} requires a value`);
    index += 1;
    options[argument.slice(2)] = argument === "--root" ? value : positiveInteger(value, argument);
  }
  if (!options.root) throw new Error("--root is required");
  return options;
}

async function walkJsonFiles(root) {
  const files = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    let entries;
    try {
      entries = await readdir(current, { withFileTypes: true });
    } catch (error) {
      if (error.code === "ENOENT") continue;
      throw error;
    }
    entries.sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      const destination = path.join(current, entry.name);
      if (entry.isDirectory()) stack.push(destination);
      if (entry.isFile() && path.extname(entry.name) === ".json") files.push(destination);
    }
  }
  files.sort();
  return files;
}

function dateKey(store, file) {
  const relative = path.relative(store, file).split(path.sep);
  if (relative.length < 5) return null;
  const date = relative.slice(1, 4).join("-");
  return /^\d{4}-\d{2}-\d{2}$/.test(date) ? date : null;
}

async function totalBytes(files) {
  const batchSize = 256;
  let bytes = 0;
  for (let start = 0; start < files.length; start += batchSize) {
    const batch = files.slice(start, start + batchSize);
    const sizes = await Promise.all(batch.map(async (file) => (await stat(file)).size));
    bytes += sizes.reduce((sum, size) => sum + size, 0);
  }
  return bytes;
}

async function collectStoreFiles(store) {
  const files = {};
  for (const stream of STREAMS) {
    files[stream] = await walkJsonFiles(path.join(store, STREAM_PATHS[stream] ?? stream));
  }
  return files;
}

async function scanStore(store) {
  const result = {};
  const dates = new Set();
  const collected = await collectStoreFiles(store);
  for (const stream of STREAMS) {
    const files = collected[stream];
    const bytes = await totalBytes(files);
    for (const file of files) {
      const date = dateKey(store, file);
      if (date && stream !== "snapshots") dates.add(date);
    }
    result[stream] = { bytes, files: files.length };
  }
  return { result, dates };
}

function relativeFiles(store, collected) {
  return STREAMS.flatMap((stream) =>
    collected[stream].map((file) => ({ absolute: file, relative: path.relative(store, file) })),
  );
}

async function buildByteMirror(store) {
  const files = relativeFiles(store, await collectStoreFiles(store));
  const mirror = new Map();
  const batchSize = 256;
  for (let start = 0; start < files.length; start += batchSize) {
    const batch = files.slice(start, start + batchSize);
    const contents = await Promise.all(batch.map((file) => readFile(file.absolute)));
    batch.forEach((file, index) => mirror.set(file.relative, contents[index]));
  }
  return mirror;
}

async function compareWithByteMirror(store, mirror) {
  const files = relativeFiles(store, await collectStoreFiles(store));
  if (files.length !== mirror.size) throw new Error("byte mirror file count changed");
  const batchSize = 256;
  for (let start = 0; start < files.length; start += batchSize) {
    const batch = files.slice(start, start + batchSize);
    const contents = await Promise.all(batch.map((file) => readFile(file.absolute)));
    batch.forEach((file, index) => {
      if (!mirror.get(file.relative)?.equals(contents[index])) {
        throw new Error(`byte mirror mismatch: ${file.relative}`);
      }
    });
  }
  return files.length;
}

function roundMilliseconds(value) {
  return Number(value.toFixed(3));
}

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  if (sorted.length % 2 === 1) return sorted[middle];
  return (sorted[middle - 1] + sorted[middle]) / 2;
}

export async function inventoryStore(options) {
  const store = path.join(path.resolve(options.root), ".knots");
  const scanTimings = [];
  let scan;
  for (let run = 0; run < options.runs; run += 1) {
    const started = performance.now();
    scan = await scanStore(store);
    scanTimings.push(performance.now() - started);
  }
  const mirror = await buildByteMirror(store);
  const syncTimings = [];
  let comparedFiles = 0;
  for (let run = 0; run < options.runs; run += 1) {
    const started = performance.now();
    comparedFiles = await compareWithByteMirror(store, mirror);
    syncTimings.push(performance.now() - started);
  }
  const counts = Object.fromEntries(STREAMS.map((name) => [name, scan.result[name].files]));
  const bytes = Object.fromEntries(STREAMS.map((name) => [name, scan.result[name].bytes]));
  const historicalFiles = counts.events + counts.index;
  const observedDays = scan.dates.size;
  return {
    counts: {
      ...counts,
      total: historicalFiles + counts.snapshots + counts.manifests,
    },
    bytes: {
      ...bytes,
      total: bytes.events + bytes.index + bytes.snapshots + bytes.manifests,
    },
    daily_rates: {
      observed_days: observedDays,
      events: observedDays === 0 ? 0 : counts.events / observedDays,
      index: observedDays === 0 ? 0 : counts.index / observedDays,
      total: observedDays === 0 ? 0 : historicalFiles / observedDays,
    },
    no_op_scan: {
      runs: options.runs,
      elapsed_ms: scanTimings.map(roundMilliseconds),
      median_ms: roundMilliseconds(median(scanTimings)),
    },
    no_op_sync_model: {
      model: "collect_and_compare_in_memory_byte_mirror",
      runs: options.runs,
      compared_files: comparedFiles,
      elapsed_ms: syncTimings.map(roundMilliseconds),
      median_ms: roundMilliseconds(median(syncTimings)),
    },
  };
}

async function main() {
  const report = await inventoryStore(parseInventoryArgs(process.argv.slice(2)));
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main().catch((error) => {
    process.stderr.write(`event-log inventory failed: ${error.message}\n`);
    process.exitCode = 1;
  });
}
