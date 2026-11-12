"""Reproducible headless-UI inventory over Git objects; never a build guard."""
import argparse
import io
import json
from pathlib import Path
import posixpath
import re
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
SURFACES = ('agent_popup bar_switch chart_settings drawing_chrome home '
            'indicator_picker indicator_preview market_palette remove_chart '
            'script_editor strategy_popup toast').split()
CHROME_WIRING = ['capsule_wiring', 'toolbar_wiring']
RECIPE = '.claude/skills/new-extension/references/headless-ui.md'
TARGETS = {
    'app_production': 'strictly below baseline',
    'app_owned_tests': 'strictly below baseline',
    'quantick_app_fields_max': 30,
    'new_wiring_paths_max': 0,
    'chrome_wiring_retired': CHROME_WIRING,
    'surface_count': 12,
}


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])


def snapshot(ref):
    sha = git('rev-parse', '--verify', ref + '^{commit}').decode().strip()
    with tarfile.open(fileobj=io.BytesIO(git('archive', sha))) as archive:
        files = {entry.name: archive.extractfile(entry).read().decode('utf-8')
                 for entry in archive if entry.isfile()
                 and entry.name.endswith(('.rs', '.toml', '.md', '.json'))}
    return sha, files


def production_flags(source):
    """Literal port of guards::size::production_flags (column-zero rule)."""
    lines = source.splitlines()
    flags = [True] * len(lines)
    index = 0
    while index < len(lines):
        if lines[index] != '#[cfg(test)]':
            index += 1
            continue
        flags[index] = False
        index += 1
        while index < len(lines) and (
                lines[index].startswith(('#[', '//')) or not lines[index]):
            flags[index] = False
            index += 1
        if index == len(lines):
            break
        terminal = lines[index].endswith(';')
        flags[index] = False
        index += 1
        if terminal:
            continue
        while index < len(lines) and lines[index] not in ('}', '};'):
            flags[index] = False
            index += 1
        if index < len(lines):
            flags[index] = False
        index += 1
    return flags


def test_module_files(files):
    """Follow top-level cfg(test) external modules, including renamed helpers."""
    declaration = r'(?P<attrs>(?:#\[[^\n]+\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod (?P<name>\w+);'
    gated = re.compile(r'^#\[cfg\(test\)\]\s*' + declaration, re.M)
    external = re.compile(r'^' + declaration, re.M)
    found = set()
    pending = []

    def follow(path, pattern):
        for match in pattern.finditer(files[path]):
            explicit = re.search(r'#\[path\s*=\s*"([^"]+)"\]', match['attrs'])
            parent = posixpath.dirname(path)
            if explicit:
                candidates = [posixpath.normpath(parent + '/' + explicit[1])]
            else:
                base = parent if Path(path).stem in ('mod', 'lib', 'main') else path[:-3]
                candidates = [base + '/' + match['name'] + '.rs',
                              base + '/' + match['name'] + '/mod.rs']
            for target in candidates:
                if target in files and target.startswith('crates/app/') and target not in found:
                    found.add(target)
                    pending.append(target)

    for path in files:
        if path.startswith('crates/app/') and path.endswith('.rs'):
            follow(path, gated)
    while pending:
        follow(pending.pop(), external)
    return found


def classify(path, source, test_modules=()):
    raw = len(source.splitlines())
    if path.startswith('crates/app/tests/'):
        category = 'app/tests'
    elif '/tests/' in path or path in test_modules:
        category = 'src/tests'
    else:
        category = 'inline'
    production = sum(production_flags(source)) if category == 'inline' else 0
    return {'raw': raw, 'production': production, 'tests': raw - production,
            'test_category': category}


def structs(source):
    """Inventory named, multiline Rust structs, including conditional fields.

    Counts are declaration counts, not a particular feature configuration.
    Keep field types to expose app-owned wrappers during subsequent audits.
    """
    result = {}
    for match in re.finditer(r'^\s*(?:pub(?:\([^\n]*?\))?\s+)?struct (\w+)[^;\n]*\{\n(.*?)^}',
                             source, re.M | re.S):
        fields = re.findall(r'^    (?:pub(?:\([^\n]*?\))?\s+)?(\w+):\s*(.+),\s*$',
                            match[2], re.M)
        result[match[1]] = dict(fields)
    return result


def measure(sha, files):
    test_modules = test_module_files(files)
    inventory = {path: classify(path, source, test_modules) for path, source in sorted(files.items())
                 if path.startswith('crates/app/') and path.endswith('.rs')}
    totals = {'raw': 0, 'production': 0, 'tests': 0,
              'guard_production': 0,
              'test_categories': {'app/tests': 0, 'src/tests': 0, 'inline': 0}}
    areas = {}
    owners = {}
    for path, counts in inventory.items():
        area = path.removeprefix('crates/app/').split('/')
        area = '/'.join(area[:2]) if len(area) > 2 else '/'.join(area)
        group = areas.setdefault(area, {'raw': 0, 'production': 0, 'tests': 0})
        for key in ('raw', 'production', 'tests'):
            totals[key] += counts[key]
            group[key] += counts[key]
        totals['test_categories'][counts['test_category']] += counts['tests']
        if '/tests/' not in path:
            totals['guard_production'] += sum(production_flags(files[path]))
        production = '\n'.join(line for line, flag in
                               zip(files[path].splitlines(), production_flags(files[path])) if flag)
        if counts['production']:
            for name, fields in structs(production).items():
                owners[path + '::' + name] = fields
    app_fields = owners['crates/app/src/app.rs::QuantickApp']
    surface_inventory = {}
    for name in SURFACES:
        prefix = 'crates/app/src/surfaces/' + name
        paths = [p for p in inventory if p == prefix + '.rs' or p.startswith(prefix + '/')]
        test_paths = [p for p, c in inventory.items() if c['tests'] and
                      (p in paths or name in Path(p).stem)]
        surface_inventory[name] = {'paths': paths, 'test_paths': test_paths,
                                   'test_inventory': {p: inventory[p]['tests'] for p in test_paths},
                                   'production': sum(inventory[p]['production'] for p in paths),
                                   'tests': sum(inventory[p]['tests'] for p in paths)}
    return {'schema_version': 1, 'commit': sha, 'targets': TARGETS,
            'totals': totals, 'areas': areas, 'files': inventory,
            'test_only_modules': sorted(test_modules),
            'quantick_app_fields': app_fields, 'quantick_app_field_count': len(app_fields),
            'app_owner_structs': owners,
            'wiring_paths': sorted(p for p in files if p.endswith('_wiring.rs')),
            'surfaces': surface_inventory,
            'surface_module_declarations': re.findall(
                r'^pub\(crate\) mod (\w+);',
                files.get('crates/app/src/surfaces/mod.rs', ''), re.M),
            'existing_widget_adapter': 'crates/app/src/widget_surfaces/mod.rs',
            'wiring_ownership': {
                'chrome': CHROME_WIRING,
                'mixed_domain_adapter': {'layout_wiring':
                    'Session layout effects, indicator fan-out and drawing persistence; '
                    'retain domain adapter, migrate chrome callers.'}},
            'pane_header_tests': {p: c['tests'] for p, c in inventory.items()
                                  if 'pane_header' in p}}


def check_recipe(files, recipe_path):
    """Recipe carries an executable manifest in its fenced JSON block."""
    text = files.get(recipe_path, '')
    block = re.search(r'```json\s*(.*?)\s*```', text, re.S)
    if not block:
        raise ValueError('recipe must contain a fenced JSON docking manifest')
    recipe = json.loads(block[1])
    required = ('model', 'view', 'catalog', 'registration')
    if any(not isinstance(recipe.get(k), str) or not recipe[k] for k in required):
        raise ValueError('recipe requires model, view, catalog and registration strings')
    for key in required[:3]:
        if recipe[key] not in files:
            raise ValueError('recipe references missing ' + recipe[key])
    if not recipe['model'].startswith('crates/ui-model/'):
        raise ValueError('recipe model must live in ui-model')
    if not recipe['view'].startswith('crates/app/'):
        raise ValueError('recipe view must live in app')
    registration = recipe['registration']
    if '\n' in registration or files[recipe['catalog']].splitlines().count(registration) != 1:
        raise ValueError('recipe registration must match exactly one shipped catalog line')
    for key in ('model', 'view'):
        if Path(recipe[key]).stem not in registration:
            raise ValueError('registration must name the model and view modules')


def check_targets(current, baseline, files, chrome=False, final=False):
    failures = []
    if current['quantick_app_field_count'] > TARGETS['quantick_app_fields_max']:
        failures.append('QuantickApp has more than 30 fields')
    if set(current['wiring_paths']) - set(baseline['wiring_paths']):
        failures.append('new *_wiring.rs paths')
    if chrome or final:
        for stem in CHROME_WIRING:
            if any(Path(p).stem == stem for p in current['wiring_paths']):
                failures.append('chrome wiring remains: ' + stem)
    if final:
        for key in ('production', 'guard_production', 'tests'):
            if current['totals'][key] >= baseline['totals'][key]:
                failures.append('app ' + key + ' must be strictly below baseline')
        models = {p: s for p, s in files.items() if p.startswith('crates/ui-model/')}
        if not models:
            failures.append('ui-model is missing')
        forbidden = (r'\b(?:egui|eframe|SystemTime)\b|'
                     r'\bInstant\s*::\s*now\b|\bstd\s*::\s*thread\s*::\s*sleep\b')
        for path, source in models.items():
            if path.endswith('.rs'):
                source = re.sub(r'//[^\n]*|/\*.*?\*/', '', source, flags=re.S)
            if path.endswith(('.rs', '.toml')) and re.search(forbidden, source):
                failures.append('UI or clock reference in ' + path)
    if failures:
        raise ValueError('; '.join(failures))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ref', default='HEAD', help='commit or Git ref (never working tree)')
    parser.add_argument('--check-baseline', type=Path)
    parser.add_argument('--compare', type=Path, help='baseline JSON; print numeric deltas')
    parser.add_argument('--check-chrome', action='store_true')
    parser.add_argument('--check-recipe', nargs='?', const=RECIPE)
    parser.add_argument('--check-final', action='store_true')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    sha, files = snapshot(args.ref)
    current = measure(sha, files)
    try:
        if args.check_baseline:
            expected = json.loads(args.check_baseline.read_text(encoding='utf-8'))
            if current != expected:
                raise ValueError('baseline differs from measured Git snapshot')
        if args.compare or args.check_chrome or args.check_final:
            baseline_path = args.compare or ROOT / 'docs/headless-ui/baseline.json'
            baseline = json.loads(baseline_path.read_text(encoding='utf-8'))
            check_targets(current, baseline, files, args.check_chrome, args.check_final)
            current['delta'] = {k: current['totals'][k] - baseline['totals'][k]
                                for k in ('raw', 'production', 'guard_production', 'tests')}
        if args.check_recipe or args.check_final:
            check_recipe(files, args.check_recipe or RECIPE)
    except (ValueError, KeyError) as error:
        parser.exit(1, str(error) + '\n')
    rendered = json.dumps(current, indent=2, sort_keys=True) + '\n'
    if args.output:
        args.output.write_text(rendered, encoding='utf-8')
    elif not (args.check_baseline or args.check_chrome or args.check_recipe or args.check_final):
        print(rendered, end='')
    else:
        print('PASS ' + sha)


if __name__ == '__main__':
    main()
