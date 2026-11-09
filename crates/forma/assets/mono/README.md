# Monospace face

Unmodified IBM Plex Mono Regular from Google Fonts, pinned to revision
`2fe1e8c6b137fb13041a7cfca6fdcd8a910ea4dc`. `PROVENANCE.json` records the
download URLs, sizes and SHA-256 hashes; the upstream OFL and METADATA files
sit next to the font.

Selected per text with `font.family: mono;` (see `FontFamily` in
`src/text.rs`). Characters the face lacks fall back to Ubuntu, then the CJK and
emoji faces, with advances normalized to the monospace face's units.
