"""Tests for the pure side-inference policy in export_session.py.

The repo's CI is cargo-only, so these do not run there; they exist so the
policy can be proven without a logged-in terminal. Run either way:

    python tools/mt5/test_export_session.py
    python -m pytest tools/mt5/test_export_session.py
"""

from datetime import datetime, timezone

from export_session import (
    fill_missing_sides,
    flags_are_one_sided,
    rederive_sides,
    spread_side,
)

STAMP = datetime(2026, 8, 13, 12, 0, 0, tzinfo=timezone.utc)


def tape(*rows):
    """(price, bid, ask, side) → the print tuple export_tape builds."""
    return [(STAMP, price, bid, ask, 1.0, side) for price, bid, ask, side in rows]


def sides(prints):
    return [side for *_, side in prints]


def test_one_sided_detection_needs_proof_and_tolerates_typos():
    # A handful of one-sided prints proves nothing.
    assert not flags_are_one_sided(10, 0)
    # A whole stamped session does — including one stamped with a few typos.
    assert flags_are_one_sided(750_000, 0)
    assert flags_are_one_sided(2_000, 3)
    # A market with both sides present keeps its flags.
    assert not flags_are_one_sided(1_500, 500)


def test_tick_rule_rederives_a_stamped_day():
    prints = tape(
        (100.0, 0.0, 0.0, "B"),  # leader: no movement yet
        (101.0, 0.0, 0.0, "B"),  # uptick → B
        (100.0, 0.0, 0.0, "B"),  # downtick → S
        (100.0, 0.0, 0.0, "B"),  # unchanged → carries S
    )
    rederived, inferred = rederive_sides(prints)
    assert sides(rederived) == ["B", "B", "S", "S"]
    assert inferred == 4


def test_leaders_inherit_the_first_decided_side():
    prints = tape(
        (100.0, 0.0, 0.0, "B"),
        (100.0, 0.0, 0.0, "B"),
        (99.0, 0.0, 0.0, "B"),  # first movement: downtick → S
    )
    rederived, _ = rederive_sides(prints)
    assert sides(rederived) == ["S", "S", "S"]


def test_an_undecidable_day_refuses_to_export():
    prints = tape((100.0, 0.0, 0.0, ""), (100.0, 0.0, 0.0, ""))
    try:
        rederive_sides(prints)
    except ValueError:
        pass
    else:
        raise AssertionError("a made-up side would chart a lie")


def test_venue_gaps_fall_back_to_spread_then_carry_forward():
    prints = tape(
        (100.0, 99.0, 100.0, "B"),
        (100.0, 99.0, 100.0, ""),  # spread: at the ask → B
        (99.0, 99.0, 100.0, ""),  # spread: at the bid → S
        (99.5, 0.0, 0.0, ""),  # inside, no quote → carries S
    )
    filled, inferred = fill_missing_sides(prints)
    assert sides(filled) == ["B", "B", "S", "S"]
    assert inferred == 3


def test_spread_side_is_evidence_or_nothing():
    assert spread_side(100.0, 99.0, 100.0) == "B"
    assert spread_side(99.0, 99.0, 100.0) == "S"
    assert spread_side(99.5, 99.0, 100.0) == ""
    assert spread_side(100.0, 0.0, 0.0) == ""


if __name__ == "__main__":
    for name, test in sorted(globals().items()):
        if name.startswith("test_") and callable(test):
            test()
            print(f"ok - {name}")
