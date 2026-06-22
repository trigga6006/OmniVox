export const meta = {
  name: 'omnivox-parakeet-research',
  description: 'Map the OmniVox codebase, then deep-research NVIDIA Parakeet + local voice control, and synthesize a Jarvis-mode integration proposal',
  phases: [
    { title: 'Codebase Map', detail: 'parallel readers over each Rust/React subsystem' },
    { title: 'Research', detail: 'multi-angle web research on Parakeet + local voice control' },
    { title: 'Verify', detail: 'adversarially verify decision-critical claims' },
    { title: 'Synthesize', detail: 'tie research to the codebase into a feature proposal' },
  ],
}

// ---------- Schemas ----------
const CODEBASE_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    subsystem: { type: 'string' },
    purpose: { type: 'string' },
    key_files: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        properties: { path: { type: 'string' }, role: { type: 'string' } },
        required: ['path', 'role'],
      },
    },
    how_it_works: { type: 'string', description: 'Concrete data flow / control flow, function names, key types' },
    voice_control_relevance: { type: 'string', description: 'How this subsystem helps or blocks a voice-command/Jarvis mode' },
    extension_points: { type: 'string', description: 'Exact seams where new behavior would hook in' },
  },
  required: ['subsystem', 'purpose', 'key_files', 'how_it_works', 'voice_control_relevance', 'extension_points'],
}

const RESEARCH_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    dimension: { type: 'string' },
    summary: { type: 'string' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        properties: {
          claim: { type: 'string' },
          detail: { type: 'string' },
          sources: { type: 'array', items: { type: 'string' } },
          confidence: { type: 'string', enum: ['high', 'medium', 'low'] },
        },
        required: ['claim', 'detail', 'sources', 'confidence'],
      },
    },
    critical_claims: { type: 'array', items: { type: 'string' }, description: 'The 1-2 claims most load-bearing for a build/no-build decision' },
    recommendation: { type: 'string' },
  },
  required: ['dimension', 'summary', 'findings', 'critical_claims', 'recommendation'],
}

const VERDICT_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    claim: { type: 'string' },
    verdict: { type: 'string', enum: ['confirmed', 'refuted', 'uncertain'] },
    evidence: { type: 'string' },
    sources: { type: 'array', items: { type: 'string' } },
    correction: { type: 'string', description: 'If refuted/uncertain, the corrected statement' },
  },
  required: ['claim', 'verdict', 'evidence'],
}

// ---------- Phase 1: Codebase Map ----------
phase('Codebase Map')

const ROOT = 'C:/Users/fowle/Documents/dev/OmniVox'
const readBase = (name, files, focus) =>
  'You are mapping ONE subsystem of OmniVox, a local AI dictation desktop app (Tauri 2 = Rust backend + React 19/TS frontend, on Windows).\n\n' +
  'Subsystem: ' + name + '\n' +
  'Read these files under ' + ROOT + ' (and any siblings/imports you need):\n' + files.map(f => '  - ' + f).join('\n') + '\n\n' +
  'Focus: ' + focus + '\n\n' +
  'Read the ACTUAL code — do not guess. Report concrete function names, types, events (Tauri emit/listen names), Tauri commands, and the real control/data flow. ' +
  'In voice_control_relevance, assess specifically how this subsystem would support or obstruct a new "voice command / Jarvis" mode where the user speaks a command (e.g. open Spotify, new line, send) and the app performs an OS action. ' +
  'In extension_points, name the exact file:function seams where new code would attach. Be specific and terse.'

const READERS = [
  { key: 'asr', name: 'ASR / Whisper engine + model management',
    files: ['src-tauri/src/asr/engine.rs', 'src-tauri/src/asr/mod.rs', 'src-tauri/src/asr/types.rs', 'src-tauri/src/models/manager.rs', 'src-tauri/src/models/downloader.rs', 'src-tauri/src/models/types.rs', 'src-tauri/src/commands/models.rs'],
    focus: 'How audio becomes text: whisper-rs usage, model loading/lifecycle, GPU (Vulkan/CUDA) selection, the live-preview/streaming path, initial-prompt/vocabulary injection, and how hard it would be to swap or add a DIFFERENT ASR backend (e.g. Parakeet) behind the same interface. Is there an AsrEngine trait/abstraction or is whisper hardcoded?' },
  { key: 'audio', name: 'Audio capture pipeline',
    files: ['src-tauri/src/audio/capture.rs', 'src-tauri/src/audio/mod.rs', 'src-tauri/src/audio/types.rs', 'src-tauri/src/audio/denoise.rs', 'src-tauri/src/audio/normalize.rs', 'src-tauri/src/audio/resample.rs', 'src-tauri/src/audio/ducking.rs'],
    focus: 'cpal capture, sample rate/format, buffering, snapshot_tail, RMS levels, denoise (RNNoise), normalization, resampling, ducking. Whether the capture design could support always-on/streaming listening + VAD for wake-word/continuous command mode rather than only push-to-talk.' },
  { key: 'llm', name: 'Local LLM / Structured Mode',
    files: ['src-tauri/src/llm/engine.rs', 'src-tauri/src/llm/runner.rs', 'src-tauri/src/llm/schema.rs', 'src-tauri/src/llm/voxify.rs', 'src-tauri/src/llm/prompt.rs', 'src-tauri/src/llm/template.rs', 'src-tauri/src/llm/grammar.rs', 'src-tauri/src/llm/mod.rs', 'src-tauri/src/llm/types.rs', 'src-tauri/src/llm_models/manager.rs', 'src-tauri/src/llm_models/mod.rs', 'src-tauri/src/commands/llm.rs'],
    focus: 'llama-cpp-2 usage: model load/lifecycle, prompt templating, GBNF/grammar-constrained decoding, JSON slot extraction schema, the Voxify trigger word gate, timeouts. This is the key reusable engine for natural-language to structured COMMAND intent (tool/function-calling style). Assess fitness for intent routing.' },
  { key: 'postprocess', name: 'Post-processing + EXISTING voice commands',
    files: ['src-tauri/src/postprocess/voice_commands.rs', 'src-tauri/src/postprocess/processor.rs', 'src-tauri/src/postprocess/phonetic.rs', 'src-tauri/src/postprocess/formatter.rs', 'src-tauri/src/postprocess/mod.rs', 'src-tauri/src/postprocess/types.rs'],
    focus: 'THE MOST IMPORTANT subsystem for this idea. Exactly how voice_commands.rs parses transcript into Text|Command segments today, what commands exist, how it splits/matches, the dictionary/phonetic correction, formatter. This is the existing seam a Jarvis command grammar would extend or sit beside.' },
  { key: 'output', name: 'Output routing, focus, hotkeys',
    files: ['src-tauri/src/output/router.rs', 'src-tauri/src/output/mod.rs', 'src-tauri/src/output/types.rs', 'src-tauri/src/focus.rs', 'src-tauri/src/hotkey.rs'],
    focus: 'How text/keystrokes reach the OS: enigo type-simulation, arboard clipboard, segment sending, Ship Mode Enter. focus.rs foreground-window capture/restore (HWND, process name). hotkey.rs global keyboard hook. These are the OS-action primitives a Jarvis mode would reuse to launch apps / press keys / control windows.' },
  { key: 'screen', name: 'Screen context capture',
    files: ['src-tauri/src/screen_context/mod.rs', 'src-tauri/src/screen_context/extract.rs', 'src-tauri/src/screen_context/windows.rs'],
    focus: 'UI Automation (UIA) tree walk to capture foreground window text/tokens + source app. How tokens feed Whisper prompt + LLM. Relevance: this same UIA capability could let a Jarvis mode SEE on-screen elements to click/act on them.' },
  { key: 'core', name: 'App core: state, storage, commands, settings, context modes',
    files: ['src-tauri/src/state.rs', 'src-tauri/src/lib.rs', 'src-tauri/src/main.rs', 'src-tauri/src/error.rs', 'src-tauri/src/storage/mod.rs', 'src-tauri/src/storage/settings.rs', 'src-tauri/src/storage/context_modes.rs', 'src-tauri/src/storage/app_bindings.rs', 'src-tauri/src/commands/mod.rs', 'src-tauri/src/commands/settings.rs', 'src-tauri/src/commands/context_modes.rs'],
    focus: 'AppState shape, the SQLite storage layer, settings schema (every relevant toggle), context modes + per-app bindings + auto-switch, how Tauri commands are registered (invoke_handler in lib.rs), error model. Where a new command mode setting + command registry would live.' },
  { key: 'frontend', name: 'React/TS frontend',
    files: ['src/App.tsx', 'src/overlay-main.tsx', 'src/features/overlay/FloatingPill.tsx', 'src/features/overlay/ModeSelector.tsx', 'src/features/overlay/StructuredPanel.tsx', 'src/features/settings/SettingsPage.tsx', 'src/features/modes/ContextModesPage.tsx', 'src/stores/recordingStore.ts', 'src/stores/settingsStore.ts', 'src/lib/tauri.ts', 'src/hooks/useTauriEvent.ts'],
    focus: 'Overall UI: main window vs overlay/floating pill, how the frontend invokes Tauri commands + listens to events, recording state machine, settings UI patterns, mode selector. Where a Command mode toggle/UI + a command-feedback surface would slot in.' },
]

const codebase = (await parallel(
  READERS.map(r => () => agent(readBase(r.name, r.files, r.focus), {
    label: 'map:' + r.key, phase: 'Codebase Map', schema: CODEBASE_SCHEMA,
  }))
)).filter(Boolean)

log('Codebase map complete: ' + codebase.length + '/' + READERS.length + ' subsystems')

// ---------- Phase 2 + 3: Research (pipelined into Verify) ----------
phase('Research')

const TODAY = 'June 2026'
const researchPrompt = (d) =>
  'You are a meticulous technical researcher. Today is ' + TODAY + '. Research this dimension for a SOLO DEVELOPER building a LOCAL (offline, privacy-first) voice-control / Jarvis feature into an existing Windows desktop app written in Rust (Tauri 2). The app already does local Whisper dictation and runs a small local LLM (Qwen via llama.cpp).\n\n' +
  'DIMENSION: ' + d.title + '\n' + d.brief + '\n\n' +
  'Method: FIRST call ToolSearch with query "select:WebSearch,WebFetch" to load the web tools. Then run multiple searches, open the primary/authoritative sources (Hugging Face model cards, official NVIDIA/NeMo docs, GitHub READMEs, benchmark leaderboards, project docs), and FETCH them — do not rely on snippets alone. Prefer 2025-2026 information; note when something may be stale.\n\n' +
  'Be concrete and quantitative: model variants + sizes, license (exact license name + commercial-use terms), WER %, RTFx / real-time factor, latency, VRAM/RAM needs, language support, streaming support, CPU-only feasibility. ALWAYS attach source URLs to each finding. Mark confidence honestly. In critical_claims, list the 1-2 facts that most determine whether this is buildable/worthwhile.'

const DIMENSIONS = [
  { key: 'parakeet-specs', title: 'NVIDIA Parakeet model family — specs, accuracy, license',
    brief: 'Enumerate current Parakeet variants (e.g. parakeet-tdt-0.6b-v2/v3, parakeet-ctc, parakeet-rnnt, parakeet-tdt-1.1b) and how they relate to Canary. For each: model size/params, architecture (TDT/CTC/RNNT/FastConformer), accuracy on Open ASR Leaderboard (WER), RTFx/speed, languages, max audio length, punctuation/capitalization/timestamps. CRITICAL: exact license (is it CC-BY-4.0? commercially usable?) and where weights are hosted.' },
  { key: 'parakeet-local-rust', title: 'Running Parakeet locally on Windows, ideally from Rust/Tauri',
    brief: 'The hard practical question. Options to run Parakeet OFFLINE on Windows: NVIDIA NeMo (Python, heavy), ONNX export + onnxruntime, sherpa-onnx (C API + bindings, does it support Parakeet? Rust bindings?), parakeet-mlx (Mac-only?), Rust-native crates (parakeet-rs, transcribe-rs, candle-based), whisper.cpp-style ports. For each: does it run on Windows? CPU-only or GPU-required? Is there a Rust path (crate or C-FFI) that a Tauri app could embed WITHOUT shipping a Python runtime? Model download size, first-run setup. This determines feasibility for this specific app.' },
  { key: 'parakeet-vs-whisper', title: 'Parakeet vs Whisper for dictation AND for low-latency command recognition',
    brief: 'Head-to-head: accuracy, speed/RTFx, latency, streaming/partial results, punctuation, multilingual coverage, robustness to short utterances and commands. Which is better for (a) long-form dictation and (b) fast short voice COMMANDS? Note Parakeet English-mostly vs Whisper multilingual tradeoff, and any quality regressions on short/noisy input.' },
  { key: 'voice-arch', title: 'Local voice-assistant architecture: wake word, VAD, always-on listening',
    brief: 'How to build an always-listening or wake-word-triggered local pipeline. Wake-word engines (openWakeWord, Picovoice Porcupine — license/cost, Rust support), VAD (Silero VAD, webrtcvad — Rust crates), push-to-talk vs wake-word vs continuous, endpointing, latency budgets for feeling responsive. Privacy implications of always-on mic. Practical patterns + any Rust crates.' },
  { key: 'existing-projects', title: 'Existing local voice-control / computer-control projects to learn from',
    brief: 'Survey: Talon Voice (voice control of the computer — command grammars, accessibility), Home Assistant Assist / Wyoming / Willow, Rhasspy, Leon, numen, Serenade, and any Whisper/Parakeet-based command tools. For each: how they map speech to OS/app actions, command-grammar vs LLM-intent approach, what works, what is hard. Extract transferable design patterns.' },
  { key: 'os-automation', title: 'Windows OS automation from Rust: launch apps, control windows, click UI',
    brief: 'How to perform actions from a Rust/Tauri app on Windows: launch applications (open Spotify -> resolve + start: ShellExecute, start menu / AppUserModelID / UWP, PATH lookup), focus/minimize/close windows, simulate keys (enigo already used), type/paste, and click on-screen elements via UI Automation. Relevant crates (windows-rs, enigo). Permissions/UAC, reliability, and safety concerns of programmatic OS control.' },
  { key: 'intent-routing', title: 'Speech to structured command intent with a small local LLM',
    brief: 'Turning a natural-language utterance into a structured action (intent + slots / function call) using a SMALL local LLM (the app already runs Qwen via llama.cpp). Compare: rigid grammar/regex command matching vs LLM tool/function-calling vs GBNF grammar-constrained JSON decoding (llama.cpp supports GBNF). Latency, reliability, hallucination/safety, confirmation UX. How to define a command registry/ontology. Best practice for hybrid (fast-path grammar + LLM fallback).' },
]

const researched = (await pipeline(
  DIMENSIONS,
  d => agent(researchPrompt(d), { label: 'research:' + d.key, phase: 'Research', schema: RESEARCH_SCHEMA }),
  (res, d) => {
    if (!res) return null
    const claims = (res.critical_claims || []).slice(0, 2)
    if (!claims.length) return Object.assign({}, res, { verdicts: [] })
    return parallel(claims.map(c => () => agent(
      'You are an adversarial fact-checker. Today is ' + TODAY + '. A researcher claimed the following about local voice tech for a Rust/Windows app:\n\n"' + c + '"\n\n' +
      'FIRST call ToolSearch query "select:WebSearch,WebFetch" to load web tools. Then INDEPENDENTLY verify by finding authoritative primary sources (official docs, model cards, repo code/README). Try to REFUTE it. Common failure modes: outdated license info, conflating model variants, optimistic latency/WER under unrealistic conditions, claiming Windows/Rust support that is actually Mac-only or Python-only. Give a verdict with evidence and source URLs; if wrong or shaky, provide the correction.',
      { label: 'verify:' + d.key, phase: 'Verify', schema: VERDICT_SCHEMA }
    ))).then(vs => Object.assign({}, res, { verdicts: vs.filter(Boolean) }))
  }
)).filter(Boolean)

log('Research + verification complete: ' + researched.length + '/' + DIMENSIONS.length + ' dimensions')

// ---------- Phase 4: Synthesize ----------
phase('Synthesize')

const proposal = await agent(
  'You are a principal engineer writing a feature design memo for the SOLO developer of OmniVox (local Whisper dictation app, Tauri 2 = Rust + React, Windows-first, privacy/local-first). ' +
  'The developer wants to add a local Jarvis-style VOICE CONTROL mode (speak commands to open apps, control the computer, do assistant tasks), and is specifically interested in NVIDIA Parakeet as the ASR.\n\n' +
  'You are given (A) a map of the EXISTING codebase and (B) verified research. Ground EVERY architectural claim in the real codebase map — cite actual file paths and function/seam names from it. Where research was refuted/uncertain, use the correction, not the original claim.\n\n' +
  '=== (A) CODEBASE MAP ===\n' + JSON.stringify(codebase, null, 1) + '\n\n' +
  '=== (B) RESEARCH + VERDICTS ===\n' + JSON.stringify(researched, null, 1) + '\n\n' +
  'Write a thorough but tight memo with these sections:\n' +
  '1. Reality check on Parakeet for THIS app — should the ASR even change? (Parakeet vs keeping Whisper; the Rust/Windows-local runnability verdict is decisive — be honest if it is painful). Give a clear recommendation.\n' +
  '2. The actual hard part — argue that ASR is the easy 20%; the assistant/command layer (wake/trigger, intent routing, action execution, safety/confirmation) is the real work. Map it to existing seams (voice_commands.rs, the Qwen llama.cpp runner + GBNF grammar, enigo/output router, focus.rs, screen_context UIA).\n' +
  '3. Proposed architecture — a Command Mode that reuses existing primitives. Trigger model (push-to-talk hotkey reusing hotkey.rs vs wake word), speech-to-intent (hybrid: fast grammar fast-path + GBNF-constrained Qwen fallback), an action/command registry, execution via existing OS primitives, and a confirmation/undo UX in the overlay. Include a concrete data-flow walkthrough for "open Spotify" and for "send this email".\n' +
  '4. Phased delivery — an MVP that ships in days reusing what exists, then increments. Be specific about which files change.\n' +
  '5. Risks, safety, and open questions the developer must decide.\n' +
  'Be concrete, opinionated, and honest about effort. Prefer reusing the existing local Qwen LLM over adding new ML deps.',
  { label: 'synthesize:memo', phase: 'Synthesize', effort: 'high' }
)

return { codebase, researched, proposal }
