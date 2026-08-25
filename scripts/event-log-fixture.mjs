#!/usr/bin/env node

import { mkdir, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULTS = Object.freeze({ full: 50_000, index: 30_000, days: 40 });
const SMALL = Object.freeze({ full: 50, index: 30, days: 4 });
const START = new Date("2026-01-01T00:00:00Z");

function positiveInteger(value, option) {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isSafeInteger(parsed) || parsed < 1) {
    throw new Error(`${option} must be a positive integer`);
  }
  return parsed;
}

export function parseFixtureArgs(argv) {
  const options = { root: null, ...DEFAULTS };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--small") {
      Object.assign(options, SMALL);
    } else if (["--root", "--full", "--index", "--days"].includes(argument)) {
      const value = argv[index + 1];
      if (value === undefined) throw new Error(`${argument} requires a value`);
      index += 1;
      const name = argument.slice(2);
      options[name] = name === "root" ? value : positiveInteger(value, argument);
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  if (!options.root) throw new Error("--root is required");
  return options;
}

function dateParts(dayIndex) {
  const date = new Date(START);
  date.setUTCDate(date.getUTCDate() + dayIndex);
  return date.toISOString().slice(0, 10).split("-");
}

function eventPath(store, stream, ordinal, days, suffix) {
  const [year, month, day] = dateParts(ordinal % days);
  const name = `${String(ordinal + 1).padStart(8, "0")}-${suffix}.json`;
  return path.join(store, stream, year, month, day, name);
}

function fullEvent(ordinal) {
  return {
    event_id: `fixture-full-${String(ordinal + 1).padStart(8, "0")}`,
    occurred_at: "2026-01-01T00:00:00Z",
    knot_id: `fixture-knot-${String((ordinal % 1_000) + 1).padStart(4, "0")}`,
    type: "knot.description_set",
    data: { description: `fixture description ${ordinal + 1}` },
  };
}

function indexEvent(ordinal) {
  const knot = `fixture-knot-${String((ordinal % 1_000) + 1).padStart(4, "0")}`;
  return {
    event_id: `fixture-index-${String(ordinal + 1).padStart(8, "0")}`,
    occurred_at: "2026-01-01T00:00:00Z",
    type: "idx.knot_head",
    data: {
      knot_id: knot,
      title: `Fixture knot ${ordinal + 1}`,
      state: "ready_for_planning",
      workflow_id: "work_sdlc",
      profile_id: "autopilot",
      updated_at: "2026-01-01T00:00:00Z",
      terminal: false,
    },
  };
}

async function assertFixtureTargetIsEmpty(store) {
  let entries;
  try {
    entries = await readdir(store);
  } catch (error) {
    if (error.code === "ENOENT") return;
    throw error;
  }
  if (entries.length > 0) {
    throw new Error(`fixture target already contains data: ${store}`);
  }
}

async function writeEvents(store, stream, count, days, suffix, build) {
  const batchSize = 256;
  for (let dayIndex = 0; dayIndex < days; dayIndex += 1) {
    const [year, month, day] = dateParts(dayIndex);
    await mkdir(path.join(store, stream, year, month, day), { recursive: true });
  }
  for (let start = 0; start < count; start += batchSize) {
    const writes = [];
    const end = Math.min(start + batchSize, count);
    for (let ordinal = start; ordinal < end; ordinal += 1) {
      const destination = eventPath(store, stream, ordinal, days, suffix);
      writes.push(writeFile(destination, `${JSON.stringify(build(ordinal))}\n`));
    }
    await Promise.all(writes);
  }
}

export async function generateFixture(options) {
  const root = path.resolve(options.root);
  const store = path.join(root, ".knots");
  await assertFixtureTargetIsEmpty(store);
  await writeEvents(
    store,
    "events",
    options.full,
    options.days,
    "knot.description_set",
    fullEvent,
  );
  await writeEvents(store, "index", options.index, options.days, "idx.knot_head", indexEvent);
  return { days: options.days, full_files: options.full, index_files: options.index };
}

async function main() {
  const summary = await generateFixture(parseFixtureArgs(process.argv.slice(2)));
  process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  main().catch((error) => {
    process.stderr.write(`event-log fixture failed: ${error.message}\n`);
    process.exitCode = 1;
  });
}
