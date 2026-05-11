import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

const scriptPath = fileURLToPath(new URL("./check-changesets.mjs", import.meta.url));

function makeRepo(files) {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "knots-changesets-"));
  fs.mkdirSync(path.join(repoRoot, ".changeset"));
  fs.writeFileSync(
    path.join(repoRoot, "package.json"),
    JSON.stringify({ name: "knots", version: "0.0.0" }, null, 2),
    "utf8"
  );

  for (const [relativePath, content] of Object.entries(files)) {
    const filePath = path.join(repoRoot, relativePath);
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    fs.writeFileSync(filePath, content, "utf8");
  }

  return repoRoot;
}

function runCheck(repoRoot) {
  return spawnSync(process.execPath, [scriptPath], {
    cwd: repoRoot,
    encoding: "utf8",
  });
}

test("accepts changesets for the package name", () => {
  const repoRoot = makeRepo({
    ".changeset/good.md": [
      "---",
      '"knots": patch',
      "---",
      "",
      "Ship the thing.",
    ].join("\n"),
  });

  const result = runCheck(repoRoot);

  assert.equal(result.status, 0);
  assert.match(result.stdout, /Validated 1 changeset file/);
});

test("rejects the CLI name as a changeset package key", () => {
  const repoRoot = makeRepo({
    ".changeset/bad.md": [
      "---",
      '"kno": patch',
      "---",
      "",
      "This should use the package name.",
    ].join("\n"),
  });

  const result = runCheck(repoRoot);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /uses package key "kno"/);
  assert.match(result.stderr, /use "knots" instead/);
});

test("rejects unknown package keys", () => {
  const repoRoot = makeRepo({
    ".changeset/bad.md": [
      "---",
      '"other": minor',
      "---",
      "",
      "This package is not in the repo.",
    ].join("\n"),
  });

  const result = runCheck(repoRoot);

  assert.equal(result.status, 1);
  assert.match(result.stderr, /uses unknown package key "other"/);
});

test("ignores the changeset readme", () => {
  const repoRoot = makeRepo({
    ".changeset/README.md": "This is not a release note.\n",
  });

  const result = runCheck(repoRoot);

  assert.equal(result.status, 0);
  assert.match(result.stdout, /No unreleased changesets/);
});
