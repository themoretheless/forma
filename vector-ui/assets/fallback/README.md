# Portable fallback font assets

Unmodified fonts from Google Fonts, pinned to revision `2fe1e8c6b137fb13041a7cfca6fdcd8a910ea4dc`.
`PROVENANCE.json` records immutable download URLs, sizes and SHA-256 hashes.
Each font includes its upstream OFL and METADATA files.

- NotoSansSC.ttf: 17,772,300 bytes; CJK vector outlines.
- NotoEmoji.ttf: 1,982,596 bytes; monochrome emoji vector outlines.

`cargo run --offline --locked --manifest-path vector-ui/Cargo.toml --example font_coverage_probe`
checks weight400, glyph lookup, positive advances and nonempty outlines for
sample CJK/emoji scalars. The default simple text renderer now uses Ubuntu,
then NotoSansSC and the static FormaEmoji derivative. It normalizes advances to primary-font units
and scales outlines by each selected face's units per em. Native CPU rendering
and mixed-font caret/vector parity have regression evidence. The shaped API uses the same faces and exposes font_index; full
language/emoji sequence coverage and colour emoji remain open.

Both fonts add 19,754,896 bytes before compression. Fresh vector-ui WASM Node14/14 and Chromium17/17 passed. Generated module
size is21,828,894bytes (gzip13,246,694bytes). Controlled startup and resolver
performance, broader font metrics/sequence qualification remain open.

Static weight400 candidates are generated with
`/path/to/fonttools-venv/bin/python vector-ui/scripts/instance-fallback-fonts.py`
using exactly `fonttools==4.68.0`. `STATIC_INSTANCES.json` records source/output
hashes. Two generations reproduced identical bytes; all cmap tables (including
UVS data) and glyph order are preserved. The renamed FormaCJK/FormaEmoji
derivatives retain upstream license/copyright records. FormaEmoji is now used by the renderer; FormaCJK remains experimental. `font_instance_compare` exhaustively rejects changed outline
topology, advances or coordinate error above 0.5 font units. Current CJK error
is 0.49981689453125 units; emoji is exact. Raster output still needs qualification.

Static emoji production switch:240 Rust tests passed (2 GPU and1 manual probe
ignored), Node14/14,90 exact WASM frame/caret comparisons against variable-font
baseline. Chromium17/17 and actual macOS mixed-paste presentation passed. Payload saved1,095,944bytes;
see packages/svet/benchmarks/static-emoji-qualification-2026-11-03.json for size.
