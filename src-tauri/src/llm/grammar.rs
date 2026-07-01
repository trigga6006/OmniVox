/// GBNF grammar that constrains the LLM output to the exact slot-JSON shape.
///
/// Embedded at compile time so builds are self-contained — installing OmniVox
/// never requires a grammar file on disk.  The Rust side decides what JSON
/// shape it will accept; the LLM physically cannot emit anything else.
pub const SLOT_EXTRACTION_V1: &str =
    include_str!("../../resources/grammars/slot_extraction_v1.gbnf");

/// Grammar root symbol — matches the `root ::=` rule in the GBNF file.
pub const SLOT_EXTRACTION_ROOT: &str = "root";

/// GBNF grammar for Command Mode's free-form fallback: constrains Qwen to emit
/// a JSON array of `{"action":<closed enum>,"target":<string>}` objects so a
/// natural-language command maps to an ordered sequence of allowed actions —
/// one step ("bring up spotify") or several ("open spotify and play it").
pub const COMMAND_INTENT_V1: &str =
    include_str!("../../resources/grammars/command_intent_v1.gbnf");

/// Root symbol for [`COMMAND_INTENT_V1`].
pub const COMMAND_INTENT_ROOT: &str = "root";
