#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const packageJsonPath = path.join(repoRoot, "package.json");
const changesetDir = path.join(repoRoot, ".changeset");
const packageLinePattern =
  /^(?:"([^"]+)"|'([^']+)'|([^'":][^:]*?)):\s*(patch|minor|major)\s*$/;

function readJson(filePath, description) {
  if (!fs.existsSync(filePath)) {
    throw new Error(`${description} was not found`);
  }

  try {
    return JSON.parse(fs.readFileSync(filePath, "utf8"));
  } catch (error) {
    throw new Error(`could not parse ${description}: ${error.message}`);
  }
}

function readPackageName() {
  const parsed = readJson(packageJsonPath, "package.json");
  if (typeof parsed.name !== "string" || parsed.name.trim().length === 0) {
    throw new Error("package.json name is missing or invalid");
  }
  return parsed.name;
}

function listChangesets() {
  if (!fs.existsSync(changesetDir)) {
    return [];
  }

  return fs
    .readdirSync(changesetDir)
    .filter((entry) => entry.endsWith(".md") && entry !== "README.md")
    .sort()
    .map((entry) => path.join(changesetDir, entry));
}

function frontmatterLines(filePath) {
  const raw = fs.readFileSync(filePath, "utf8");
  const lines = raw.split(/\r?\n/);
  if (lines[0]?.trim() !== "---") {
    return {
      errors: [`${path.relative(repoRoot, filePath)} must start with changeset frontmatter`],
      lines: [],
    };
  }

  const closingIndex = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
  if (closingIndex === -1) {
    return {
      errors: [`${path.relative(repoRoot, filePath)} is missing closing frontmatter marker`],
      lines: [],
    };
  }

  return {
    errors: [],
    lines: lines.slice(1, closingIndex),
  };
}

function parsePackageLine(line) {
  const match = line.match(packageLinePattern);
  if (!match) {
    return null;
  }

  const packageName = (match[1] ?? match[2] ?? match[3]).trim();
  return {
    bump: match[4],
    packageName,
  };
}

function validateChangeset(filePath, expectedPackageName) {
  const relativePath = path.relative(repoRoot, filePath);
  const frontmatter = frontmatterLines(filePath);
  const errors = [...frontmatter.errors];
  let packageLineCount = 0;

  for (const [index, line] of frontmatter.lines.entries()) {
    const trimmed = line.trim();
    if (trimmed.length === 0 || trimmed.startsWith("#")) {
      continue;
    }

    const parsed = parsePackageLine(trimmed);
    if (!parsed) {
      errors.push(`${relativePath}:${index + 2} has unsupported changeset frontmatter`);
      continue;
    }

    packageLineCount += 1;
    if (parsed.packageName === "kno" && expectedPackageName !== "kno") {
      errors.push(
        `${relativePath}:${index + 2} uses package key "kno"; use ` +
          `"${expectedPackageName}" instead. "kno" is the CLI name, not the package.`
      );
      continue;
    }

    if (parsed.packageName !== expectedPackageName) {
      errors.push(
        `${relativePath}:${index + 2} uses unknown package key ` +
          `"${parsed.packageName}"; expected "${expectedPackageName}".`
      );
    }
  }

  if (frontmatter.errors.length === 0 && packageLineCount === 0) {
    errors.push(`${relativePath} does not declare a package bump`);
  }

  return errors;
}

function main() {
  const expectedPackageName = readPackageName();
  const changesetFiles = listChangesets();
  if (changesetFiles.length === 0) {
    console.log("No unreleased changesets to validate.");
    return;
  }

  const errors = changesetFiles.flatMap((filePath) =>
    validateChangeset(filePath, expectedPackageName)
  );
  if (errors.length > 0) {
    for (const error of errors) {
      console.error(`error: ${error}`);
    }
    process.exit(1);
  }

  console.log(
    `Validated ${changesetFiles.length} changeset file(s) for ` +
      `package key "${expectedPackageName}".`
  );
}

try {
  main();
} catch (error) {
  console.error(`error: ${error.message}`);
  process.exit(1);
}
