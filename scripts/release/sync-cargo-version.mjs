#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const packageJsonPath = path.join(repoRoot, "package.json");
const cargoTomlPath = path.join(repoRoot, "Cargo.toml");
const cargoLockPath = path.join(repoRoot, "Cargo.lock");
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
  const name = parsed.name;
  const version = parsed.version;
  if (typeof name !== "string" || name.trim().length === 0) {
    fail("package.json name is missing or invalid");
  }
  if (typeof version !== "string" || version.trim().length === 0) {
    fail("package.json version is missing or invalid");
  }
  if (!semverPattern.test(version)) {
    fail(`package.json version '${version}' is not valid semver`);
  }
  return { name, version };
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

function findCargoLockPackageVersion(lines, packageName) {
  let inPackage = false;
  let currentName = null;
  let currentVersion = null;

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed === "[[package]]") {
      if (inPackage && currentName === packageName && currentVersion) {
        return currentVersion;
      }
      inPackage = true;
      currentName = null;
      currentVersion = null;
      continue;
    }

    if (!inPackage) {
      continue;
    }

    const nameMatch = trimmed.match(/^name\s*=\s*"([^"]+)"\s*$/);
    if (nameMatch) {
      currentName = nameMatch[1];
      continue;
    }

    const versionMatch = trimmed.match(/^version\s*=\s*"([^"]+)"\s*$/);
    if (versionMatch) {
      currentVersion = versionMatch[1];
    }
  }

  if (inPackage && currentName === packageName && currentVersion) {
    return currentVersion;
  }

  return null;
}

function readCargoLockVersion(packageName) {
  if (!fs.existsSync(cargoLockPath)) {
    fail("Cargo.lock was not found");
  }

  const raw = fs.readFileSync(cargoLockPath, "utf8");
  const version = findCargoLockPackageVersion(raw.split(/\r?\n/), packageName);
  if (!version) {
    fail(`could not find package '${packageName}' in Cargo.lock`);
  }
  if (!semverPattern.test(version)) {
    fail(`Cargo.lock version '${version}' is not valid semver`);
  }
  return version;
}

function refreshCargoLockfile() {
  try {
    execFileSync(
      "cargo",
      ["update", "-w"],
      { cwd: repoRoot, stdio: "ignore" }
    );
  } catch (error) {
    const detail = error.stderr?.toString().trim() || error.message;
    fail(`failed to refresh Cargo.lock via cargo update -w: ${detail}`);
  }
}

function syncCargoLock(packageName, targetVersion) {
  refreshCargoLockfile();
  const lockVersion = readCargoLockVersion(packageName);
  if (lockVersion !== targetVersion) {
    fail(
      `Cargo.lock version '${lockVersion}' differs from package.json ` +
        `version '${targetVersion}'`
    );
  }
  console.log(`Cargo.lock is in sync with package.json (${targetVersion})`);
}

function checkCargoLockVersion(packageName, targetVersion) {
  const lockVersion = readCargoLockVersion(packageName);
  if (lockVersion !== targetVersion) {
    fail(
      `Cargo.lock version '${lockVersion}' differs from package.json ` +
        `version '${targetVersion}'`
    );
  }
  console.log(`Cargo.lock is in sync with package.json (${targetVersion})`);
}

const pkg = readPackageVersion();
syncCargoVersion(pkg.version);
if (checkOnly) {
  checkCargoLockVersion(pkg.name, pkg.version);
} else {
  syncCargoLock(pkg.name, pkg.version);
}
