# OmniVox

Local-first dictation and meeting notes. Audio stays on your computer; optional
meeting intelligence can send transcript text to a model you choose through
your own OpenRouter account.

Developer guidance for measuring local ASR, Command Mode, and Structured Mode
latency is in [docs/BENCHMARKING.md](docs/BENCHMARKING.md).

## Dictation

Speech recognition runs entirely on-device. Two engine families are available
side by side on **Models > Speech Recognition**:

- **Whisper** (default) — multiple sizes from Tiny to Large V3 Turbo, including
  **Distil Large V3.5**, GPU-accelerated via Vulkan, multilingual options, and
  conditioned by your custom vocabulary, dictionary, and Screen Context.
- **Parakeet TDT 0.6B V2** — NVIDIA's transducer engine. Punctuates and
  capitalizes natively and does not hallucinate on silence. English-only,
  CPU-only, and not conditioned by vocabulary prompts or Screen Context
  (dictionary text replacement and phonetic correction still apply).

**AI Transcript Cleanup** (Models > Cleanup, off by default) runs dictations
through **S1-mini by Superwhisper**, a small local model that removes fillers,
resolves spoken self-corrections, and normalizes punctuation, numbers, dates,
and email addresses — entirely on CPU, on your device.

## Meeting notes

OmniVox can capture the microphone and Windows system output for Google Meet,
Zoom, Skype, Teams, and other virtual meetings. It writes recoverable
20-second audio chunks, transcribes them locally with Whisper, and removes each
chunk after successful transcription. Deferred transcription keeps Whisper
idle during the call; live transcription is available when immediate text is
more important than call-time CPU usage.

The meeting workspace includes:

- rough notes, searchable transcripts, markers, speaker labels, tags, and favorites;
- structured summaries, decisions, action items, and grounded follow-up questions;
- bounded local notes-version history with automatic hand-edit snapshots and safe restore;
- timestamp-linked evidence and bounded local retrieval for long meetings;
- polished-note exports for sharing, explicitly labeled full Markdown/JSON archives,
  and WebVTT/SRT transcript captions;
- a searchable, owner-filterable, due-aware cross-meeting follow-up inbox with checklist copy, source-transcript jumps, and recurring-meeting carry-forward as editable tasks;
- durable follow-ups that can be added, edited, assigned, dated, completed, or dismissed without regenerating notes;
- an in-app recovery center that automatically resumes sealed local audio, offers batch transcription retry, and never silently repeats a potentially billed AI request;
- reusable meeting setups for title, agenda, participants, and AI options; and
- per-meeting model, summary-style, and instruction overrides.

### Optional OpenRouter boundary

Cloud intelligence is an add-on and is inactive until an OpenRouter API key is
entered inside OmniVox. The key is stored in Windows Credential Manager, not in
the application database. Audio is never uploaded. When a summary or meeting
question is requested, OmniVox sends the necessary transcript text plus the
meeting title, agenda, participants, markers, and user notes.

Before every paid request, OmniVox verifies model availability, structured
output support, context capacity, current prices, the per-meeting limit, and
the monthly local guardrail. The in-app catalog provides duration-aware cost
planning, context comparison, price-limit filtering, key usage, and remaining
credit information. The catalog can compare broad benchmark quality and
quality-per-dollar alongside cost, and provider routing can prioritize lowest
cost, fastest output, fastest response start, or OpenRouter's balanced default.
Actual billing and adding credits remain with OpenRouter.

Polished-note exports contain the meeting title, date, participants, agenda,
and edited AI notes only. Raw transcript text, rough notes, markers, private
follow-up Q&A, provider/model details, and cost data remain in the explicitly
labeled full archives.

## Privacy & retention

On new installations, local transcript history is retained for 30 days by
default; existing installations keep their previously saved retention policy.
Permanent audio recordings are not retained. Recoverable meeting chunks exist
only until local transcription succeeds. History and retention can be managed
under **Settings > Privacy**.

## License

OmniVox is **source-available, not open source.**

The source code is published so you can inspect what runs locally, verify the
meeting-data boundary described above, and allow the installer to fetch it.

**You may** use OmniVox for personal purposes at no charge, inspect the source
code, and make backups for personal use.

**You may not** redistribute, modify, or use OmniVox commercially without a
separate license. See [LICENSE](LICENSE) for the full terms.

For commercial licensing inquiries: **licensing@omniimpact.com**
