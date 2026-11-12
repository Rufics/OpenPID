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
3. Header lines: write `sysinfo/<release>.rs` from the list (name, order,
   conditions). Values: those of a real log of that release where there is
   one, else the firmware's defaults, with a comment saying where each was
   read. A line with no verified value is `open(...)`.
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
