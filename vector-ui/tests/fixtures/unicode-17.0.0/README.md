# Unicode17 grapheme boundary reference

`GraphemeBreakTest.txt` is the complete, unmodified Unicode17.0.0 corpus:
https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt

Its766 test vectors supply independent expected byte boundaries for TextEdit
navigation, selection, deletion, replacement and history tests. The corpus
version matches the pinned unicode-segmentation1.13.3 Unicode version.

Unicode copyright notices remain in the source. `LICENSE.txt` was obtained from
https://www.unicode.org/license.txt and accompanies the vendored data.
Tests require no network access. Passing this finite corpus does not qualify
font shaping, BiDi, rendering, IME or every possible Unicode string.

Corpus SHA256: `e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec`


## Word boundary reference

`WordBreakTest.txt` is the complete unmodified Unicode17.0.0 corpus (1944 vectors):
https://www.unicode.org/Public/17.0.0/ucd/auxiliary/WordBreakTest.txt
SHA256: `1de23a75f37904abc7d206239ee8d34f8fdf0fb4ab32a7174dfbabbde25419b2`. The same Unicode license applies.

`word_conformance.rs` checks raw segmentation against independent reference
markers, then tests editor navigation/deletion/history at every scalar position.
UI policy skips whitespace-only segments and rounds destinations to graphemes;
that rounding uses unicode-segmentation separately qualified by GraphemeBreakTest.
No dictionary segmentation, locale-native parity, shaping or IME claim is made.
