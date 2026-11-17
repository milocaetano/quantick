use super::{Ground, SavedDefault};

/// A save on the light chart says it stops short of the dark pane beside it.
#[test]
fn a_saved_default_names_the_charts_it_speaks_for() {
    let saved = SavedDefault::OneTool(Ground::Light);
    assert!(
        saved.message().contains("on light charts"),
        "{}",
        saved.message()
    );
    assert!(
        saved
            .hint("trend line")
            .contains("trend line objects on light charts")
    );
    let reset = SavedDefault::Forgotten(Ground::Dark);
    assert!(reset.message().contains("on dark charts"));
    assert!(reset.hint("fib").contains("default preset choice are kept"));
}
