//! Canonical named items only. Unsupported protected scope is an error.

use std::collections::{BTreeMap, BTreeSet};

use super::lex::{Token, lex, normalized};

pub(super) const ROOTS: [&str; 2] = ["QuantickApp", "ChartState"];
pub(super) const TARGETS: [&str; 4] = [
    "QuantickApp",
    "ChartState",
    "IndicatorSlots",
    "IndicatorHost",
];

#[derive(Default, Debug)]
pub(super) struct FileScan {
    pub shapes: Vec<(String, String)>,
    pub lines: BTreeMap<String, BTreeSet<usize>>,
    pub spans: Vec<(String, usize, usize)>,
}

fn protected(tokens: &[Token]) -> bool {
    tokens
        .iter()
        .any(|token| TARGETS.contains(&token.text.as_str()))
}

fn root(tokens: &[Token]) -> bool {
    tokens
        .iter()
        .any(|token| ROOTS.contains(&token.text.as_str()))
}

fn fail(token: &Token, reason: &str) -> String {
    format!(
        "line {}: unsupported protected scope: {reason}",
        token.line + 1
    )
}

pub(super) fn scan(source: &str) -> Result<FileScan, String> {
    let tokens = lex(source)?;
    let flags = crate::size::production_flags(source);
    validate_exclusions(source, &tokens, &flags)?;
    let mut result = FileScan::default();
    let mut at = 0;
    while at < tokens.len() {
        let token = &tokens[at];
        if !flags[token.line] {
            at += 1;
            continue;
        }
        // Item-generating macros are not Rust items until expanded. Never
        // accept an apparent protected impl inside their token trees as proof.
        if token.text == "!" {
            let next = at + 1;
            let open = if tokens.get(next).is_some_and(|t| t.pair.is_some()) {
                next
            } else {
                next + 1 // macro_rules! name { ... }
            };
            if let Some(end) = tokens
                .get(open)
                .and_then(|t| t.pair)
                .filter(|end| *end > open)
            {
                if protected(&tokens[open + 1..end])
                    || at
                        .checked_sub(1)
                        .is_some_and(|i| tokens[i].text == "include")
                {
                    return Err(fail(
                        token,
                        "macro/include expansion contains or may import protected items",
                    ));
                }
                at = end + 1;
                continue;
            }
        }
        if token.text == "#" && tokens.get(at + 1).is_some_and(|t| t.text == "[") {
            let end = tokens[at + 1].pair.unwrap();
            if tokens[at + 2..end].iter().any(|t| t.text == "path") {
                return Err(fail(
                    token,
                    "external-path modules are outside the canonical source layout",
                ));
            }
        }
        if token.text == "mod" {
            let mut name = at + 1;
            if tokens.get(name).is_some_and(|t| t.text == "r")
                && tokens.get(name + 1).is_some_and(|t| t.text == "#")
            {
                name += 2;
            }
            if tokens
                .get(name)
                .is_some_and(|t| matches!(t.text.as_str(), "tests" | "target"))
                && tokens.get(name + 1).is_some_and(|t| t.text == ";")
            {
                return Err(fail(
                    token,
                    "out-of-line production module may import a skipped tests/target directory",
                ));
            }
        }
        if token.text == "use" {
            let end = terminator(&tokens, at + 1, ";")?;
            for (i, item) in tokens[at + 1..end].iter().enumerate() {
                if ROOTS.contains(&item.text.as_str())
                    && tokens.get(at + i + 2).is_some_and(|t| t.text == "as")
                {
                    return Err(fail(item, "root-renaming import alias"));
                }
            }
        }
        if token.text == "type" {
            let end = terminator(&tokens, at + 1, ";")?;
            if let Some(eq) = (at + 1..end).find(|i| tokens[*i].text == "=") {
                let rhs = &tokens[eq + 1..end];
                // The existing callbacks mention a root as an argument, but
                // name a function pointer, never the root itself.
                let callback = rhs.first().is_some_and(|t| t.text == "fn")
                    && rhs.get(1).is_some_and(|t| t.text == "(")
                    || rhs
                        .iter()
                        .take(5)
                        .map(|t| t.text.as_str())
                        .eq(["Box", "<", "dyn", "Fn", "("])
                        && rhs.last().is_some_and(|t| t.text == ">");
                if root(rhs) && !callback {
                    return Err(fail(
                        token,
                        "root type alias (the documented callable aliases are supported)",
                    ));
                }
            }
        }
        if matches!(token.text.as_str(), "struct" | "trait")
            && let Some(name) = tokens
                .get(at + 1)
                .filter(|t| TARGETS.contains(&t.text.as_str()))
        {
            let mut open = at + 2;
            while open < tokens.len() && !matches!(tokens[open].text.as_str(), "{" | ";" | "(") {
                open += 1;
            }
            if tokens.get(open).is_none_or(|t| t.text != "{") {
                return Err(fail(name, "target requires a named brace body"));
            }
            let header = normalized(&tokens[at + 2..open]);
            let expected = if name.text == "IndicatorSlots" {
                "< 'a >"
            } else {
                ""
            };
            if header != expected || (token.text == "trait") != (name.text == "IndicatorHost") {
                return Err(fail(name, "unsupported protected declaration header"));
            }
            let end = tokens[open].pair.unwrap();
            let visibility = visibility_before(&tokens, at)?;
            let declaration_start = declaration_start(&tokens, at);
            if declaration_start > 0 && tokens[declaration_start - 1].text == "]" {
                return Err(fail(
                    name,
                    "attributed protected declarations are unsupported",
                ));
            }
            let members = if token.text == "struct" {
                fields(&tokens[open + 1..end])?
            } else {
                methods(&tokens[open + 1..end])?
            };
            result.shapes.push((
                name.text.clone(),
                format!("{visibility}|{} {header}|{}", token.text, members.join("|")),
            ));
            if ROOTS.contains(&name.text.as_str()) {
                add_span(
                    &mut result,
                    &name.text,
                    tokens[declaration_start].line,
                    tokens[end].end_line,
                    &flags,
                );
            }
        }
        let type_position = token.text == "impl"
            && (at.checked_sub(1).is_some_and(|i| tokens[i].text == "->")
                || tokens[..at].iter().any(|t| {
                    matches!(t.text.as_str(), "(" | "[") && t.pair.is_some_and(|end| end > at)
                }));
        if token.text == "impl" && !type_position {
            let mut open = at + 1;
            while open < tokens.len() && !matches!(tokens[open].text.as_str(), "{" | ";" | "}") {
                open += 1;
            }
            let header = &tokens[at + 1..open];
            if root(header) {
                if at > 0 && matches!(tokens[at - 1].text.as_str(), "unsafe" | "default" | "]") {
                    return Err(fail(
                        token,
                        "modified/attributed protected impl is unsupported",
                    ));
                }
                let self_start = header
                    .iter()
                    .rposition(|t| t.text == "for")
                    .map_or(0, |i| i + 1);
                let self_type = &header[self_start..];
                let name = self_type.last().map(|t| t.text.as_str()).unwrap_or("");
                if !ROOTS.contains(&name)
                    || !path(self_type)
                    || (self_start > 0 && !path(&header[..self_start - 1]))
                    || tokens.get(open).is_none_or(|t| t.text != "{")
                {
                    return Err(fail(token, "unsupported protected impl header"));
                }
                let end = tokens[open].pair.unwrap();
                add_span(&mut result, name, token.line, tokens[end].end_line, &flags);
            }
        }
        at += 1;
    }
    Ok(result)
}

fn path(tokens: &[Token]) -> bool {
    let tokens = if tokens.first().is_some_and(|t| t.text == "::") {
        &tokens[1..]
    } else {
        tokens
    };
    !tokens.is_empty()
        && !tokens.len().is_multiple_of(2)
        && tokens.iter().enumerate().all(|(i, t)| {
            if i % 2 == 1 {
                t.text == "::"
            } else {
                t.text.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && t.text
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
            }
        })
}

fn terminator(tokens: &[Token], start: usize, wanted: &str) -> Result<usize, String> {
    let mut at = start;
    while at < tokens.len() {
        if tokens[at].text == wanted {
            return Ok(at);
        }
        at = tokens[at]
            .pair
            .filter(|end| *end > at)
            .map_or(at + 1, |end| end + 1);
    }
    Err(fail(&tokens[start - 1], "item has no terminator"))
}

fn visibility_before(tokens: &[Token], at: usize) -> Result<String, String> {
    if at > 0 && tokens[at - 1].text == "pub" {
        return Ok("pub".into());
    }
    if at > 0 && tokens[at - 1].text == ")" {
        let open = tokens[at - 1].pair.unwrap();
        if open > 0 && tokens[open - 1].text == "pub" {
            return Ok(normalized(&tokens[open - 1..at]));
        }
    }
    Ok("private".into())
}

fn declaration_start(tokens: &[Token], at: usize) -> usize {
    if at > 0 && tokens[at - 1].text == "pub" {
        return at - 1;
    }
    if at > 0 && tokens[at - 1].text == ")" {
        let open = tokens[at - 1].pair.unwrap();
        if open > 0 && tokens[open - 1].text == "pub" {
            return open - 1;
        }
    }
    at
}

fn fields(tokens: &[Token]) -> Result<Vec<String>, String> {
    let local = lex(&normalized(tokens))?;
    let tokens = local.as_slice();
    let mut fields = Vec::new();
    let mut start = 0;
    let mut angle = 0usize;
    let mut at = 0;
    while at < tokens.len() {
        match tokens[at].text.as_str() {
            "#" => return Err(fail(&tokens[at], "conditional/attributed protected fields")),
            "<" => angle += 1,
            ">" => {
                angle = angle
                    .checked_sub(1)
                    .ok_or_else(|| fail(&tokens[at], "unbalanced field type"))?
            }
            "," if angle == 0 => {
                let member = &tokens[start..at];
                if !member.iter().any(|t| t.text == ":") {
                    return Err(fail(&tokens[at], "expected named field and type"));
                }
                fields.push(normalized(member));
                start = at + 1;
            }
            _ => {}
        }
        at = tokens[at]
            .pair
            .filter(|end| *end > at)
            .map_or(at + 1, |end| end + 1);
    }
    if start != tokens.len() || angle != 0 {
        return Err(
            "unsupported protected scope: field list needs balanced types and trailing commas"
                .into(),
        );
    }
    Ok(fields)
}

fn methods(tokens: &[Token]) -> Result<Vec<String>, String> {
    let local = lex(&normalized(tokens))?;
    let tokens = local.as_slice();
    let mut methods = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at].text != "fn" {
            return Err(fail(
                &tokens[at],
                "host only supports ordinary method signatures",
            ));
        }
        let end = terminator(tokens, at + 1, ";")?;
        if tokens[at..end]
            .iter()
            .any(|t| matches!(t.text.as_str(), "{" | "#"))
        {
            return Err(fail(
                &tokens[at],
                "host default bodies/attributes are unsupported",
            ));
        }
        methods.push(normalized(&tokens[at..=end]));
        at = end + 1;
    }
    Ok(methods)
}

fn add_span(result: &mut FileScan, name: &str, start: usize, end: usize, flags: &[bool]) {
    result.spans.push((name.into(), start + 1, end + 1));
    result
        .lines
        .entry(name.into())
        .or_default()
        .extend((start..=end).filter(|line| flags[*line]));
}

fn validate_exclusions(source: &str, tokens: &[Token], flags: &[bool]) -> Result<(), String> {
    for (line, text) in source
        .lines()
        .enumerate()
        .filter(|(_, text)| *text == "#[cfg(test)]")
    {
        let Some(at) = tokens.iter().position(|t| t.line == line && t.text == "#") else {
            return Err(format!(
                "line {}: production classifier marker inside lexical data",
                line + 1
            ));
        };
        if tokens
            .iter()
            .enumerate()
            .any(|(i, t)| i < at && t.pair.is_some_and(|end| end > at))
        {
            return Err(format!(
                "line {}: production classifier marker is not top-level: {text}",
                line + 1
            ));
        }
        // A classifier exclusion must end at a lexical boundary, not at a
        // column-zero brace inside a raw string or nested block.
        let last = flags
            .iter()
            .enumerate()
            .skip(line)
            .take_while(|(_, flag)| !**flag)
            .last()
            .unwrap()
            .0;
        if tokens.iter().any(|t| t.line <= last && t.end_line > last)
            || tokens.iter().enumerate().any(|(i, t)| {
                t.line >= line
                    && t.line <= last
                    && t.pair.is_some_and(|end| end > i && tokens[end].line > last)
            })
        {
            return Err(format!(
                "line {}: production classifier exclusion cuts a lexical item",
                line + 1
            ));
        }
    }
    Ok(())
}
