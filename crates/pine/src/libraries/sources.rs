//! Pure, bounded source linking. The caller owns all I/O and source selection.
use std::collections::BTreeMap;

use crate::{
    CompiledScript, ErrorCode, PineError, Span,
    lexer::{self, Tok},
};

/// Compile with an exact-key local source resolver. Only `local/name/version`
/// imports are admitted; this function never searches disk or the network.
///
/// # Errors
/// Missing sources, cycles, invalid imports and ordinary compiler diagnostics.
pub fn compile_with_libraries(
    source: &str,
    fallback_title: &str,
    resolver: impl Fn(&str) -> Option<String>,
) -> Result<CompiledScript, Vec<PineError>> {
    let mut linker = Linker {
        resolver,
        stack: Vec::new(),
        next: 0,
        version: lexer::lex(source)?
            .version
            .map_or(5, |(version, _)| version),
    };
    let (source, _) = linker.link(source, None).map_err(|error| vec![error])?;
    crate::compile(&source, fallback_title)
}

struct Linker<F> {
    resolver: F,
    stack: Vec<String>,
    next: usize,
    version: u32,
}

fn error(message: impl Into<String>) -> PineError {
    PineError::new(ErrorCode::PineSyntax, Span::new(0, 0), message)
}

impl<F: Fn(&str) -> Option<String>> Linker<F> {
    fn link(
        &mut self,
        source: &str,
        prefix: Option<String>,
    ) -> Result<(String, BTreeMap<String, String>), PineError> {
        if source.len() > 4_000_000 || self.next > 128 {
            return Err(error("local library source budget exceeded"));
        }
        let version = lexer::lex(source)
            .map_err(|mut errors| errors.remove(0))?
            .version
            .map_or(5, |(version, _)| version);
        if prefix.is_some() && version != self.version {
            return Err(error(
                "local libraries must use the importing script's Pine version",
            ));
        }
        let mut aliases = BTreeMap::new();
        let mut dependencies = String::new();
        let mut body = String::new();
        let mut names = BTreeMap::new();
        let mut exports = BTreeMap::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(import) = trimmed.strip_prefix("import ") {
                let parts: Vec<_> = import.split_whitespace().collect();
                if parts.len() != 3 || parts[1] != "as" {
                    return Err(error(
                        "local import requires `import local/name/version as alias`",
                    ));
                }
                let key = parts[0];
                let path: Vec<_> = key.split('/').collect();
                if path.len() != 3
                    || path[0] != "local"
                    || path[1].is_empty()
                    || path[2].parse::<u32>().is_err()
                {
                    return Err(error(format!(
                        "remote or invalid library import `{key}`; supply a local/name/version key"
                    )));
                }
                if self.stack.iter().any(|entry| entry == key) {
                    return Err(error(format!(
                        "cyclic local library import: {} -> {key}",
                        self.stack.join(" -> ")
                    )));
                }
                if self.stack.len() >= 32 {
                    return Err(error("local library import depth exceeds 32"));
                }
                let library = (self.resolver)(key)
                    .ok_or_else(|| error(format!("missing local library `{key}`")))?;
                self.stack.push(key.to_owned());
                let child_prefix = format!("qlib{}_", self.next);
                self.next += 1;
                let linked = self.link(&library, Some(child_prefix));
                self.stack.pop();
                let (linked, public) = linked?;
                if aliases.insert(parts[2].to_owned(), public).is_some() {
                    return Err(error(format!("duplicate library alias `{}`", parts[2])));
                }
                dependencies.push_str(&linked);
                if dependencies.len() > 4_000_000 {
                    return Err(error("local library source budget exceeded"));
                }
                body.push('\n');
                continue;
            }
            if prefix.is_some()
                && (trimmed.starts_with("library(") || trimmed.starts_with("//@version="))
            {
                body.push('\n');
                continue;
            }
            let exported = line.strip_prefix("export ");
            let declaration = exported.unwrap_or(line);
            if exported.is_some()
                && let Some(method) = declaration.strip_prefix("method ")
                && let Some((name, _)) = method.split_once('(')
            {
                exports.insert(name.trim().to_owned(), name.trim().to_owned());
            }
            if let Some(prefix) = &prefix {
                let name = declaration
                    .strip_prefix("type ")
                    .map(str::trim)
                    .or_else(|| {
                        declaration
                            .split_once('(')
                            .filter(|_| declaration.contains("=>"))
                            .map(|(name, _)| name.trim())
                    });
                if let Some(name) =
                    name.filter(|name| !name.starts_with("method ") && !name.contains(' '))
                {
                    let renamed = format!("{prefix}{name}");
                    names.insert(name.to_owned(), renamed.clone());
                    if exported.is_some() {
                        exports.insert(name.to_owned(), renamed);
                    }
                }
            }
            body.push_str(declaration);
            body.push('\n');
        }
        let tokens = lexer::lex(&body)
            .map_err(|mut errors| errors.remove(0))?
            .tokens;
        let mut edits = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            if let Tok::Ident(name) = &token.tok {
                if let Some(public) = aliases.get(name)
                    && matches!(tokens.get(index + 1).map(|t| &t.tok), Some(Tok::Dot))
                    && let Some(member) = tokens.get(index + 2)
                    && let Tok::Ident(field) = &member.tok
                {
                    let renamed = public.get(field).ok_or_else(|| {
                        error(format!("library `{name}` does not export `{field}`"))
                    })?;
                    edits.push((token.span.start, member.span.end, renamed.clone()));
                    index += 3;
                    continue;
                }
                if let Some(renamed) = names.get(name) {
                    edits.push((token.span.start, token.span.end, renamed.clone()));
                }
            }
            index += 1;
        }
        for (start, end, text) in edits.into_iter().rev() {
            body.replace_range(start..end, &text);
        }
        // Keep the root's version directive first, before dependency definitions.
        let version = source
            .lines()
            .find(|line| line.trim().starts_with("//@version="));
        let mut linked = String::new();
        if prefix.is_none()
            && let Some(version) = version
        {
            linked.push_str(version);
            linked.push('\n');
        }
        linked.push_str(&dependencies);
        linked.push_str(&body);
        Ok((linked, exports))
    }
}
