/// GBNF grammar that constrains the LLM output to the exact slot-JSON shape.
///
/// Embedded at compile time so builds are self-contained — installing OmniVox
/// never requires a grammar file on disk.  The Rust side decides what JSON
/// shape it will accept; the LLM physically cannot emit anything else.
pub const SLOT_EXTRACTION_V1: &str =
    include_str!("../../resources/grammars/slot_extraction_v1.gbnf");

/// Grammar root symbol — matches the `root ::=` rule in the GBNF file.
pub const SLOT_EXTRACTION_ROOT: &str = "root";
