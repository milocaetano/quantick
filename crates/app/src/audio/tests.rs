use super::*;

/// Tokens are the preset file's vocabulary: every sound round-trips,
/// and an unknown one is refused rather than quietly becoming the
/// default. A preset that says "critical" and plays "information" is an
/// alarm the trader cannot trust.
#[test]
fn every_sound_round_trips_through_its_stored_token() {
    for sound in AlertSound::all() {
        assert_eq!(AlertSound::from_token(sound.token()), Some(sound));
    }
    assert_eq!(AlertSound::from_token("klaxon"), None);
    assert_eq!(AlertSound::from_token(""), None);
}

/// Two sounds sharing a token would make one of them unreachable from a
/// saved preset; two sharing a label would make the picker a guess.
#[test]
fn the_sound_set_has_no_duplicates() {
    let mut tokens: Vec<&str> = AlertSound::all().map(|sound| sound.token()).collect();
    tokens.sort_unstable();
    let named = tokens.len();
    tokens.dedup();
    assert_eq!(tokens.len(), named, "two sounds share a stored token");

    let mut labels: Vec<&str> = AlertSound::all().map(|sound| sound.label()).collect();
    labels.sort_unstable();
    let named = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), named, "two sounds share a picker label");
}

/// The picker's groups cover the set exactly once, in the order the
/// headings are listed, so a sound is neither under two headings nor
/// under none — and no clip claims the system heading.
#[test]
fn the_categories_partition_the_sounds_in_picker_order() {
    let grouped: Vec<AlertSound> = SoundCategory::ALL
        .into_iter()
        .flat_map(AlertSound::in_category)
        .collect();
    let all: Vec<AlertSound> = AlertSound::all().collect();
    assert_eq!(grouped, all);
    assert_eq!(
        all.len(),
        AlertSound::PLATFORM.len() + quantick_strategy::sound::CLIPS.len()
    );
    assert!(
        AlertSound::in_category(SoundCategory::System).all(|sound| !sound.can_be_cut()),
        "a platform beep cannot be cut, and no clip is filed under system"
    );
    assert!(
        AlertSound::in_category(SoundCategory::Nature).all(AlertSound::can_be_cut),
        "every clip can be cut"
    );
}

/// The default is the sound this platform has always made here, so a
/// preset that never names one behaves as the app did before the choice
/// existed — and a cue that never names a length plays it whole.
#[test]
fn the_defaults_are_what_the_app_already_did() {
    assert_eq!(AlertSound::default(), AlertSound::Information);
    assert_eq!(Cue::default(), Cue::whole(AlertSound::Information));
    assert_eq!(
        Cue::cut_after(AlertSound::Beep, 5).length,
        PlayLength::Capped(Duration::from_secs(5))
    );
}

/// A cut applies to a clip and not to a beep: the same beep asked for
/// with and without seconds is one cue, so two presets that agree on
/// the sound cannot make the platform play it twice in a frame.
#[test]
fn a_cut_only_reaches_a_sound_that_can_be_cut() {
    let clip = AlertSound::in_category(SoundCategory::Standard)
        .next()
        .expect("a standard clip");
    assert_eq!(Cue::new(clip, Some(4)), Cue::cut_after(clip, 4));
    assert_eq!(Cue::new(clip, None), Cue::whole(clip));
    assert_eq!(
        Cue::new(AlertSound::Critical, Some(4)),
        Cue::whole(AlertSound::Critical)
    );
    assert_eq!(
        Cue::new(AlertSound::Critical, Some(4)),
        Cue::new(AlertSound::Critical, None)
    );
}

/// The port's other implementation records what it was asked for, in
/// order — the whole reason the alarm can be tested at all.
#[test]
fn the_recording_sink_keeps_what_it_was_asked_to_play() {
    let sink = RecordingAlerts::default();
    // The handle the test keeps and the one the app is given are the
    // same recorder — the whole point of sharing it.
    let mut installed = sink.clone();
    let first = Cue::whole(AlertSound::Critical);
    let second = Cue::cut_after(AlertSound::Beep, 3);
    installed.warm_up(first);
    assert_eq!(installed.play(&[first]), Ok(()));
    assert_eq!(installed.play(&[second]), Ok(()));
    assert_eq!(sink.cues(), vec![first, second]);
    assert_eq!(sink.sounds(), vec![AlertSound::Critical, AlertSound::Beep]);
    assert_eq!(sink.warmed_up(), vec![first]);
}
