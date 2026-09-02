# Dimension 4 — where a test lives

Read before writing a test-layout finding. The rule that every new behaviour has a failing-without-it test is in `SKILL.md`; this is the Rust layout that rule is graded against.


- **Unit tests** live in a `#[cfg(test)] mod tests` child of the module under
  test — inline at the bottom of the file, or, once that module outgrows the
  file it is buried in, in a sibling `tests.rs` pulled in with
  `#[cfg(test)] mod tests;`. The attribute is the whole point: it keeps the
  tests out of `cargo build` and out of the shipped binary, and it is what buys
  access to private items with no `[InternalsVisibleTo]` equivalent needed. A
  test module without `#[cfg(test)]` is a finding on its own.
- **Integration tests** live in `crates/<crate>/tests/*.rs` — a separate crate
  that links the library from outside and therefore can only reach the public
  API, which is precisely what makes it a contract test. That is where this
  repo already proves its contracts: `engine/tests/golden_*.rs`,
  `control/tests/*_contract.rs`, `pine/tests/*_semantics.rs`,
  `indicators/tests/fmath_guard.rs`. An integration test that needs a private
  item is either a unit test filed in the wrong folder, or the signal that the
  port under review was never made public — say which.
- **Test support an integration test needs** cannot be `#[cfg(test)]` — that
  attribute is false when the separate test crate compiles, so the helper would
  vanish exactly where it is wanted. Other Rust projects reach for a
  `test-util` cargo feature; **this repo does not, and proposing one is the
  finding, not the fix** — no crate here has a `[features]` section at all.
  The repo's answer is a *deliberately published* module, documented as part of
  what the crate is: `engine::fixture` and `engine::golden`, and
  `control::fake`, whose fake host/client ports `AGENTS.md` names in the
  crate's own description. Per-file helpers used by one integration test go in
  `tests/common/mod.rs`, never a top-level `tests/common.rs` — cargo builds
  every top-level file in `tests/` as its own test binary, so the flat version
  compiles as a test target containing no tests.

So the line this dimension draws is not "test code in `src/` is bad". It is
**deliberate and documented, or accidental and leaking**. The findings, in
order of how much damage they do:

- **`#[cfg(test)]` that changes behaviour instead of only adding tests** — a
  branch, a shortened timeout, a stubbed clock or a skipped validation inside
  production logic. Then the thing under test is not the thing that ships and
  the suite proves nothing about the binary; a **Blocker**, and it collides
  with priority 0 besides. The fix is a seam, not a flag: pass the clock in,
  take the trait, hand the value to the constructor — the pattern `replay`
  already follows by being *told* how much time passed rather than reading a
  clock.
- **A `pub` item on a production type whose only callers are tests**, added for
  one test's convenience and documented nowhere. Either gate it `#[cfg(test)]`,
  move it inside the test module, or publish it deliberately the way
  `engine::fixture` is published — with a doc comment saying it is test support
  and who is meant to call it. The finding is the undeclared widening, not the
  existence of test support.
- **A test asserting on a private detail from the outside**, reached by
  loosening visibility for the test's benefit. The visibility change is the
  finding, not the assertion.

Before filing any of these, check the crate's `lib.rs` and `CLAUDE.md`: a
module the architecture names on purpose is not a leak, and calling one a leak
is the review inventing a second way to do a solved thing.

