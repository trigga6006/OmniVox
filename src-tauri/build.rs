fn main() {
    // Give the main thread a larger stack on Windows — whisper.cpp and llama.cpp
    // in debug builds use deep call stacks with large frames, especially during
    // Vulkan device enumeration.
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg=/STACK:67108864"); // 64 MB main thread stack
    }

    tauri_build::build();
}
