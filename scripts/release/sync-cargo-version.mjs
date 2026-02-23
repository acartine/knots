#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const packageJsonPath = path.join(repoRoot, "package.json");
const cargoTomlPath = path.join(repoRoot, "Cargo.toml");
const checkOnly = process.argv.includes("--check");

const semverPattern =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-.]+)?(\+[0-9A-Za-z-.]+)?$/;

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

function readPackageVersion() {
  if (!fs.existsSync(packageJsonPath)) {
    fail("package.json was not found");
  }
  const raw = fs.readFileSync(packageJsonPath, "utf8");
  const parsed = JSON.parse(raw);
  const version = parsed.version;
  if (typeof version !== "string" || version.trim().length === 0) {
    fail("package.json version is missing or invalid");
  }
  if (!semverPattern.test(version)) {
    fail(`package.json version '${version}' is not valid semver`);
  }
  return version;
}

function findCargoVersionLine(lines) {
  let inPackageSection = false;
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    const trimmed = line.trim();

    if (trimmed.startsWith("[")) {
      inPackageSection = trimmed === "[package]";
      continue;
    }

    if (!inPackageSection) {
      continue;
    }

    const match = line.match(/^version\s*=\s*"([^"]+)"\s*$/);
    if (match) {
      return { lineIndex: i, version: match[1] };
    }
  }
  return null;
}

function syncCargoVersion(targetVersion) {
  if (!fs.existsSync(cargoTomlPath)) {
    fail("Cargo.toml was not found");
  }

  const raw = fs.readFileSync(cargoTomlPath, "utf8");
  const lines = raw.split(/\r?\n/);
  const located = findCargoVersionLine(lines);
  if (!located) {
    fail("could not find package.version in Cargo.toml");
  }

  if (!semverPattern.test(located.version)) {
    fail(`Cargo.toml version '${located.version}' is not valid semver`);
  }

  if (checkOnly) {
    if (located.version !== targetVersion) {
      fail(
        `Cargo.toml version '${located.version}' differs from package.json ` +
          `version '${targetVersion}'`
      );
    }
    console.log(`Cargo.toml is in sync with package.json (${targetVersion})`);
    return;
  }

  if (located.version === targetVersion) {
    console.log(`Cargo.toml already matches package.json (${targetVersion})`);
    return;
  }

  lines[located.lineIndex] = `version = "${targetVersion}"`;
  fs.writeFileSync(cargoTomlPath, lines.join("\n"), "utf8");
  console.log(`Updated Cargo.toml version: ${located.version} -> ${targetVersion}`);
}

const packageVersion = readPackageVersion();
syncCargoVersion(packageVersion);
