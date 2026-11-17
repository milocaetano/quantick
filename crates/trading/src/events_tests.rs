use super::*;

#[test]
fn a_venue_refusal_keeps_the_brokers_words_and_stays_copy() {
    let reason = RejectReason::Venue {
        code: Some(10016),
        message: VenueText::new("Invalid stops"),
    };
    let copied = reason;
    assert_eq!(copied, reason);
    assert_eq!(
        reason.to_string(),
        "the broker refused: Invalid stops (code 10016)"
    );
}

#[test]
fn long_venue_text_is_cut_at_a_character_and_says_so() {
    let long = "é".repeat(VENUE_TEXT_BYTES);
    let text = VenueText::new(&long);
    assert!(text.is_truncated());
    assert_eq!(text.as_str().len(), VENUE_TEXT_BYTES);
    assert!(text.to_string().ends_with("..."));
    // Under clippy's large-error bound: refusals travel in `Result`s.
    assert!(std::mem::size_of::<RejectReason>() < 128);
    let short = VenueText::new("No money");
    assert!(!short.is_truncated());
    assert_eq!(short.to_string(), "No money");
}
