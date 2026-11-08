//! Protect field labels and function parameters while namespacing local sources.
// Members are handled separately by the linker; these are declaration scopes.
pub(super) fn scopes(source: &str) -> Vec<(usize, usize, Vec<String>)> {
    let mut result = Vec::new();
    let lines: Vec<_> = source.split_inclusive('\n').collect();
    let mut offset = 0;
    for (index, line) in lines.iter().enumerate() {
        let end = lines[index + 1..]
            .iter()
            .take_while(|line| line.trim().is_empty() || line.starts_with(char::is_whitespace))
            .map(|line| line.len())
            .sum::<usize>()
            + offset
            + line.len();
        if line.starts_with("type ") {
            let mut field_offset = offset + line.len();
            for field in &lines[index + 1..] {
                if field_offset >= end {
                    break;
                }
                let declaration = field.split('=').next().unwrap_or(field);
                if let Some(name) = declaration.split_whitespace().nth(1) {
                    // Only protect the field label, not identifiers in its default.
                    let start = field_offset + declaration.rfind(name).expect("field label");
                    result.push((start, start + name.len(), vec![name.to_owned()]));
                }
                field_offset += field.len();
            }
        } else if !line.starts_with(char::is_whitespace)
            && line.contains("=>")
            && let Some((_, parameters)) = line.split_once('(')
            && let Some((parameters, _)) = parameters.split_once(')')
        {
            let locals = parameters
                .split(',')
                .filter_map(|parameter| parameter.split_whitespace().last().map(str::to_owned))
                .collect();
            // Header types and the function name remain global declarations.
            let start = offset + line.find('(').expect("function parameters") + 1;
            result.push((start, end, locals));
        }
        offset += line.len();
    }
    result
}
