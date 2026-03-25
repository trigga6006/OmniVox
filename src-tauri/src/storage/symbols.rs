use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::dictionary;

/// Seed common programming symbol dictionary entries.
/// These convert spoken words like "slash" to their symbol equivalents.
/// Only inserts entries that don't already exist (by phrase match).
pub fn seed_programming_symbols(db: &Database) -> AppResult<()> {
    let conn = db.conn()?;

    // Check if we've already seeded by looking for a sentinel entry
    let already_seeded: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM dictionary_entries WHERE phrase = 'slash' AND mode_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if already_seeded {
        return Ok(());
    }

    let symbols: &[(&str, &str)] = &[
        // Punctuation & common symbols
        ("slash", "/"),
        ("forward slash", "/"),
        ("backslash", "\\"),
        ("back slash", "\\"),
        ("dash", "-"),
        ("hyphen", "-"),
        ("double dash", "--"),
        ("underscore", "_"),
        ("dot", "."),
        ("period", "."),
        ("comma", ","),
        ("colon", ":"),
        ("double colon", "::"),
        ("semicolon", ";"),
        ("semi colon", ";"),
        // Brackets and parens
        ("open paren", "("),
        ("close paren", ")"),
        ("open parenthesis", "("),
        ("close parenthesis", ")"),
        ("left paren", "("),
        ("right paren", ")"),
        ("open bracket", "["),
        ("close bracket", "]"),
        ("left bracket", "["),
        ("right bracket", "]"),
        ("open curly", "{"),
        ("close curly", "}"),
        ("open brace", "{"),
        ("close brace", "}"),
        ("left curly", "{"),
        ("right curly", "}"),
        ("left brace", "{"),
        ("right brace", "}"),
        ("open angle", "<"),
        ("close angle", ">"),
        ("left angle", "<"),
        ("right angle", ">"),
        ("less than", "<"),
        ("greater than", ">"),
        // Quotes
        ("single quote", "'"),
        ("double quote", "\""),
        ("backtick", "`"),
        ("back tick", "`"),
        ("triple backtick", "```"),
        // Operators & symbols
        ("equals", "="),
        ("equal sign", "="),
        ("double equals", "=="),
        ("triple equals", "==="),
        ("not equals", "!="),
        ("plus", "+"),
        ("plus sign", "+"),
        ("minus", "-"),
        ("minus sign", "-"),
        ("asterisk", "*"),
        ("star", "*"),
        ("double star", "**"),
        ("ampersand", "&"),
        ("double ampersand", "&&"),
        ("pipe", "|"),
        ("double pipe", "||"),
        ("exclamation", "!"),
        ("exclamation mark", "!"),
        ("bang", "!"),
        ("question mark", "?"),
        ("at sign", "@"),
        ("at symbol", "@"),
        ("hash", "#"),
        ("hash sign", "#"),
        ("pound sign", "#"),
        ("dollar sign", "$"),
        ("percent", "%"),
        ("percent sign", "%"),
        ("caret", "^"),
        ("tilde", "~"),
        // Arrows & compound
        ("arrow", "->"),
        ("fat arrow", "=>"),
        ("double arrow", "=>"),
        ("ellipsis", "..."),
        ("spread operator", "..."),
        ("null coalescing", "??"),
        ("optional chaining", "?."),
    ];

    for (phrase, replacement) in symbols {
        dictionary::add_entry(db, phrase, replacement, None)?;
    }

    Ok(())
}
