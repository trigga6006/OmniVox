// Stage the sherpa-onnx/ONNX Runtime DLLs for bundling. Runs as Tauri's
// beforeBundleCommand — i.e. AFTER the whole cargo build has completed, which
// is the only point where the DLLs are guaranteed to exist. (src-tauri/build.rs
// also stages them best-effort, but Cargo gives it no ordering edge against
// sherpa-onnx-sys's build script, so on a cold build it can run too early —
// that race produced an installer with no DLLs during release verification.)
//
// The bundler's `"runtime-dlls/*": "/"` resource glob silently matches zero
// files, so THIS script is the loud gate: missing DLLs fail the bundle.
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { execSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUNTIME_DLLS = [
  "sherpa-onnx-c-api.dll",
  "sherpa-onnx-cxx-api.dll",
  "onnxruntime.dll",
  "onnxruntime_providers_shared.dll",
];

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = join(repoRoot, "src-tauri", "Cargo.toml");

// CARGO_TARGET_DIR (CI) wins; otherwise ask cargo, which resolves the
// .cargo/config.toml target-dir redirects this repo uses locally.
let targetDir = process.env.CARGO_TARGET_DIR;
if (!targetDir) {
  const metadata = JSON.parse(
    execSync(`cargo metadata --format-version 1 --no-deps --manifest-path "${manifest}"`, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    })
  );
  targetDir = metadata.target_directory;
}

const releaseDir = join(targetDir, "release");
const stageDir = join(repoRoot, "src-tauri", "runtime-dlls");
mkdirSync(stageDir, { recursive: true });

const missing = [];
for (const name of RUNTIME_DLLS) {
  const source = join(releaseDir, name);
  if (existsSync(source)) {
    copyFileSync(source, join(stageDir, name));
    console.log(`staged ${name}`);
  } else {
    missing.push(source);
  }
}

if (missing.length > 0) {
  console.error(
    "stage-runtime-dlls: required runtime DLLs are missing after the cargo " +
      "build — the bundle would produce an installer whose app cannot " +
      "launch.\nMissing:\n" +
      missing.map((p) => `  ${p}`).join("\n")
  );
  process.exit(1);
}
