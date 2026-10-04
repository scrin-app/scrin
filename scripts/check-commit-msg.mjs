#!/usr/bin/env node
// commit-msg hook: Conventional Commits header (<= 100 chars) + DCO sign-off (CONTRIBUTING.md).
// Usage: node scripts/check-commit-msg.mjs <path-to-COMMIT_EDITMSG>
import { readFileSync } from "node:fs";

const TYPES = ["feat", "fix", "perf", "refactor", "docs", "test", "build", "ci", "chore", "revert", "style", "security"];
const MAX = 100;

const file = process.argv[2];
if (!file) {
  console.error("commit-msg: missing message file argument");
  process.exit(2);
}
const lines = readFileSync(file, "utf8")
  .split(/\r?\n/)
  .filter((l) => !l.startsWith("#"));
const header = lines.find((l) => l.trim()) ?? "";

// git-generated messages keep their own shape
if (/^(Merge |Revert "|fixup! |squash! |amend! )/.test(header)) process.exit(0);

const errors = [];
const re = new RegExp(`^(${TYPES.join("|")})(\\([a-z0-9][a-z0-9,./-]*\\))?!?: \\S`);
if (!re.test(header)) {
  errors.push(`header must be "<type>(<scope>)?: <subject>"; types: ${TYPES.join(", ")}`);
}
if (header.length > MAX) errors.push(`header is ${header.length} chars (max ${MAX})`);
if (header.endsWith(".")) errors.push("header must not end with a period");

const signoff = lines.some((l) => /^Signed-off-by: .+ <[^<>@\s]+@[^<>\s]+>$/.test(l.trim()));
if (!signoff) errors.push('missing DCO trailer "Signed-off-by: Name <email>" — commit with `git commit -s`');

if (errors.length) {
  console.error(`commit-msg: "${header}"`);
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}
