"""Small lexical helpers for source inventories, preserving source positions."""
import re

RAW = re.compile(r'(?:br|cr|r)(#*)"')
CHARACTER = re.compile(r"'(?:\\(?:[nrt0\\'\"]|x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]+\})|[^\\'\n])'")


def code_only(source):
    """Mask comments and literals; retain lifetimes, code and line boundaries."""
    result = list(source)
    index = 0
    while index < len(source):
        start = index
        if source.startswith('//', index):
            end = source.find('\n', index)
            index = len(source) if end < 0 else end
        elif source.startswith('/*', index):
            index += 2
            depth = 1
            while index < len(source) and depth:
                if source.startswith('/*', index):
                    depth += 1
                    index += 2
                elif source.startswith('*/', index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
        else:
            raw = RAW.match(source, index) if source[index] in 'bcr' else None
            character = CHARACTER.match(source, index) if source[index] == "'" else None
            if raw:
                delimiter = '"' + raw[1]
                end = source.find(delimiter, raw.end())
                index = len(source) if end < 0 else end + len(delimiter)
            elif character:
                index = character.end()
            elif source[index] == '"':
                index += 1
                while index < len(source):
                    if source[index] == '\\':
                        index += 2
                    elif source[index] == '"':
                        index += 1
                        break
                    else:
                        index += 1
            else:
                index += 1
                continue
        for position in range(start, min(index, len(source))):
            if source[position] not in '\r\n':
                result[position] = ' '
    return ''.join(result)


def structs(source):
    """Inventory named Rust struct fields, including multiline field types."""
    code = code_only(source)
    tokens = list(re.finditer(r'[A-Za-z_]\w*|->|::|[^\s]', code))
    result = {}
    index = 0
    while index + 1 < len(tokens):
        if tokens[index][0] != 'struct':
            index += 1
            continue
        name = tokens[index + 1][0]
        index += 2
        while index < len(tokens) and tokens[index][0] not in ('{', ';'):
            index += 1
        if index == len(tokens) or tokens[index][0] != '{':
            continue
        index += 1
        fields = {}
        segment = []
        depth = 0
        while index < len(tokens):
            token = tokens[index]
            value = token[0]
            index += 1
            if depth == 0 and value in (',', '}'):
                read_field(segment, code, fields)
                segment = []
                if value == '}':
                    break
                continue
            segment.append(token)
            if value in ('<', '(', '[', '{'):
                depth += 1
            elif value in ('>', ')', ']', '}'):
                depth -= 1
        result[name] = fields
    return result


def read_field(tokens, code, fields):
    """Attributes and visibility precede the first top-level name/colon pair."""
    depth = 0
    for index, token in enumerate(tokens[:-1]):
        value = token[0]
        if depth == 0 and re.fullmatch(r'[A-Za-z_]\w*', value) and tokens[index + 1][0] == ':':
            field_type = code[tokens[index + 1].end():tokens[-1].end()]
            fields[value] = ' '.join(field_type.split())
            return
        if value in ('(', '[', '{'):
            depth += 1
        elif value in (')', ']', '}'):
            depth -= 1
