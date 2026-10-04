#!/usr/bin/env node
// pre-commit: refuse secrets-bearing files, build output, scratch files and large blobs.
// Usage: node scripts/check-staged.mjs <staged files...>
import { statSync } from "node:fs";

const MAX_BYTES = 1024 * 1024; // 1 MB
const ALLOW_LARGE = [/^LICENSE$/];
const FORBID = [
  [/(^|\/)\.env(\.(?!example$)[^/]*)?$/, ".env files hold secrets; commit .env.example instead"],
  [/(^|\/)\.copilot-tmp\//, ".copilot-tmp/ is scratch space"],
  [/(^|\/)(target|node_modules|dist|build|\.turbo)\//, "build output"],
  [/\.(jks|keystore|p12|pfx|pem|key|aab|apk|ipa|msix|exe|msi)$/i, "signing material or binary artefact"],
  [/service-account.*\.json$/i, "cloud credentials"],
  [/(^|\/)google-services\.json$/, "Firebase config is per-environment"],
  [/(^|\/)local\.properties$/, "machine-local Android SDK path"],
];

let bad = 0;
for (const f of process.argv.slice(2).map((p) => p.replaceAll("\\", "/"))) {
  const hit = FORBID.find(([re]) => re.test(f));
  if (hit) {
    console.error(`forbidden path staged: ${f} (${hit[1]})`);
    bad++;
    continue;
  }
  let size;
  try {
    size = statSync(f).size;
  } catch {
    continue; // deleted in this commit
  }
  if (size > MAX_BYTES && !ALLOW_LARGE.some((re) => re.test(f))) {
    console.error(`too large (${(size / 1024).toFixed(0)} KB > 1024 KB): ${f}`);
    bad++;
  }
}
process.exit(bad ? 1 : 0);
