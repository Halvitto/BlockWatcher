import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
} from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(appRoot, "..");
const ccusageRoot = resolve(repoRoot, "vendor/ccusage");
const rustRoot = resolve(ccusageRoot, "rust");
const pricing = resolve(appRoot, "src-tauri/sidecar/empty-pricing.json");

if (!existsSync(resolve(ccusageRoot, "package.json"))) {
  throw new Error("Missing vendor/ccusage. Clone BlockWatcher with submodules.");
}

const host = execFileSync("rustc", ["--print", "host-tuple"], {
  encoding: "utf8",
}).trim();
const target =
  process.env.BLOCKWATCHER_TARGET ??
  process.env.TAURI_ENV_TARGET_TRIPLE ??
  host;
const extension = process.platform === "win32" ? ".exe" : "";
const targetArguments = target === host ? [] : ["--target", target];

execFileSync(
  "cargo",
  [
    "build",
    "--locked",
    "--release",
    "-p",
    "ccusage",
    "--bin",
    "ccusage",
    ...targetArguments,
  ],
  {
    cwd: rustRoot,
    env: { ...process.env, CCUSAGE_PRICING_JSON_PATH: pricing },
    stdio: "inherit",
  },
);

const source = resolve(
  rustRoot,
  "target",
  ...(target === host ? [] : [target]),
  `release/ccusage${extension}`,
);
const destination = resolve(
  appRoot,
  `src-tauri/binaries/ccusage-${target}${extension}`,
);
mkdirSync(dirname(destination), { recursive: true });
copyFileSync(source, destination);
if (process.platform !== "win32") chmodSync(destination, 0o755);
