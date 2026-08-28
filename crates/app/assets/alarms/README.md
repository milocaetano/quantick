# Alarm clips

The sound library the signal alarm picks from, shipped inside the binary
(`include_bytes!` in `crates/app/src/audio/library.rs`) so a preset that
names a clip plays the same clip on every machine.

- `standard/` — clips that behave like an alarm: beeps, phones, a cuckoo.
- `nature/` — ambient recordings: rain, surf, a steam train. Long, which is
  what the preset's *stop after N seconds* cut is for.

Files are AAC in an MP4 container (`.m4a`), the format the library was
supplied in; the app decodes them on play. A file's stem is the token a
preset stores, so stems are lower-case ASCII with hyphens and nothing else.

Adding a clip: drop the file in the right folder, add its row to `CLIPS`
in `library.rs`, and run `cargo test -p quantick-app audio` — the catalogue
test refuses a folder and a table that disagree, and the decode test
refuses a file the shipped decoder cannot read.

Origin: the trader's own alarm collection (the "Sample Alarms" set that
ships with the Alarm Clock application), supplied for inclusion here.
