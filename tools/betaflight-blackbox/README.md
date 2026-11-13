# The Blackbox format of a Betaflight release, from its source

`extract.py` reads out of a Betaflight checkout what the log writer
(`crates/openpid-fmt-blackbox/src/write/`, `docs/DESIGN.md` §5) has to know
about a release: the field definitions of the `I`/`P`, `G` and `H` frames and
the system-information header lines with the build options they depend on.

```sh
git clone --depth 1 https://github.com/betaflight/betaflight
git -C betaflight fetch --depth 1 origin tag 2026.6.2
python3 tools/betaflight-blackbox/extract.py betaflight 2026.6.2
```

Adding a release to the writer:

1. Run the script for the new tag and for the newest release the writer has;
   compare the two outputs.
2. Field definitions: change `version.rs` so that its tables give the new
   lines, and add those lines to `src/write/field-tables.txt` (prefixed with
   the name of the target); the unit test `the_tables_are_the_firmware_s`
   holds the two together.
3. Header lines: `src/write/sysinfo/lines.txt` is one table for all
   releases, each line once, with the releases that write it and its
   conditions. A span that is open at the end (`4.3-`) already includes the
   new release, so only what changed needs an edit: close the span of a line
   the release dropped (`4.3-2026.6`), add a row for a new line
   (`<release>-`) where the firmware writes it, and for a line whose
   conditions or place changed close the old row and add a new one. The
   table has no values: the writer writes a line when it knows its value.
   What a release fixes itself (`Firmware type`, the version and commit in
   `Firmware revision`, a new constant line) goes into `version.rs` or
   `config.rs`, with the file it was read from. The unit tests of
   `sysinfo/mod.rs` check the table (well-formed, the header lists of the
   fixtures).
4. Read what the script cannot see, in `src/main/blackbox/blackbox.c` at the
   tag: `testBlackboxConditionUncached` (the conditions and the bits of
   `fields_disabled_mask`, the enum in `blackbox_fielddefs.h`),
   `loadMainState` (units), `blackboxInit` and `blackboxLogIteration` (the
   schedule), `blackboxLogEvent` (events). `docs/BLACKBOX-VERSIONS.md` has the
   table of what differed between the releases so far: add a column.
5. A log of the new release, if one exists under an MIT-compatible licence,
   goes into `tests/logs/` and into `tests/rewrite.rs`: written back, it must
   come out byte for byte.

No firmware code is copied: the script takes names, numbers and
preprocessor conditions, which are the format.
