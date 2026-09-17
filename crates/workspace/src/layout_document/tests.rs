use super::*;
use crate::indicator_document::{SavedInput, SavedKind};
fn ema() -> SavedIndicator {
    SavedIndicator {
        kind: SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: vec![SavedInput::Int(20), SavedInput::Source("close".to_owned())],
        plot_styles: Vec::new(),
    }
}

#[test]
fn starter_document_bytes_are_stable() {
    let book = LayoutBook::starter(vec![ema()]);
    assert_eq!(
        toml::to_string_pretty(&book).unwrap(),
        concat!(
            "version = 1\nactive = 1\nnext_id = 2\n\n",
            "[[layouts]]\nid = 1\nname = \"Layout 1\"\n\n",
            "[[layouts.indicators]]\nhidden = false\n\n",
            "[layouts.indicators.kind.native]\nid = \"native.ema\"\n\n",
            "[[layouts.indicators.inputs]]\ntype = \"int\"\nvalue = 20\n\n",
            "[[layouts.indicators.inputs]]\ntype = \"source\"\nvalue = \"close\"\n",
        )
    );
}
#[test]
fn a_starter_book_has_one_active_layout_named_for_its_number() {
    let book = LayoutBook::starter(vec![ema()]);
    assert_eq!(book.layouts().len(), 1);
    assert_eq!(book.active().name, "Layout 1");
    assert_eq!(book.active().indicators, vec![ema()]);
}
#[test]
fn the_strip_is_bounded_and_the_last_layout_stays() {
    let mut book = LayoutBook::default();
    for _ in 1..MAX_LAYOUTS {
        book.create(None).unwrap();
    }
    assert_eq!(book.create(None), Err(LayoutError::TooMany));
    assert_eq!(book.delete(LayoutId(999)), Err(LayoutError::Unknown));
    let only = LayoutBook::default();
    let mut only = only;
    assert_eq!(only.delete(only.active_id()), Err(LayoutError::Last));
}
#[test]
fn deleting_the_active_layout_hands_over_to_its_left_neighbour() {
    let mut book = LayoutBook::default();
    let second = book.create(None).unwrap();
    let third = book.create(None).unwrap();
    book.switch(third).unwrap();
    book.delete(third).unwrap();
    assert_eq!(book.active_id(), second);
    book.delete(LayoutId(1)).unwrap();
    assert_eq!(
        book.active_id(),
        second,
        "deleting another layout moves nothing"
    );
    assert_eq!(
        book.switch(second),
        Ok(false),
        "switching to the active one is not a change"
    );
}

#[test]
fn creating_takes_the_first_free_default_name() {
    let mut book = LayoutBook::default();
    let second = book.create(None).unwrap();
    assert_eq!(book.get(second).unwrap().name, "Layout 2");
    book.delete(second).unwrap();
    let again = book.create(None).unwrap();
    assert_ne!(again, second, "an id is never reborn");
    assert_eq!(
        book.get(again).unwrap().name,
        "Layout 2",
        "but the name is free again"
    );
}
#[test]
fn names_are_cleaned_unique_and_bounded() {
    let mut book = LayoutBook::default();
    let id = book.create(Some("  open   session ")).unwrap();
    assert_eq!(book.get(id).unwrap().name, "open session");
    assert_eq!(
        book.create(Some("open  session")),
        Err(LayoutError::Duplicate)
    );
    assert_eq!(book.create(Some("   ")), Err(LayoutError::Empty));
    assert_eq!(book.rename(id, "Layout 1"), Err(LayoutError::Duplicate));
    assert_eq!(
        book.rename(id, "open session"),
        Ok(false),
        "the same name is not a change"
    );
    let long = "x".repeat(MAX_LAYOUT_NAME + 10);
    assert_eq!(book.rename(id, &long), Ok(true));
    assert_eq!(book.get(id).unwrap().name.chars().count(), MAX_LAYOUT_NAME);
}
