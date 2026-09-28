use std::fmt;
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use thiserror::Error;

use crate::storage::atomic::{atomic_write_str, AtomicError};

/// Errors that can occur during VDF parsing or serialization.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum VdfError {
    #[error("Parse error at line {line}, col {col}: {message}")]
    ParseError {
        line: usize,
        col: usize,
        message: String,
    },
    #[error("Unterminated string starting at line {line}, col {col}")]
    UnterminatedString { line: usize, col: usize },
    #[error("Unexpected token at line {line}, col {col}: found '{found}', expected {expected}")]
    UnexpectedToken {
        line: usize,
        col: usize,
        found: String,
        expected: String,
    },
    #[error("Unexpected end of file, expected {expected}")]
    UnexpectedEof { expected: String },
    #[error("I/O error reading VDF file: {0}")]
    Io(String),
    #[error("Atomic write error: {0}")]
    Atomic(String),
}

impl From<AtomicError> for VdfError {
    fn from(err: AtomicError) -> Self {
        Self::Atomic(err.to_string())
    }
}

/// A value in a Valve KeyValues / VDF document: either a string or a nested table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    Str(String),
    Obj(VdfTable),
}

impl VdfValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            Self::Obj(_) => None,
        }
    }

    pub fn as_obj(&self) -> Option<&VdfTable> {
        match self {
            Self::Obj(t) => Some(t),
            Self::Str(_) => None,
        }
    }

    pub fn as_obj_mut(&mut self) -> Option<&mut VdfTable> {
        match self {
            Self::Obj(t) => Some(t),
            Self::Str(_) => None,
        }
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Self::Str(_))
    }

    pub fn is_obj(&self) -> bool {
        matches!(self, Self::Obj(_))
    }
}

/// An ordered collection of key-value pairs representing a VDF block or document root.
/// Preserves insertion order and provides case-insensitive lookup to match Valve Steam standards.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VdfTable {
    pub entries: Vec<(String, VdfValue)>,
}

impl VdfTable {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Case-insensitive lookup of a value by key.
    pub fn get(&self, key: &str) -> Option<&VdfValue> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// Case-sensitive lookup of a value by key.
    pub fn get_exact(&self, key: &str) -> Option<&VdfValue> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Case-insensitive mutable lookup of a value by key.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut VdfValue> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// Retrieves string value by key (case-insensitive).
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_str())
    }

    /// Retrieves nested table reference by key (case-insensitive).
    pub fn get_obj(&self, key: &str) -> Option<&VdfTable> {
        self.get(key).and_then(|v| v.as_obj())
    }

    /// Retrieves nested table mutable reference by key (case-insensitive).
    pub fn get_obj_mut(&mut self, key: &str) -> Option<&mut VdfTable> {
        self.get_mut(key).and_then(|v| v.as_obj_mut())
    }

    /// Sets or updates a key-value entry (case-insensitive match for existing keys).
    pub fn set<K: Into<String>>(&mut self, key: K, value: VdfValue) {
        let key_str = key.into();
        if let Some((_, existing)) = self
            .entries
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(&key_str))
        {
            *existing = value;
        } else {
            self.entries.push((key_str, value));
        }
    }

    /// Sets a string key-value entry.
    pub fn set_str<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.set(key, VdfValue::Str(value.into()));
    }

    /// Sets a nested table entry.
    pub fn set_obj<K: Into<String>>(&mut self, key: K, obj: VdfTable) {
        self.set(key, VdfValue::Obj(obj));
    }

    /// Removes an entry by key (case-insensitive).
    pub fn remove(&mut self, key: &str) -> Option<VdfValue> {
        if let Some(pos) = self
            .entries
            .iter()
            .position(|(k, _)| k.eq_ignore_ascii_case(key))
        {
            Some(self.entries.remove(pos).1)
        } else {
            None
        }
    }

    /// Checks if a key exists (case-insensitive).
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(key))
    }

    /// Iterates over all key-value pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &VdfValue)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Iterates mutably over all key-value pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut VdfValue)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    /// Navigates a nested path of keys case-insensitively (e.g. `&["Software", "Valve", "Steam"]`).
    pub fn find_path(&self, path: &[&str]) -> Option<&VdfValue> {
        if path.is_empty() {
            return None;
        }
        let mut curr_val = self.get(path[0])?;
        for &segment in &path[1..] {
            match curr_val {
                VdfValue::Obj(tbl) => {
                    curr_val = tbl.get(segment)?;
                }
                VdfValue::Str(_) => return None,
            }
        }
        Some(curr_val)
    }

    /// Navigates a nested path of keys to find a nested table.
    pub fn find_obj_path(&self, path: &[&str]) -> Option<&VdfTable> {
        if path.is_empty() {
            return Some(self);
        }
        match self.find_path(path)? {
            VdfValue::Obj(tbl) => Some(tbl),
            VdfValue::Str(_) => None,
        }
    }

    /// Navigates a nested path of keys to find a mutable nested table.
    pub fn find_obj_path_mut(&mut self, path: &[&str]) -> Option<&mut VdfTable> {
        if path.is_empty() {
            return Some(self);
        }
        let mut curr_tbl = self;
        for &segment in path {
            let next = curr_tbl.get_mut(segment)?;
            match next {
                VdfValue::Obj(ref mut tbl) => {
                    curr_tbl = tbl;
                }
                VdfValue::Str(_) => return None,
            }
        }
        Some(curr_tbl)
    }

    /// Ensures that nested tables exist along the specified path, creating them if needed.
    pub fn ensure_obj_path_mut(&mut self, path: &[&str]) -> &mut VdfTable {
        let mut curr = self;
        for &segment in path {
            let exists = curr
                .entries
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case(segment) && v.is_obj());
            if !exists {
                if let Some((_, val)) = curr
                    .entries
                    .iter_mut()
                    .find(|(k, _)| k.eq_ignore_ascii_case(segment))
                {
                    *val = VdfValue::Obj(VdfTable::new());
                } else {
                    curr.entries
                        .push((segment.to_string(), VdfValue::Obj(VdfTable::new())));
                }
            }
            let pos = curr
                .entries
                .iter()
                .position(|(k, v)| k.eq_ignore_ascii_case(segment) && v.is_obj())
                .unwrap();
            match &mut curr.entries[pos].1 {
                VdfValue::Obj(ref mut next_tbl) => {
                    curr = next_tbl;
                }
                _ => unreachable!(),
            }
        }
        curr
    }

    /// Sets a string value at a nested path, creating intermediate tables as needed.
    pub fn set_path_value(&mut self, path: &[&str], key: &str, value: &str) {
        let target_tbl = self.ensure_obj_path_mut(path);
        target_tbl.set_str(key, value);
    }

    /// Retrieves a string value at a nested path.
    pub fn get_path_str(&self, path: &[&str], key: &str) -> Option<&str> {
        let target_tbl = self.find_obj_path(path)?;
        target_tbl.get_str(key)
    }
}

/// A parsed Valve KeyValues / VDF document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VdfDocument {
    pub root: VdfTable,
}

impl Deref for VdfDocument {
    type Target = VdfTable;

    fn deref(&self) -> &Self::Target {
        &self.root
    }
}

impl DerefMut for VdfDocument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.root
    }
}

impl fmt::Display for VdfDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_vdf_string())
    }
}

impl VdfDocument {
    pub fn new() -> Self {
        Self {
            root: VdfTable::new(),
        }
    }

    /// Parses a VDF document from a string slice.
    pub fn parse(input: &str) -> Result<Self, VdfError> {
        let tokens = tokenize(input)?;
        let mut pos = 0;
        let root = parse_table(&tokens, &mut pos, true)?;
        Ok(Self { root })
    }

    /// Reads and parses a VDF document from a file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, VdfError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .map_err(|e| VdfError::Io(format!("{}: {}", path.display(), e)))?;
        Self::parse(&content)
    }

    /// Serializes the document into a formatted VDF string matching Valve formatting.
    pub fn to_vdf_string(&self) -> String {
        let mut out = String::new();
        serialize_table(&self.root, 0, &mut out);
        out
    }

    /// Serializes and atomically writes the VDF document to `path`.
    pub fn save_atomic<P: AsRef<Path>>(&self, path: P) -> Result<(), VdfError> {
        let serialized = self.to_vdf_string();
        atomic_write_str(path, &serialized)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Lexer / Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Str(String),
    OpenBrace,
    CloseBrace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    line: usize,
    col: usize,
}

fn tokenize(input: &str) -> Result<Vec<Token>, VdfError> {
    let mut tokens = Vec::new();

    // Strip UTF-8 BOM if present
    let raw = if let Some(stripped) = input.strip_prefix('\u{FEFF}') {
        stripped
    } else {
        input
    };

    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut line = 1;
    let mut col = 1;

    while i < len {
        let c = chars[i];

        // Whitespace handling
        if c.is_whitespace() {
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
            continue;
        }

        // Line comments: // ...
        if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            i += 2;
            col += 2;
            while i < len && chars[i] != '\n' {
                i += 1;
                col += 1;
            }
            continue;
        }

        // Block comments: /* ... */
        if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            col += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }
            if i + 1 < len {
                i += 2;
                col += 2;
            } else {
                i = len;
            }
            continue;
        }

        // Structural braces
        if c == '{' {
            tokens.push(Token {
                kind: TokenKind::OpenBrace,
                line,
                col,
            });
            i += 1;
            col += 1;
            continue;
        }

        if c == '}' {
            tokens.push(Token {
                kind: TokenKind::CloseBrace,
                line,
                col,
            });
            i += 1;
            col += 1;
            continue;
        }

        // Conditionals: [ ... ] (e.g. [$WIN32] or [!$OSX])
        if c == '[' {
            while i < len && chars[i] != ']' && chars[i] != '\n' {
                i += 1;
                col += 1;
            }
            if i < len && chars[i] == ']' {
                i += 1;
                col += 1;
            }
            continue;
        }

        // Quoted string
        if c == '"' {
            let start_line = line;
            let start_col = col;
            i += 1;
            col += 1;

            let mut s = String::new();
            let mut closed = false;

            while i < len {
                let ch = chars[i];
                if ch == '\\' {
                    if i + 1 < len {
                        let next = chars[i + 1];
                        match next {
                            '"' => {
                                s.push('"');
                                i += 2;
                                col += 2;
                            }
                            '\\' => {
                                s.push('\\');
                                i += 2;
                                col += 2;
                            }
                            other => {
                                // Valve VDF contains unescaped backslashes in Windows paths like
                                // C:\Program Files, C:\tools, C:\new, C:\release.
                                // Preserving them prevents path corruption into control characters (\t, \n, \r).
                                if other == '\n' {
                                    line += 1;
                                    col = 1;
                                } else {
                                    col += 2;
                                }
                                s.push('\\');
                                s.push(other);
                                i += 2;
                            }
                        }
                    } else {
                        s.push('\\');
                        i += 1;
                        col += 1;
                    }
                } else if ch == '"' {
                    i += 1;
                    col += 1;
                    closed = true;
                    break;
                } else {
                    if ch == '\n' {
                        line += 1;
                        col = 1;
                    } else {
                        col += 1;
                    }
                    s.push(ch);
                    i += 1;
                }
            }

            if !closed {
                return Err(VdfError::UnterminatedString {
                    line: start_line,
                    col: start_col,
                });
            }

            tokens.push(Token {
                kind: TokenKind::Str(s),
                line: start_line,
                col: start_col,
            });
            continue;
        }

        // Bareword / unquoted string (identifiers, numbers, flags)
        let start_line = line;
        let start_col = col;
        let mut s = String::new();

        while i < len {
            let ch = chars[i];
            if ch.is_whitespace() || ch == '{' || ch == '}' || ch == '"' || ch == '[' || ch == ']' {
                break;
            }
            if ch == '/' && i + 1 < len && (chars[i + 1] == '/' || chars[i + 1] == '*') {
                break;
            }
            s.push(ch);
            i += 1;
            col += 1;
        }

        if !s.is_empty() {
            tokens.push(Token {
                kind: TokenKind::Str(s),
                line: start_line,
                col: start_col,
            });
        }
    }

    Ok(tokens)
}

// ---------------------------------------------------------------------------
// AST Parser
// ---------------------------------------------------------------------------

fn parse_table(tokens: &[Token], pos: &mut usize, is_root: bool) -> Result<VdfTable, VdfError> {
    let mut table = VdfTable::new();

    while *pos < tokens.len() {
        let token = &tokens[*pos];

        match &token.kind {
            TokenKind::CloseBrace => {
                if is_root {
                    return Err(VdfError::UnexpectedToken {
                        line: token.line,
                        col: token.col,
                        found: "}".into(),
                        expected: "key string or block start".into(),
                    });
                }
                // End of this block
                break;
            }
            TokenKind::OpenBrace => {
                return Err(VdfError::UnexpectedToken {
                    line: token.line,
                    col: token.col,
                    found: "{".into(),
                    expected: "key string before block".into(),
                });
            }
            TokenKind::Str(key) => {
                let key = key.clone();
                *pos += 1;

                if *pos >= tokens.len() {
                    return Err(VdfError::UnexpectedEof {
                        expected: format!("value or '{{' after key '{}'", key),
                    });
                }

                let next_token = &tokens[*pos];
                match &next_token.kind {
                    TokenKind::OpenBrace => {
                        *pos += 1; // Consume '{'
                        let child_table = parse_table(tokens, pos, false)?;

                        if *pos >= tokens.len() {
                            return Err(VdfError::UnexpectedEof {
                                expected: format!("matching '}}' for block '{}'", key),
                            });
                        }

                        if tokens[*pos].kind != TokenKind::CloseBrace {
                            return Err(VdfError::UnexpectedToken {
                                line: tokens[*pos].line,
                                col: tokens[*pos].col,
                                found: format!("{:?}", tokens[*pos].kind),
                                expected: "}".into(),
                            });
                        }

                        *pos += 1; // Consume '}'
                        table.entries.push((key, VdfValue::Obj(child_table)));
                    }
                    TokenKind::Str(val) => {
                        let val = val.clone();
                        *pos += 1; // Consume value
                        table.entries.push((key, VdfValue::Str(val)));
                    }
                    TokenKind::CloseBrace => {
                        return Err(VdfError::UnexpectedToken {
                            line: next_token.line,
                            col: next_token.col,
                            found: "}".into(),
                            expected: format!("value or '{{' after key '{}'", key),
                        });
                    }
                }
            }
        }
    }

    Ok(table)
}

// ---------------------------------------------------------------------------
// Serializer
// ---------------------------------------------------------------------------

fn serialize_table(table: &VdfTable, depth: usize, out: &mut String) {
    let indent = "\t".repeat(depth);
    for (key, val) in &table.entries {
        match val {
            VdfValue::Str(s) => {
                out.push_str(&indent);
                out.push('"');
                escape_str_into(key, out);
                out.push_str("\"\t\t\"");
                escape_str_into(s, out);
                out.push_str("\"\n");
            }
            VdfValue::Obj(child) => {
                out.push_str(&indent);
                out.push('"');
                escape_str_into(key, out);
                out.push_str("\"\n");
                out.push_str(&indent);
                out.push_str("{\n");
                serialize_table(child, depth + 1, out);
                out.push_str(&indent);
                out.push_str("}\n");
            }
        }
    }
}

fn escape_str_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_key_values() {
        let input = r#"
            "Key1" "Value1"
            "Key2" "Value2"
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        assert_eq!(doc.get_str("Key1"), Some("Value1"));
        assert_eq!(doc.get_str("key2"), Some("Value2")); // Case-insensitive
        assert_eq!(doc.get_str("NonExistent"), None);
    }

    #[test]
    fn test_nested_blocks_and_braces() {
        let input = r#"
            "users"
            {
                "76561198000000001"
                {
                    "AccountName" "gabe"
                    "PersonaName" "Gabe Newell"
                    "MostRecent" "1"
                }
                "76561198000000002"
                {
                    "AccountName" "gordon"
                    "PersonaName" "Gordon Freeman"
                    "MostRecent" "0"
                }
            }
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        let users = doc.get_obj("users").expect("users table");
        assert_eq!(users.len(), 2);

        let gabe = users.get_obj("76561198000000001").expect("gabe table");
        assert_eq!(gabe.get_str("AccountName"), Some("gabe"));
        assert_eq!(gabe.get_str("MostRecent"), Some("1"));

        let gordon = users.get_obj("76561198000000002").expect("gordon table");
        assert_eq!(gordon.get_str("AccountName"), Some("gordon"));
        assert_eq!(gordon.get_str("MostRecent"), Some("0"));

        // Path navigation
        assert_eq!(
            doc.get_path_str(&["users", "76561198000000001"], "AccountName"),
            Some("gabe")
        );
    }

    #[test]
    fn test_comments_and_whitespace_variations() {
        let input = r#"
            // Leading comment
            "AppConfig" // Block comment
            {
                // Key-value comment
                "InstallDir"		"C:\Program Files (x86)\Steam"
                "BetaOptIn" "none" // Trailing inline comment
                "Enabled" 1 // Bareword value
            }
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        let app = doc.get_obj("appconfig").unwrap();
        assert_eq!(
            app.get_str("InstallDir"),
            Some(r#"C:\Program Files (x86)\Steam"#)
        );
        assert_eq!(app.get_str("BetaOptIn"), Some("none"));
        assert_eq!(app.get_str("Enabled"), Some("1"));
    }

    #[test]
    fn test_escaped_quotes_and_slashes() {
        let input = r#"
            "QuoteTest" "He said \"Hello World!\""
            "PathTest" "C:\\Games\\Steam\\steamapps"
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        assert_eq!(doc.get_str("QuoteTest"), Some(r#"He said "Hello World!""#));
        assert_eq!(doc.get_str("PathTest"), Some(r#"C:\Games\Steam\steamapps"#));
    }

    #[test]
    fn test_utf8_bom_handling() {
        let input = "\u{FEFF}\"Registry\" { \"Key\" \"Value\" }";
        let doc = VdfDocument::parse(input).unwrap();
        assert_eq!(doc.get_path_str(&["Registry"], "Key"), Some("Value"));
    }

    #[test]
    fn test_conditionals_ignored() {
        let input = r#"
            "Software"
            {
                "Valve"
                {
                    "Steam"
                    {
                        "CompatTool" "proton" [$WIN32]
                    }
                }
            }
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        assert_eq!(
            doc.get_path_str(&["Software", "Valve", "Steam"], "CompatTool"),
            Some("proton")
        );
    }

    #[test]
    fn test_mutation_and_roundtrip_serialization() {
        let original_vdf = r#""users"
{
	"76561198000000001"
	{
		"AccountName"		"alpha"
		"RememberPassword"		"1"
		"MostRecent"		"1"
	}
	"76561198000000002"
	{
		"AccountName"		"beta"
		"RememberPassword"		"1"
		"MostRecent"		"0"
	}
}
"#;
        let mut doc = VdfDocument::parse(original_vdf).unwrap();

        // Swap most recent
        let users = doc.get_obj_mut("users").unwrap();
        let u1 = users.get_obj_mut("76561198000000001").unwrap();
        u1.set_str("MostRecent", "0");

        let u2 = users.get_obj_mut("76561198000000002").unwrap();
        u2.set_str("MostRecent", "1");

        let serialized = doc.to_vdf_string();
        let reparsed = VdfDocument::parse(&serialized).unwrap();

        assert_eq!(
            reparsed.get_path_str(&["users", "76561198000000001"], "MostRecent"),
            Some("0")
        );
        assert_eq!(
            reparsed.get_path_str(&["users", "76561198000000002"], "MostRecent"),
            Some("1")
        );
    }

    #[test]
    fn test_syntax_errors() {
        // Unterminated quote
        assert!(matches!(
            VdfDocument::parse(r#""Key" "Value"#),
            Err(VdfError::UnterminatedString { .. })
        ));

        // Missing closing brace
        assert!(matches!(
            VdfDocument::parse(r#""Key" { "SubKey" "SubValue" "#),
            Err(VdfError::UnexpectedEof { .. })
        ));

        // Unexpected closing brace
        assert!(matches!(
            VdfDocument::parse(r#""Key" "Value" }"#),
            Err(VdfError::UnexpectedToken { .. })
        ));
    }

    #[test]
    fn test_real_world_loginusers_vdf() {
        let real_loginusers = r#""users"
{
	"76561198012345678"
	{
		"AccountName"		"john_doe"
		"PersonaName"		"John"
		"RememberPassword"		"1"
		"MostRecent"		"1"
		"Timestamp"		"1694500000"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
		"AllowAutoLogin"		"1"
	}
	"76561198987654321"
	{
		"AccountName"		"jane_doe"
		"PersonaName"		"Jane <3"
		"RememberPassword"		"1"
		"MostRecent"		"0"
		"Timestamp"		"1694400000"
		"WantsOfflineMode"		"0"
		"SkipOfflineModeWarning"		"0"
		"AllowAutoLogin"		"1"
	}
	"76561198111222333"
	{
		"AccountName"		"pro_sniper_2026"
		"PersonaName"		"Sniper // Ghost"
		"RememberPassword"		"0"
		"MostRecent"		"0"
		"Timestamp"		"1693000000"
	}
}
"#;
        let mut doc = VdfDocument::parse(real_loginusers).unwrap();
        let users = doc.get_obj_mut("users").unwrap();
        assert_eq!(users.len(), 3);

        // Switch active user from john_doe to jane_doe
        for (_, user_val) in users.iter_mut() {
            if let VdfValue::Obj(ref mut u_tbl) = user_val {
                let acc = u_tbl.get_str("AccountName").unwrap_or("");
                if acc.eq_ignore_ascii_case("jane_doe") {
                    u_tbl.set_str("MostRecent", "1");
                    u_tbl.set_str("RememberPassword", "1");
                } else {
                    u_tbl.set_str("MostRecent", "0");
                }
            }
        }

        let serialized = doc.to_vdf_string();
        let reparsed = VdfDocument::parse(&serialized).unwrap();

        assert_eq!(
            reparsed.get_path_str(&["users", "76561198012345678"], "MostRecent"),
            Some("0")
        );
        assert_eq!(
            reparsed.get_path_str(&["users", "76561198987654321"], "MostRecent"),
            Some("1")
        );
        assert_eq!(
            reparsed.get_path_str(&["users", "76561198987654321"], "RememberPassword"),
            Some("1")
        );
        assert_eq!(
            reparsed.get_path_str(&["users", "76561198111222333"], "MostRecent"),
            Some("0")
        );
        assert_eq!(
            reparsed.get_path_str(&["users", "76561198111222333"], "RememberPassword"),
            Some("0")
        );
    }

    #[test]
    fn test_real_world_config_vdf() {
        let real_config = r#""InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"AutoLoginUser"		"test_user_old"
				"RememberPassword"		"1"
				"AlreadyRetriedOfflineMode"		"0"
				"Accounts"
				{
					"test_user_old"
					{
						"SteamID"		"76561198012345678"
					}
					"test_user_new"
					{
						"SteamID"		"76561198987654321"
					}
				}
				"apps"
				{
					"220"
					{
						"cloudenabled"		"1"
						"cloud_sync_state"		"synced"
					}
					"570"
					{
						"cloudenabled"		"0"
					}
				}
			}
		}
	}
}
"#;
        let mut doc = VdfDocument::parse(real_config).unwrap();
        assert_eq!(
            doc.get_path_str(
                &["InstallConfigStore", "Software", "Valve", "Steam"],
                "AutoLoginUser"
            ),
            Some("test_user_old")
        );

        // Update AutoLoginUser
        doc.set_path_value(
            &["InstallConfigStore", "Software", "Valve", "Steam"],
            "AutoLoginUser",
            "test_user_new",
        );

        let serialized = doc.to_vdf_string();
        let reparsed = VdfDocument::parse(&serialized).unwrap();

        assert_eq!(
            reparsed.get_path_str(
                &["InstallConfigStore", "Software", "Valve", "Steam"],
                "AutoLoginUser"
            ),
            Some("test_user_new")
        );
        assert_eq!(
            reparsed.get_path_str(
                &[
                    "InstallConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "apps",
                    "220"
                ],
                "cloudenabled"
            ),
            Some("1")
        );
    }

    #[test]
    fn test_block_comments_and_windows_paths_with_backslashes() {
        let input = r#"
            /* Top level block comment
               spanning multiple lines */
            "Paths"
            {
                /* Comment before key */
                "SteamDir" "C:\tools\new_games\releases" /* Inline block comment */
                "Secondary" "D:\test\native\root"
                "UnescapedBackslash" "E:\Program Files (x86)\Steam"
            }
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        assert_eq!(
            doc.get_path_str(&["Paths"], "SteamDir"),
            Some(r#"C:\tools\new_games\releases"#)
        );
        assert_eq!(
            doc.get_path_str(&["Paths"], "Secondary"),
            Some(r#"D:\test\native\root"#)
        );
        assert_eq!(
            doc.get_path_str(&["Paths"], "UnescapedBackslash"),
            Some(r#"E:\Program Files (x86)\Steam"#)
        );

        // Verify round-trip preserves valid paths
        let serialized = doc.to_vdf_string();
        let reparsed = VdfDocument::parse(&serialized).unwrap();
        assert_eq!(
            reparsed.get_path_str(&["Paths"], "SteamDir"),
            Some(r#"C:\tools\new_games\releases"#)
        );
    }

    #[test]
    fn test_brackets_inside_quoted_string_not_treated_as_conditionals() {
        let input = r#"
            "users"
            {
                "12345"
                {
                    "PersonaName" "[CLAN] Pro Player [RUS]"
                    "Tag" "[MVP]"
                }
            }
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        assert_eq!(
            doc.get_path_str(&["users", "12345"], "PersonaName"),
            Some("[CLAN] Pro Player [RUS]")
        );
        assert_eq!(doc.get_path_str(&["users", "12345"], "Tag"), Some("[MVP]"));
    }

    #[test]
    fn test_empty_block_and_multiple_root_keys() {
        let input = r#"
            "BlockA"
            {
            }
            "BlockB"
            {
                "SubBlock"
                {
                }
            }
        "#;
        let doc = VdfDocument::parse(input).unwrap();
        assert!(doc.get_obj("BlockA").unwrap().is_empty());
        assert!(doc
            .find_obj_path(&["BlockB", "SubBlock"])
            .unwrap()
            .is_empty());

        let serialized = doc.to_vdf_string();
        let reparsed = VdfDocument::parse(&serialized).unwrap();
        assert!(reparsed.get_obj("BlockA").unwrap().is_empty());
        assert!(reparsed
            .find_obj_path(&["BlockB", "SubBlock"])
            .unwrap()
            .is_empty());
    }
}
