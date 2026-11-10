# Полужирное начертание

Немодифицированный Ubuntu Bold из Google Fonts, ревизия
`2fe1e8c6b137fb13041a7cfca6fdcd8a910ea4dc`. `PROVENANCE.json` хранит адреса
загрузки, размеры и SHA-256; рядом лежат исходные UFL и METADATA. Лицензия —
Ubuntu Font Licence 1.0 (та же, что у `../Ubuntu-Light.ttf`, см. `../UFL.txt`).

Шрифт не урезан: UFL требует переименования изменённых версий, а полный файл
добавляет 331 612 байт (около 1,7 % к уже встроенным 18,6 МБ шрифтов).

Выбирается свойством `fontWeight: bold;` у `Text` и у отрезков `runs`
(`FontFamily::SansBold` в `src/text.rs`). Для `fontFamily: mono` полужирного
начертания нет: такой текст рисуется обычным IBM Plex Mono. Символы, которых
нет в Ubuntu Bold, берутся из общих запасных шрифтов.
