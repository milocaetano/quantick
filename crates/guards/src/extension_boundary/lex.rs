//! A lexical token stream with source lines; no macro expansion or type resolution.

#[derive(Debug)]
pub(super) struct Token {
    pub text: String,
    pub line: usize,
    pub end_line: usize,
    pub pair: Option<usize>,
}

pub(super) fn lex(source: &str) -> Result<Vec<Token>, String> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut at = 0;
    let mut line = 0;
    while at < bytes.len() {
        let start = at;
        let start_line = line;
        if bytes[at].is_ascii_whitespace() {
            line += usize::from(bytes[at] == b'\n');
            at += 1;
            continue;
        }
        if bytes[at..].starts_with(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes[at..].starts_with(b"/*") {
            at += 2;
            let mut depth = 1;
            while at < bytes.len() && depth > 0 {
                if bytes[at..].starts_with(b"/*") {
                    depth += 1;
                    at += 2;
                } else if bytes[at..].starts_with(b"*/") {
                    depth -= 1;
                    at += 2;
                } else {
                    line += usize::from(bytes[at] == b'\n');
                    at += 1;
                }
            }
            if depth != 0 {
                return Err(format!(
                    "line {}: unterminated block comment",
                    start_line + 1
                ));
            }
            continue;
        }
        let raw_start = if bytes[at..].starts_with(b"br") || bytes[at..].starts_with(b"cr") {
            Some(at + 2)
        } else if bytes[at] == b'r' {
            Some(at + 1)
        } else {
            None
        };
        let raw = raw_start.and_then(|mut cursor| {
            let hash_start = cursor;
            while bytes.get(cursor) == Some(&b'#') {
                cursor += 1;
            }
            (bytes.get(cursor) == Some(&b'"')).then_some((cursor, cursor - hash_start))
        });
        if let Some((quote, hashes)) = raw {
            at = quote + 1;
            loop {
                if at >= bytes.len() {
                    return Err(format!("line {}: unterminated raw string", start_line + 1));
                }
                if bytes[at] == b'"'
                    && bytes.get(at + 1..at + 1 + hashes) == Some(vec![b'#'; hashes].as_slice())
                {
                    at += 1 + hashes;
                    break;
                }
                line += usize::from(bytes[at] == b'\n');
                at += 1;
            }
        } else {
            let quote = if matches!(bytes[at], b'b' | b'c')
                && matches!(bytes.get(at + 1), Some(b'"' | b'\''))
            {
                Some(at + 1)
            } else if matches!(bytes[at], b'"' | b'\'') {
                Some(at)
            } else {
                None
            };
            // An apostrophe followed by an identifier without a closing quote
            // is a lifetime (or label). Character literals have the close.
            let lifetime = quote == Some(at)
                && bytes[at] == b'\''
                && bytes.get(at + 1).is_some_and(|b| ident_start(*b))
                && {
                    let mut end = at + 2;
                    while bytes.get(end).is_some_and(|b| ident(*b)) {
                        end += 1;
                    }
                    bytes.get(end) != Some(&b'\'')
                };
            if lifetime {
                at += 2;
                while bytes.get(at).is_some_and(|b| ident(*b)) {
                    at += 1;
                }
            } else if let Some(quote) = quote {
                let delimiter = bytes[quote];
                at = quote + 1;
                loop {
                    if at >= bytes.len() {
                        return Err(format!("line {}: unterminated literal", start_line + 1));
                    }
                    if bytes[at] == delimiter {
                        at += 1;
                        break;
                    }
                    if bytes[at] == b'\\' {
                        at += 1;
                        if at >= bytes.len() {
                            return Err(format!("line {}: unterminated escape", start_line + 1));
                        }
                    }
                    line += usize::from(bytes[at] == b'\n');
                    at += 1;
                }
            } else if ident_start(bytes[at]) || bytes[at].is_ascii_digit() {
                at += 1;
                while bytes.get(at).is_some_and(|b| ident(*b)) {
                    at += 1;
                }
            } else {
                at += 1;
                if matches!(source.get(start..at + 1), Some("::" | "->" | "=>")) {
                    at += 1;
                }
            }
        }
        let text = source
            .get(start..at)
            .ok_or_else(|| format!("line {}: unsupported non-ASCII punctuation", start_line + 1))?;
        let index = tokens.len();
        tokens.push(Token {
            text: text.into(),
            line: start_line,
            end_line: line,
            pair: None,
        });
        if matches!(text, "(" | "[" | "{") {
            stack.push((bytes[start], index));
        } else if matches!(text, ")" | "]" | "}") {
            let Some((open, other)) = stack.pop() else {
                return Err(format!("line {}: unmatched {text}", line + 1));
            };
            if !matches!(
                (open, bytes[start]),
                (b'(', b')') | (b'[', b']') | (b'{', b'}')
            ) {
                return Err(format!("line {}: mismatched {text}", line + 1));
            }
            tokens[index].pair = Some(other);
            tokens[other].pair = Some(index);
        }
    }
    if let Some((_, index)) = stack.first() {
        return Err(format!(
            "line {}: unclosed delimiter",
            tokens[*index].line + 1
        ));
    }
    Ok(tokens)
}

fn ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 128
}

fn ident(byte: u8) -> bool {
    ident_start(byte) || byte.is_ascii_digit()
}

pub(super) fn normalized(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}
