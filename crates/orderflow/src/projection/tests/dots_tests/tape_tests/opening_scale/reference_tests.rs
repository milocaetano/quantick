//! Opening-reference shortcuts must preserve every retained output field.

use super::*;

#[test]
fn expired_opening_metadata_leaves_the_complete_retained_frame_unchanged() {
    let mut marks = vec![
        native(1, 717, 100, 100, 75),
        native(2, 801, 100, 25, 5),
        native(3, 3_201, 170, 25, 5),
    ];
    let mut ordinary = TapeDotMemory::default();
    let mut expired = TapeDotMemory::default();
    for pass in 0..3 {
        let expected = project(&mut ordinary, &marks, &[], None);
        let actual = project(&mut expired, &marks, &[-100, 0, 0], None);
        assert_eq!(
            actual.marks, expected.marks,
            "all facts, positions and sizes at pass {pass}"
        );
        assert_eq!(actual.full_quantity, expected.full_quantity);
        assert_eq!(actual.max_radius, expected.max_radius);
        if pass == 0 {
            marks[1] = native(2, 801, 100, 30, 10);
        } else if pass == 1 {
            marks.push(native(4, 3_601, 175, 10, 3));
        }
    }
}

#[test]
fn a_retained_mixed_group_counts_eligible_quantity_once_with_duplicate_opening_metadata() {
    let marks = [
        native(1, 717, 100, 74_365, 74_365),
        native(2, 801, 100, 100, 0),
        native(3, 901, 100, 25, 0),
        native(4, 3_201, 170, 25, 5),
    ];
    let mut memory = TapeDotMemory::default();
    for _ in 0..2 {
        let frame = project(&mut memory, &marks, &[700, 0, 700, 0], None);
        let mixed = with_id(&frame, 1);
        assert_eq!(mixed.agg_ids, [1, 2, 3]);
        assert_eq!(mixed.quantity, Decimal::from(74_490));
        assert_eq!(mixed.buy_quantity, Decimal::from(74_365));
        assert_eq!(mixed.price, Decimal::from(100));
        assert_eq!(
            mixed.timestamp_quantity,
            marks[..3]
                .iter()
                .map(|mark| mark.timestamp_quantity)
                .sum::<Decimal>()
        );
        assert_eq!(
            frame.full_quantity,
            Decimal::from(125),
            "exclude each opening source once; retain every ordinary member"
        );
        assert_eq!(
            with_id(&frame, 4).size,
            crate::projection::normalized_area_size(Decimal::from(25), Decimal::from(125))
        );
        assert_eq!(
            frame
                .marks
                .iter()
                .map(|mark| mark.quantity)
                .sum::<Decimal>(),
            Decimal::from(74_515)
        );
    }
}
