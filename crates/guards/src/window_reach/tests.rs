use super::*;
use crate::scratch_dir::ScratchDir;

#[test]
fn an_impl_on_the_window_is_reach_whether_inherent_or_trait() {
    let source = "\
impl QuantickApp {
    fn layout_adapter(&mut self) {}
}
impl Port for super::QuantickApp {
    fn act(&mut self, other: &mut QuantickApp) {}
}
impl<'a> MarkPort for dyn ControlPort + 'a {}
impl Port for LayoutAdapter<'_> {}
";
    // Three window impls; the method inside one is not counted again.
    assert_eq!(reach_of(source), 3);
}

#[test]
fn a_function_taking_the_window_by_reference_is_reach() {
    let source = "\
fn apply(app: &mut QuantickApp, request: Request) {}
pub(crate) fn read<'a>(window: &'a ControlWindow) -> u8 { 0 }
fn bridge(window: &mut dyn ControlPort) {}
fn qualified(app: &crate::app::QuantickApp) {}
fn owned(state: &mut AlertRuntime, app_name: QuantickAppName) {}
fn by_value(app: QuantickApp) {}
type Hook = fn(&mut QuantickApp);
impl Owner {
    fn method(&self, app: &QuantickApp) {}
}
";
    assert_eq!(reach_of(source), 5);
}

#[test]
fn a_struct_holding_the_window_by_reference_is_reach() {
    let source = "\
pub struct LayoutAdapter<'a> { app: &'a mut QuantickApp }
struct View<'a>(&'a dyn ControlPort);
struct Owned { app: Box<QuantickApp> }
struct Unit;
";
    assert_eq!(reach_of(source), 2);
}

#[test]
fn test_items_comments_and_strings_are_not_reach() {
    let source = "\
// impl QuantickApp {}
const TEXT: &str = \"fn apply(app: &mut QuantickApp) {}\";
#[cfg(test)]
mod tests {
    impl QuantickApp {}
    fn helper(app: &mut QuantickApp) {}
}
";
    assert_eq!(reach_of(source), 0);
}

fn tree(app: &str, baseline: &str) -> ScratchDir {
    let root = ScratchDir::new("window-reach");
    fs::create_dir_all(root.join("crates/app/src/app/tests")).unwrap();
    fs::create_dir_all(root.join("crates/guards")).unwrap();
    fs::write(root.join("crates/app/src/app.rs"), app).unwrap();
    fs::write(
        root.join("crates/app/src/app/tests/app_tests.rs"),
        "impl QuantickApp {}\n",
    )
    .unwrap();
    fs::write(root.join(BASELINE_FILE), baseline).unwrap();
    root
}

const AT_TWO: &str = "!budget 2\nwindow-reach 2\n";

#[test]
fn the_ceiling_holds_and_a_test_file_is_not_counted() {
    let root = tree("impl QuantickApp {}\nfn f(app: &QuantickApp) {}\n", AT_TWO);
    assert_eq!(measured(&root).unwrap(), 2);
    assert!(check(&root).is_empty());
}

#[test]
fn a_new_impl_on_the_window_fails_naming_the_target_shape() {
    let root = tree(
        "impl QuantickApp {}\nfn f(app: &QuantickApp) {}\nimpl Port for QuantickApp {}\n",
        AT_TWO,
    );
    let findings = check(&root);
    assert_eq!(findings.len(), 1, "{:?}", findings.iter().map(|f| &f.line));
    assert!(
        findings[0].line.contains("window-reach: 3"),
        "{}",
        findings[0].line
    );
    for exemplar in ["LayoutAdapter", "AlertDefinitionsPort::alert_runtime"] {
        assert!(findings[0].remedy.contains(exemplar));
    }
    assert_eq!(check_file(&root, "crates/app/src/app.rs").len(), 1);
}

#[test]
fn a_fall_asks_for_tighten_and_tighten_writes_it() {
    let root = tree("impl QuantickApp {}\n", AT_TWO);
    let findings = check(&root);
    assert!(
        findings.iter().any(|f| f.line.contains("tighten")),
        "{:?}",
        findings.iter().map(|f| &f.line).collect::<Vec<_>>()
    );
    tighten(&root).unwrap();
    assert!(check(&root).is_empty());
}

#[test]
fn an_edit_outside_the_app_source_asks_nothing() {
    let root = tree(
        "impl QuantickApp {}\nimpl Port for QuantickApp {}\n",
        "!budget 0\n",
    );
    assert!(check_file(&root, "crates/chart/src/lib.rs").is_empty());
    assert!(check_file(&root, "crates/app/src/app/tests/app_tests.rs").is_empty());
    assert!(!check_file(&root, "crates/app/src/app.rs").is_empty());
}

#[test]
fn a_test_field_or_argument_does_not_hide_what_follows_it() {
    let source = "\
pub struct Access {
    #[cfg(test)]
    observed: usize,
}
impl Access {
    fn execute(&mut self, window: &mut ControlWindow) {}
}
fn build() {
    launch(
        #[cfg(any(feature = \"drawing-harness\", test))]
        toolrail,
    );
}
impl eframe::App for QuantickApp {}
#[cfg(any(feature = \"control-harness\", test))]
fn drive(app: &mut QuantickApp) {}
";
    assert_eq!(reach_of(source), 3);
}

#[test]
fn every_reference_spelling_of_the_window_is_reach() {
    let source = "\
impl<'a> SomePort for &'a mut QuantickApp {}
fn by_impl(app: &mut impl ControlPort) {}
fn parenthesised<'a>(app: &mut (dyn ControlPort + 'a)) {}
enum Target<'a> { App(&'a mut QuantickApp), Other }
";
    assert_eq!(reach_of(source), 4);
}
