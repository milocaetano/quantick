//! Public receiver spellings share the namespaced method's already-linked body.
use std::collections::BTreeMap;

pub(super) fn receiver_aliases(source: &str, exports: &BTreeMap<String, String>) -> String {
    let lines: Vec<_> = source.split_inclusive('\n').collect();
    let mut result = String::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((name, _)) = line.strip_prefix("method ").and_then(|s| s.split_once('(')) else {
            continue;
        };
        let Some((public, _)) = exports
            .iter()
            .find(|(_, internal)| internal.as_str() == name.trim())
        else {
            continue;
        };
        // Qualified calls use the unique internal identity. Receiver syntax is
        // still overload-selected by its public name and typed first argument.
        result.push_str(&line.replacen(name.trim(), public, 1));
        for body in lines[index + 1..]
            .iter()
            .take_while(|line| line.trim().is_empty() || line.starts_with(char::is_whitespace))
        {
            result.push_str(body);
        }
    }
    result
}
