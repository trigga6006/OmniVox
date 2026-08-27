use std::path::{Path, PathBuf};

/// Runtime DLLs produced by the sherpa-onnx `shared` build. The bundle
/// resource map (`tauri.conf.json`) picks these up from a `runtime-dlls/*`
/// glob, which quietly matches zero files rather than failing the build —
/// see FIX F1 in the Parakeet integration review.
const RUNTIME_DLL_NAMES: &[&str] = &[
    "sherpa-onnx-c-api.dll",
    "sherpa-onnx-cxx-api.dll",
    "onnxruntime.dll",
    "onnxruntime_providers_shared.dll",
];

fn main() {
    // Give the main thread a larger stack on Windows — whisper.cpp and llama.cpp
    // in debug builds use deep call stacks with large frames, especially during
    // Vulkan device enumeration.
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg=/STACK:67108864"); // 64 MB main thread stack
    }

    stage_runtime_dlls();

    tauri_build::build();
}

/// Copy the sherpa-onnx/ONNX Runtime DLLs from this build's cargo profile
/// directory into `runtime-dlls/` so the Tauri bundler can pick them up.
///
/// `OUT_DIR` looks like `<target>/<profile>/build/<pkg>-<hash>/out`; walking
/// up three ancestors lands on `<target>/<profile>`, the same technique
/// sherpa-onnx-sys's own build script uses to find its output directory —
/// and where its build script copies these DLLs on Windows. Files that
/// aren't there yet (first cold check) are skipped silently rather than
/// failing the build.
fn stage_runtime_dlls() {
    let dest_dir = Path::new("runtime-dlls");
    let _ = std::fs::create_dir_all(dest_dir);

    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let out_dir = PathBuf::from(out_dir);
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };

    let mut missing: Vec<&str> = Vec::new();
    for name in RUNTIME_DLL_NAMES {
        let source = profile_dir.join(name);
        println!("cargo:rerun-if-changed={}", source.display());
        if source.is_file() {
            let _ = std::fs::copy(&source, dest_dir.join(name));
        } else if !dest_dir.join(name).is_file() {
            missing.push(name);
        }
    }

    // A release bundle without these DLLs installs an app that cannot launch
    // (the exe hard-imports sherpa-onnx-c-api and onnxruntime), and the
    // bundler's `runtime-dlls/*` glob matches zero files without complaint.
    // This staging pass is BEST-EFFORT ONLY: Cargo gives this build script no
    // ordering edge against sherpa-onnx-sys's build script, so on a cold
    // build the DLLs may legitimately not exist yet when we run. The
    // authoritative staging happens in `beforeBundleCommand`
    // (scripts/stage-runtime-dlls.mjs), which runs after the whole cargo
    // build completes and fails loudly if the DLLs are still absent.
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && !missing.is_empty()
    {
        println!(
            "cargo:warning=runtime DLLs not staged yet ({missing:?} missing from {}); \
             the beforeBundleCommand hook stages them after the build completes",
            profile_dir.display()
        );
    }
}
