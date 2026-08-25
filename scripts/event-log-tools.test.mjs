import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { generateFixture, parseFixtureArgs } from "./event-log-fixture.mjs";
import { inventoryStore, parseInventoryArgs } from "./event-log-inventory.mjs";

async function withWorkspace(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "knots-event-log-tools-"));
  try {
    await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("small fixture is deterministic and inventory reports stable totals", async () => {
  await withWorkspace(async (root) => {
    const options = parseFixtureArgs(["--root", root, "--small"]);
    assert.deepEqual(await generateFixture(options), {
      days: 4,
      full_files: 50,
      index_files: 30,
    });

    const manifests = path.join(root, ".knots", "compaction");
    await mkdir(manifests, { recursive: true });
    await writeFile(path.join(manifests, "checkpoint.json"), "{}\n");
    const report = await inventoryStore({ root, runs: 2 });
    assert.deepEqual(report.counts, {
      events: 50,
      index: 30,
      snapshots: 0,
      manifests: 1,
      total: 81,
    });
    assert.deepEqual(report.daily_rates, {
      observed_days: 4,
      events: 12.5,
      index: 7.5,
      total: 20,
    });
    assert.equal(report.no_op_scan.runs, 2);
    assert.equal(report.no_op_scan.elapsed_ms.length, 2);
    assert.equal(report.no_op_sync_model.compared_files, 81);
    assert.equal(report.no_op_sync_model.elapsed_ms.length, 2);
    assert.equal(
      report.no_op_sync_model.model,
      "collect_and_compare_in_memory_byte_mirror",
    );
    assert.ok(report.bytes.total > 0);
  });
});

test("fixture refuses to overwrite an existing store", async () => {
  await withWorkspace(async (root) => {
    const options = parseFixtureArgs(["--root", root, "--full", "1", "--index", "1"]);
    await generateFixture(options);
    await assert.rejects(generateFixture(options), /already contains data/);
  });
});

test("argument parsers reject incomplete and invalid input", () => {
  assert.throws(() => parseFixtureArgs([]), /--root is required/);
  assert.throws(() => parseFixtureArgs(["--root", "x", "--days", "0"]), /positive integer/);
  assert.throws(() => parseInventoryArgs(["--root", "x", "--runs", "bad"]), /positive integer/);
  assert.deepEqual(parseInventoryArgs(["--root", "fixture"]), {
    root: "fixture",
    runs: 5,
  });
});
