"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const PLATFORM_PACKAGES = {
  "darwin-arm64": "@editnovel/cli-darwin-arm64",
  "darwin-x64": "@editnovel/cli-darwin-x64",
  "linux-arm64": "@editnovel/cli-linux-arm64-gnu",
  "linux-x64": "@editnovel/cli-linux-x64-gnu",
  "win32-arm64": "@editnovel/cli-win32-arm64-msvc",
  "win32-x64": "@editnovel/cli-win32-x64-msvc"
};

function resolveBinary() {
  if (process.env.EDITNOVEL_BIN_PATH && fs.existsSync(process.env.EDITNOVEL_BIN_PATH)) {
    return process.env.EDITNOVEL_BIN_PATH;
  }

  const binName = process.platform === "win32" ? "editnovel.exe" : "editnovel";
  const key = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[key];

  if (pkg) {
    try {
      const pkgJsonPath = require.resolve(`${pkg}/package.json`);
      const candidate = path.join(path.dirname(pkgJsonPath), "bin", binName);
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    } catch (_) {}
  }

  const repoRoot = path.resolve(__dirname, "..", "..", "..");
  const releaseCandidate = path.join(repoRoot, "target", "release", binName);
  if (fs.existsSync(releaseCandidate)) {
    return releaseCandidate;
  }
  const debugCandidate = path.join(repoRoot, "target", "debug", binName);
  if (fs.existsSync(debugCandidate)) {
    return debugCandidate;
  }

  return binName;
}

function main() {
  const bin = resolveBinary();
  const args = process.argv.slice(2);
  const result = spawnSync(bin, args, { stdio: "inherit" });

  if (result.error) {
    const fallback = spawnSync("cargo", ["run", "--quiet", "--", ...args], {
      stdio: "inherit"
    });
    if (fallback.error) {
      process.stderr.write(
        JSON.stringify({
          status: "error",
          error: `Unable to locate editnovel binary (${result.error.message})`
        }) + "\n"
      );
      process.exit(1);
    }
    process.exit(fallback.status ?? 0);
  }

  process.exit(result.status ?? 0);
}

main();
