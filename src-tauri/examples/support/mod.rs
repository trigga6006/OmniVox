// Each example compiles this private shared module independently and therefore
// uses only a subset of its report helpers/corpora.
#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

pub mod command_corpus;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Cpu,
    Vulkan,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedActualBackend {
    Cpu,
    Unknown,
    NotExercised,
}

/// What the benchmark can prove about the backend that actually executed.
///
/// A successful Vulkan-configured model load is not evidence that every layer
/// was offloaded: both llama.cpp and whisper.cpp may fall back without exposing
/// a verified device/layer count through the APIs used by this application.
#[derive(Debug)]
pub struct BackendObservation {
    pub verified_actual: VerifiedActualBackend,
    pub evidence: String,
}

impl BackendObservation {
    pub fn model_path(requested: Backend, vulkan_evidence: impl Into<String>) -> Self {
        match requested {
            Backend::Cpu => Self {
                verified_actual: VerifiedActualBackend::Cpu,
                evidence: "GPU execution was disabled in the model configuration".to_string(),
            },
            Backend::Vulkan => Self {
                verified_actual: VerifiedActualBackend::Unknown,
                evidence: vulkan_evidence.into(),
            },
        }
    }

    pub fn not_exercised(evidence: impl Into<String>) -> Self {
        Self {
            verified_actual: VerifiedActualBackend::NotExercised,
            evidence: evidence.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BackendMetadata {
    pub requested: Backend,
    pub compiled_cpu: bool,
    pub compiled_vulkan: bool,
    pub verified_actual: VerifiedActualBackend,
    pub evidence: String,
}

impl Backend {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "cpu" => Ok(Self::Cpu),
            "vulkan" => Ok(Self::Vulkan),
            _ => Err(format!(
                "unsupported backend '{value}'; expected cpu or vulkan"
            )),
        }
    }

    pub fn use_gpu(self) -> bool {
        matches!(self, Self::Vulkan)
    }

    pub fn validate_build(self) -> Result<(), String> {
        if matches!(self, Self::Vulkan) && !cfg!(feature = "vulkan") {
            return Err(
                "Vulkan backend requested, but this binary was not built with --features vulkan"
                    .to_string(),
            );
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct CommonArgs {
    pub model_path: PathBuf,
    pub backend: Backend,
    pub runs: usize,
    pub warmups: usize,
    pub json_path: Option<PathBuf>,
    pub max_p95_ms: Option<u64>,
    pub min_quality_pct: Option<f64>,
    pub source_label: String,
    pub hardware_label: String,
}

pub fn parse_common_args(model_env: &str, usage: &str) -> Result<CommonArgs, String> {
    let mut model_path = None;
    let mut backend = Backend::Cpu;
    let mut runs = 1usize;
    let mut warmups = 1usize;
    let mut json_path = None;
    let mut max_p95_ms = None;
    let mut min_quality_pct = None;
    let mut source_label = String::new();
    let mut hardware_label = String::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let next_value = |flag: &str, args: &mut std::iter::Skip<std::env::Args>| {
            args.next()
                .ok_or_else(|| format!("{flag} requires a value\n\n{usage}"))
        };
        match arg.as_str() {
            "--model" => model_path = Some(PathBuf::from(next_value("--model", &mut args)?)),
            "--backend" => backend = Backend::parse(&next_value("--backend", &mut args)?)?,
            "--runs" => {
                runs = next_value("--runs", &mut args)?
                    .parse()
                    .map_err(|_| "--runs must be a positive integer".to_string())?;
            }
            "--warmups" => {
                warmups = next_value("--warmups", &mut args)?
                    .parse()
                    .map_err(|_| "--warmups must be a non-negative integer".to_string())?;
            }
            "--json" => json_path = Some(PathBuf::from(next_value("--json", &mut args)?)),
            "--max-p95-ms" => {
                max_p95_ms = Some(
                    next_value("--max-p95-ms", &mut args)?
                        .parse()
                        .map_err(|_| "--max-p95-ms must be a non-negative integer".to_string())?,
                );
            }
            "--min-quality-pct" => {
                min_quality_pct = Some(
                    next_value("--min-quality-pct", &mut args)?
                        .parse()
                        .map_err(|_| "--min-quality-pct must be a number".to_string())?,
                );
            }
            "--source-label" => source_label = next_value("--source-label", &mut args)?,
            "--hardware-label" => hardware_label = next_value("--hardware-label", &mut args)?,
            "--help" | "-h" => {
                println!("{usage}");
                std::process::exit(0);
            }
            value if !value.starts_with('-') && model_path.is_none() => {
                model_path = Some(PathBuf::from(value));
            }
            _ => return Err(format!("unknown argument '{arg}'\n\n{usage}")),
        }
    }

    if runs == 0 {
        return Err("--runs must be at least 1".to_string());
    }
    if let Some(minimum) = min_quality_pct {
        if !(0.0..=100.0).contains(&minimum) {
            return Err("--min-quality-pct must be between 0 and 100".to_string());
        }
    }

    let model_path = model_path
        .or_else(|| std::env::var_os(model_env).map(PathBuf::from))
        .ok_or_else(|| format!("missing model path (--model or {model_env})\n\n{usage}"))?;
    // A directory is a valid model too: sherpa-onnx transducers ship as a set
    // of ONNX files rather than one weights blob.
    if !model_path.is_file() && !model_path.is_dir() {
        return Err(format!(
            "model path does not exist: {}",
            model_path.display()
        ));
    }
    backend.validate_build()?;

    Ok(CommonArgs {
        model_path,
        backend,
        runs,
        warmups,
        json_path,
        max_p95_ms,
        min_quality_pct,
        source_label,
        hardware_label,
    })
}

#[derive(Debug, Serialize)]
pub struct SourceMetadata {
    pub revision: String,
    pub dirty: bool,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct HardwareMetadata {
    pub label: String,
    pub os: &'static str,
    pub arch: &'static str,
    pub logical_cpus: usize,
    pub cpu_identifier: String,
    pub gpu_identifier: String,
}

#[derive(Debug, Serialize)]
pub struct ModelMetadata {
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct ProductionFingerprint {
    pub files: Vec<String>,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct LatencySummary {
    pub count: usize,
    pub min_ms: u64,
    pub max_ms: u64,
    pub mean_ms: f64,
    pub p50_ms: u64,
    pub p95_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct PhaseReport {
    pub name: String,
    pub kind: &'static str,
    pub samples_ms: Vec<u64>,
    pub latency: LatencySummary,
}

impl PhaseReport {
    pub fn new(name: impl Into<String>, kind: &'static str, samples_ms: Vec<u64>) -> Self {
        let latency = summarize(&samples_ms);
        Self {
            name: name.into(),
            kind,
            samples_ms,
            latency,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ThresholdResult {
    pub metric: String,
    pub operator: &'static str,
    pub expected: f64,
    pub actual: f64,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkReport {
    pub schema_version: u32,
    pub benchmark: String,
    pub generated_at_utc: String,
    pub source: SourceMetadata,
    pub harness_sha256: String,
    pub production: ProductionFingerprint,
    pub backend: BackendMetadata,
    pub hardware: HardwareMetadata,
    pub model: ModelMetadata,
    pub config: Value,
    pub phases: Vec<PhaseReport>,
    pub quality: Value,
    pub thresholds: Vec<ThresholdResult>,
    pub passed: bool,
}

pub struct ReportInput {
    pub benchmark: String,
    pub args: CommonArgs,
    pub harness_sha256: String,
    pub production: ProductionFingerprint,
    pub backend_observation: BackendObservation,
    pub config: Value,
    pub phases: Vec<PhaseReport>,
    pub quality: Value,
    pub quality_pct: f64,
    pub additional_thresholds: Vec<ThresholdResult>,
}

pub fn finish_report(input: ReportInput) -> Result<bool, String> {
    let thresholds = default_latency_thresholds(&input);
    finish_report_with_latency_thresholds(input, thresholds)
}

/// Finish a report with benchmark-specific latency thresholds.
///
/// Candidate microbenchmarks that compare diagnostic and production paths
/// should use this instead of pooling every `warm` phase into one percentile.
/// Quality gating remains shared and is appended here as usual.
pub fn finish_report_with_latency_thresholds(
    input: ReportInput,
    mut thresholds: Vec<ThresholdResult>,
) -> Result<bool, String> {
    thresholds.extend(input.additional_thresholds.iter().map(clone_threshold));
    if let Some(minimum) = input.args.min_quality_pct {
        thresholds.push(ThresholdResult {
            metric: "quality_pct".to_string(),
            operator: ">=",
            expected: minimum,
            actual: input.quality_pct,
            passed: input.quality_pct >= minimum,
        });
    }
    let passed = thresholds.iter().all(|threshold| threshold.passed);

    // Hash after all timed work. Reading a multi-GB model before load would warm
    // the OS page cache and make the reported cold-load phase meaningless.
    eprintln!("hashing model after timed phases...");
    let model = model_metadata(&input.args.model_path).map_err(|error| error.to_string())?;
    let report = BenchmarkReport {
        schema_version: SCHEMA_VERSION,
        benchmark: input.benchmark,
        generated_at_utc: chrono::Utc::now().to_rfc3339(),
        source: source_metadata(&input.args.source_label),
        harness_sha256: input.harness_sha256,
        production: input.production,
        backend: BackendMetadata {
            requested: input.args.backend,
            compiled_cpu: true,
            compiled_vulkan: cfg!(feature = "vulkan"),
            verified_actual: input.backend_observation.verified_actual,
            evidence: input.backend_observation.evidence,
        },
        hardware: hardware_metadata(&input.args.hardware_label),
        model,
        config: input.config,
        phases: input.phases,
        quality: input.quality,
        thresholds,
        passed,
    };
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    if let Some(path) = &input.args.json_path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, format!("{json}\n")).map_err(|error| error.to_string())?;
        eprintln!("wrote {}", path.display());
    }
    println!("{json}");
    Ok(passed)
}

fn clone_threshold(threshold: &ThresholdResult) -> ThresholdResult {
    ThresholdResult {
        metric: threshold.metric.clone(),
        operator: threshold.operator,
        expected: threshold.expected,
        actual: threshold.actual,
        passed: threshold.passed,
    }
}

fn default_latency_thresholds(input: &ReportInput) -> Vec<ThresholdResult> {
    let warm_p95 = input
        .phases
        .iter()
        .filter(|phase| phase.kind == "warm")
        .flat_map(|phase| phase.samples_ms.iter().copied())
        .collect::<Vec<_>>();
    let warm_p95 = summarize(&warm_p95).p95_ms;

    let mut thresholds = Vec::new();
    if let Some(maximum) = input.args.max_p95_ms {
        thresholds.push(ThresholdResult {
            metric: "warm_p95_ms".to_string(),
            operator: "<=",
            expected: maximum as f64,
            actual: warm_p95 as f64,
            passed: warm_p95 <= maximum,
        });
    }
    thresholds
}

pub fn run_or_exit(run: impl FnOnce() -> Result<bool, String>) {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("benchmark failed: {error}");
            std::process::exit(2);
        }
    }
}

pub fn elapsed_ms(start: std::time::Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub fn harness_sha256(parts: &[&[u8]]) -> String {
    let mut sha = Sha256::new();
    for part in parts {
        sha.update(part);
    }
    hex(&sha.finalize())
}

/// Fingerprint the production implementation exercised by a harness.
///
/// File labels and byte lengths are included in the digest so concatenation
/// boundaries are unambiguous. `include_bytes!` callers fingerprint exactly the
/// source compiled into the benchmark, even in a dirty worktree.
pub fn production_fingerprint(parts: &[(&str, &[u8])]) -> ProductionFingerprint {
    let mut sha = Sha256::new();
    for (path, bytes) in parts {
        sha.update(&(path.len() as u64).to_le_bytes());
        sha.update(path.as_bytes());
        sha.update(&(bytes.len() as u64).to_le_bytes());
        sha.update(bytes);
    }
    ProductionFingerprint {
        files: parts.iter().map(|(path, _)| (*path).to_string()).collect(),
        sha256: hex(&sha.finalize()),
    }
}

fn summarize(samples: &[u64]) -> LatencySummary {
    if samples.is_empty() {
        return LatencySummary {
            count: 0,
            min_ms: 0,
            max_ms: 0,
            mean_ms: 0.0,
            p50_ms: 0,
            p95_ms: 0,
        };
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let percentile = |pct: f64| {
        let rank = (pct * sorted.len() as f64).ceil() as usize;
        sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
    };
    let total = sorted.iter().map(|&value| value as u128).sum::<u128>();
    LatencySummary {
        count: sorted.len(),
        min_ms: sorted[0],
        max_ms: sorted[sorted.len() - 1],
        mean_ms: total as f64 / sorted.len() as f64,
        p50_ms: percentile(0.50),
        p95_ms: percentile(0.95),
    }
}

fn source_metadata(label: &str) -> SourceMetadata {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let revision =
        git_output(manifest_dir, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let dirty = git_output(manifest_dir, &["status", "--porcelain"])
        .map(|output| !output.is_empty())
        .unwrap_or(true);
    SourceMetadata {
        revision,
        dirty,
        label: label.to_string(),
    }
}

fn git_output(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn hardware_metadata(label: &str) -> HardwareMetadata {
    let cpu_identifier = std::env::var("PROCESSOR_IDENTIFIER")
        .or_else(|_| std::env::var("HOSTTYPE"))
        .unwrap_or_else(|_| "unknown".to_string());
    let gpu_identifier = std::env::var("OMNIVOX_GPU_IDENTIFIER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(detect_gpu_identifier)
        .unwrap_or_else(|| "unknown".to_string());
    HardwareMetadata {
        label: label.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        logical_cpus: std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1),
        cpu_identifier,
        gpu_identifier,
    }
}

fn detect_gpu_identifier() -> Option<String> {
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
        (
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join '; '",
            ],
        )
    } else {
        ("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"])
    };
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let identifier = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!identifier.is_empty()).then_some(identifier)
}

fn model_metadata(path: &Path) -> io::Result<ModelMetadata> {
    let (size_bytes, sha256) = if path.is_dir() {
        // Directory models: hash every file in a stable (name-sorted) order so
        // the digest identifies the whole artifact set reproducibly. Verification
        // marker files are skipped: their contents embed a machine-local mtime,
        // so including them would make identical model artifacts on two
        // machines hash differently.
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|entry| entry.is_file())
            .filter(|entry| {
                entry
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.ends_with(".omnivox-verified"))
            })
            .collect();
        files.sort();
        let mut size_bytes = 0_u64;
        let mut sha = Sha256::new();
        for file in &files {
            size_bytes += std::fs::metadata(file)?.len();
            sha.update(sha256_file(file)?.as_bytes());
        }
        (size_bytes, hex(&sha.finalize()))
    } else {
        (std::fs::metadata(path)?.len(), sha256_file(path)?)
    };
    Ok(ModelMetadata {
        filename: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
        size_bytes,
        sha256,
    })
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut sha = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha.update(&buffer[..read]);
    }
    Ok(hex(&sha.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    length_bytes: u64,
}

impl Sha256 {
    fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            length_bytes: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.length_bytes = self.length_bytes.wrapping_add(input.len() as u64);
        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&input[..take]);
            self.buffer_len += take;
            input = &input[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            } else {
                // The new input was fully appended to the partial block. Keep
                // its accumulated length; resetting to `input.len()` below
                // would discard the earlier bytes from a multi-part hash.
                return;
            }
        }
        while input.len() >= 64 {
            let block: &[u8; 64] = input[..64].try_into().expect("64-byte chunk");
            self.compress(block);
            input = &input[64..];
        }
        self.buffer[..input.len()].copy_from_slice(input);
        self.buffer_len = input.len();
    }

    fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.length_bytes.wrapping_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer = [0; 64];
            self.buffer_len = 0;
        }
        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);

        let mut output = [0u8; 32];
        for (chunk, word) in output.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        output
    }

    fn compress(&mut self, block: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (index, chunk) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes(chunk.try_into().expect("four-byte word"));
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            harness_sha256(&[b""]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            harness_sha256(&[b"abc"]),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            harness_sha256(&[b"a", b"b", b"c"]),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn latency_summary_uses_nearest_rank_percentiles() {
        let summary = summarize(&[10, 30, 20, 100]);
        assert_eq!(summary.p50_ms, 20);
        assert_eq!(summary.p95_ms, 100);
        assert_eq!(summary.min_ms, 10);
        assert_eq!(summary.max_ms, 100);
    }

    #[test]
    fn production_fingerprint_records_labels_and_unambiguous_boundaries() {
        let fingerprint = production_fingerprint(&[("a.rs", b"ab"), ("b.rs", b"c")]);
        let regrouped = production_fingerprint(&[("a.rs", b"a"), ("b.rs", b"bc")]);
        assert_eq!(fingerprint.files, ["a.rs", "b.rs"]);
        assert_ne!(fingerprint.sha256, regrouped.sha256);
    }
}
