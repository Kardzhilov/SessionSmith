import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const desktopRoot = fileURLToPath(new URL("../", import.meta.url));
const bindingsPath = fileURLToPath(new URL("../src/api/generated/bindings.ts", import.meta.url));
const before = await readFile(bindingsPath, "utf8");
const result = spawnSync(
  "cargo",
  ["run", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "export-bindings"],
  { cwd: desktopRoot, stdio: "inherit" },
);

if (result.error) {
  throw result.error;
}
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

const after = await readFile(bindingsPath, "utf8");
if (before !== after) {
  console.error("Generated bindings were stale. Review and check in the regenerated file.");
  process.exit(1);
}