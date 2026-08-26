#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const policyPath = resolve(root, "config/github-knots-v2-rulesets.json");
const policy = JSON.parse(readFileSync(policyPath, "utf8"));
const [command, phase] = process.argv.slice(2);

function fail(message) {
  process.stderr.write(`error: ${message}\n`);
  process.exit(1);
}

function gh(args, input) {
  const result = spawnSync("gh", args, {
    cwd: root,
    encoding: "utf8",
    input,
    stdio: [input === undefined ? "inherit" : "pipe", "pipe", "pipe"],
  });
  if (result.status !== 0) {
    fail(result.stderr.trim() || `gh ${args.join(" ")} failed`);
  }
  return result.stdout.trim();
}

function desired(rule) {
  return {
    name: rule.name,
    target: "branch",
    enforcement: rule.enforcement || "active",
    bypass_actors: [],
    conditions: { ref_name: { exclude: [], include: rule.patterns } },
    rules: rule.rules.map((type) => ({ type })),
  };
}

function listRulesets() {
  const output = gh(["api", `repos/${policy.repository}/rulesets`, "--paginate"]);
  return JSON.parse(output || "[]");
}

function normalized(value) {
  if (Array.isArray(value)) return value.map(normalized);
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value).sort().map((key) => [key, normalized(value[key])]),
    );
  }
  return value;
}

function canonical(value) {
  return JSON.stringify(normalized(value));
}

function verifyActual(actual, expected) {
  const relevant = {
    name: actual.name,
    target: actual.target,
    enforcement: actual.enforcement,
    bypass_actors: actual.bypass_actors.map(({ actor_id, actor_type, bypass_mode }) => ({
      actor_id,
      actor_type,
      bypass_mode,
    })),
    conditions: actual.conditions,
    rules: actual.rules.map(({ type }) => ({ type })),
  };
  if (canonical(relevant) !== canonical(expected)) {
    fail(`ruleset ${expected.name} does not match the tracked policy`);
  }
  if (relevant.bypass_actors.length !== 0) {
    fail(`ruleset ${expected.name} contains a bypass actor`);
  }
}

function requireCanaryEvidence() {
  const evidencePath = process.env.KNOTS_V2_CANARY_EVIDENCE;
  if (!evidencePath) fail("KNOTS_V2_CANARY_EVIDENCE is required before production apply");
  const evidence = JSON.parse(readFileSync(evidencePath, "utf8"));
  const required = [
    "ordinary_creation_succeeded",
    "ordinary_rewrite_rejected",
    "ordinary_deletion_rejected",
    "actions_attestation_verified",
    "actions_control_rewrite_rejected",
    "actions_control_deletion_rejected",
    "unattested_lookalike_rejected",
    "exact_workflow_head_verified",
  ];
  if (required.some((key) => evidence[key] !== true)) {
    fail("canary evidence does not prove every required policy behavior");
  }
  const sha = /^[0-9a-f]{40}$/;
  const digest = /^[0-9a-f]{64}$/;
  const runId = /^[1-9][0-9]*$/;
  if (!sha.test(evidence.main_sha)
      || !digest.test(evidence.manifest_sha256)
      || !runId.test(evidence.actions_run_id)
      || !sha.test(evidence.control_oid)) {
    fail("canary evidence does not bind an exact attested control epoch");
  }
}

function applyPhase(selected) {
  if (selected === "production") {
    requireCanaryEvidence();
  }
  const existing = listRulesets();
  for (const rule of policy.phases[selected]) {
    const payload = JSON.stringify(desired(rule));
    const match = existing.find((candidate) => candidate.name === rule.name);
    const endpoint = match
      ? `repos/${policy.repository}/rulesets/${match.id}`
      : `repos/${policy.repository}/rulesets`;
    gh(["api", "--method", match ? "PUT" : "POST", endpoint, "--input", "-"], payload);
  }
}

function verifyPhase(selected) {
  const existing = listRulesets();
  for (const rule of policy.phases[selected]) {
    const summary = existing.find((candidate) => candidate.name === rule.name);
    if (!summary) fail(`ruleset ${rule.name} is missing`);
    const actual = JSON.parse(gh(["api", `repos/${policy.repository}/rulesets/${summary.id}`]));
    verifyActual(actual, desired(rule));
    process.stdout.write(`${actual.id}\t${actual.name}\tverified\n`);
  }
}

const usage = "usage: knots-v2-rulesets.mjs <plan|apply|verify> "
  + "<authority_code|canary|production>";
if (!policy.phases[phase]) fail(usage);
if (command === "plan") {
  process.stdout.write(`${JSON.stringify(policy.phases[phase].map(desired), null, 2)}\n`);
} else if (command === "apply") {
  applyPhase(phase);
  verifyPhase(phase);
} else if (command === "verify") {
  verifyPhase(phase);
} else {
  fail(usage);
}
