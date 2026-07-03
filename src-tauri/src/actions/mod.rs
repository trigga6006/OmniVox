//! Command Mode — speak a command, perform an OS action.
//!
//! This is a separate path from dictation.  In Command Mode the *whole*
//! utterance is one command: it is matched by [`matcher::match_command`] into a
//! closed [`intent::CommandIntent`], then run by [`executor`].  App launching is
//! backed by the Windows AppsFolder index in [`app_index`].
//!
//! The closed enum is the safety boundary — the matcher (and, later, the LLM
//! fallback) can only ever produce a verb that exists in [`intent::CommandIntent`],
//! so a spoken phrase can never invent a destructive action.

pub mod app_index;
pub mod executor;
pub mod intent;
pub mod matcher;

pub use intent::{CommandIntent, KeyChord, MediaAction, WindowAction};
pub use matcher::match_command;
