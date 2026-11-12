"""Deterministic fixtures and the C1 pinned-snapshot acceptance test."""
import json
import unittest

import measure


class ClassificationTests(unittest.TestCase):
    def test_column_zero_rule_matches_guard(self):
        source = ('use std::fs;\n#[cfg(test)]\nuse std::io;\n\n'
                  'fn ship() {}\nfn also_ships() {}\n')
        self.assertEqual(sum(measure.production_flags(source)), 4)

    def test_stacked_attributes_and_production_after_test(self):
        source = ('#[cfg(test)]\n#[allow(dead_code)]\n// fixture\n'
                  'fn helper() {\n    assert!(true);\n}\nfn ship() {}\n')
        self.assertEqual(sum(measure.production_flags(source)), 1)

    def test_indented_test_attribute_matches_guard(self):
        source = ('struct App {\n    #[cfg(test)]\n    probe: bool,\n}\n\n'
                  '#[cfg(test)]\nmod tests {\n}\n')
        self.assertEqual(sum(measure.production_flags(source)), 5)

    def test_all_test_categories_are_disjoint(self):
        source = 'fn ship() {}\n#[cfg(test)]\nmod tests {\n}\n'
        for path, category, production in [
                ('crates/app/tests/example.rs', 'app/tests', 0),
                ('crates/app/src/app/tests/example.rs', 'src/tests', 0),
                ('crates/app/src/foo/tests.rs', 'src/tests', 0),
                ('crates/app/src/foo.rs', 'inline', 1)]:
            counts = measure.classify(path, source, {'crates/app/src/foo/tests.rs'})
            self.assertEqual(counts['test_category'], category)
            self.assertEqual(counts['production'], production)
            self.assertEqual(counts['raw'], counts['production'] + counts['tests'])

    def test_owner_fields_include_wrappers_and_conditional_state(self):
        source = ('pub struct Owner {\n    wrapper: nested::Hub,\n'
                  '    #[cfg(test)]\n    pub(crate) probe: bool,\n}\n')
        self.assertEqual(measure.structs(source)['Owner'],
                         {'wrapper': 'nested::Hub', 'probe': 'bool'})

    def test_multiline_types_comments_and_one_line_structs(self):
        source = ('pub struct Owner<T> {\n    multi: BTreeMap<\n        String,\n'
                  '        Vec<T>,\n    >, // state\n    last: bool // no final comma\n}\n'
                  'struct Small { callback: fn(u8, u8) -> bool, name: &\'static str }')
        owners = measure.structs(source)
        self.assertEqual(set(owners['Owner']), {'multi', 'last'})
        self.assertEqual(owners['Small'], {'callback': 'fn(u8, u8) -> bool',
                                          'name': "&'static str"})
        source = 'struct QuantickApp {\n' + ''.join(
            f'    field{i}: bool, // owner state\n' for i in range(31)) + '}\n'
        self.assertEqual(len(measure.structs(source)['QuantickApp']), 31)

    def test_external_test_helpers_follow_module_ownership(self):
        files = {'crates/app/src/foo.rs': '#[cfg(test)]\nmod helper;\n',
                 'crates/app/src/foo/helper.rs': 'mod fixture;\n',
                 'crates/app/src/foo/helper/fixture.rs': 'fn fixture() {}\n',
                 'crates/app/src/bar.rs': '#[cfg(test)]\n#[path = "odd.rs"]\nmod helper;\n',
                 'crates/app/src/odd.rs': 'fn fixture() {}\n'}
        self.assertEqual(measure.test_module_files(files),
                         {'crates/app/src/foo/helper.rs',
                          'crates/app/src/foo/helper/fixture.rs', 'crates/app/src/odd.rs'})


class TargetTests(unittest.TestCase):
    def setUp(self):
        self.baseline = {'totals': {'production': 100, 'guard_production': 100, 'tests': 100},
                         'wiring_paths': []}
        self.current = {'totals': {'production': 99, 'guard_production': 99, 'tests': 99},
                        'wiring_paths': [],
                        'quantick_app_field_count': 14, 'app_owner_structs': {}}
        self.files = {'crates/ui-model/src/lib.rs': 'pub trait Model {}'}

    def test_final_requires_strict_reductions(self):
        measure.check_targets(self.current, self.baseline, self.files, final=True)
        self.current['totals']['tests'] = 100
        with self.assertRaisesRegex(ValueError, 'tests must be strictly'):
            measure.check_targets(self.current, self.baseline, self.files, final=True)

    def test_chrome_requires_retiring_owner_and_wiring(self):
        self.current['wiring_paths'] = ['crates/app/src/app/toolbar_wiring.rs']
        self.baseline['wiring_paths'] = self.current['wiring_paths']
        with self.assertRaisesRegex(ValueError, 'chrome wiring remains'):
            measure.check_targets(self.current, self.baseline, self.files, chrome=True)

    def test_new_wiring_and_fields_fail(self):
        self.current['quantick_app_field_count'] = 31
        self.current['wiring_paths'] = ['new_wiring.rs']
        with self.assertRaisesRegex(ValueError, '30 fields.*new'):
            measure.check_targets(self.current, self.baseline, self.files)

    def test_recipe_checks_actual_files_and_exact_registration(self):
        manifest = {'model': 'crates/ui-model/src/toast.rs',
                    'view': 'crates/app/src/toast.rs', 'catalog': 'crates/app/src/catalog.rs',
                    'registration': 'register!(toast, toast);'}
        files = {manifest['model']: '', manifest['view']: '',
                 manifest['catalog']: manifest['registration'] + '\n',
                 'recipe.md': '```json\n' + json.dumps(manifest) + '\n```'}
        measure.check_recipe(files, 'recipe.md')
        files[manifest['catalog']] = 'register!(other);'
        with self.assertRaisesRegex(ValueError, 'exactly one'):
            measure.check_recipe(files, 'recipe.md')

    def test_final_rejects_clock_and_missing_model(self):
        for files in ({}, {'crates/ui-model/src/lib.rs': 'let now = Instant :: now();'}):
            with self.assertRaises(ValueError):
                measure.check_targets(self.current, self.baseline, files, final=True)

    def test_headless_documentation_may_name_forbidden_dependencies(self):
        self.files['crates/ui-model/src/lib.rs'] = '// No egui or eframe.\npub trait Model {}'
        measure.check_targets(self.current, self.baseline, self.files, final=True)

    def test_url_literal_does_not_hide_a_clock_call(self):
        self.files['crates/ui-model/src/lib.rs'] = (
            'pub fn update() { let endpoint = "https://example.test"; '
            'let now = std::time::Instant::now(); }')
        with self.assertRaisesRegex(ValueError, 'clock reference'):
            measure.check_targets(self.current, self.baseline, self.files, final=True)

    def test_rust_literals_and_nested_comments_are_not_code(self):
        self.files['crates/ui-model/src/lib.rs'] = (
            '/* outer /* Instant::now() */ egui */\n'
            'const DOC: &str = r##"Instant::now() // eframe"##;\n'
            'const TEXT: &str = "escaped \\" quote and egui";\n'
            "const CH: char = '\\'';\n")
        measure.check_targets(self.current, self.baseline, self.files, final=True)


class AcceptanceTests(unittest.TestCase):
    def test_C1_pinned_baseline_is_reproducible(self):
        baseline = json.loads((measure.ROOT / 'docs/headless-ui/baseline.json').read_text())
        sha, files = measure.snapshot(baseline['commit'])
        first = measure.measure(sha, files)
        second = measure.measure(*measure.snapshot(baseline['commit']))
        self.assertEqual(first, second)
        self.assertEqual(first, baseline)
        self.assertEqual(first['quantick_app_field_count'], 14)
        self.assertEqual(len(first['surfaces']), 12)
        self.assertEqual(first['surface_module_declarations'], measure.SURFACES)
        self.assertEqual(sum(first['totals']['test_categories'].values()),
                         first['totals']['tests'])


if __name__ == '__main__':
    unittest.main()
