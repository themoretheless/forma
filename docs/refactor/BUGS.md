# Журнал дефектов

Все записи получены чтением кода. Статус «подозрение» означает, что падающего теста ещё нет.
Каждый дефект чинится отдельным коммитом: сначала тест, затем исправление.

| ID | Место | Статус | Суть |
|---|---|---|---|
| F-1 | `crates/forma/src/binding.rs` `Form::sync` | исправлено | `collect_nodes_mut` клал в карту `&mut` на узел вместе с `&mut` на его потомков через `unsafe`. Заменён безопасным обходом в post-order; при совпадающих id по-прежнему побеждает последний узел в post-order (тест `control_overrides_reach_one_node_per_id_and_the_last_in_post_order_wins`) |
| F-2 | `crates/forma/src/form_document/eval.rs` `parse_hex_color` | исправлено (тесты `tests/eval_colors.rs`) | паника на не-ASCII цвете (`#é1`) подтверждена падающим тестом; также принимался знак (`#+f+f+f`). Теперь оба случая дают `Invalid hex color` |
| F-3 | `crates/forma/src/form_document.rs:513-529, 1071-1085, 1197-1206, 1311-1323` | подозрение | три разных правила раскрытия детей `let`; `let` внутри `empty {}` не виден соседям |
| F-4 | `crates/forma/src/main.rs:917-956` | расхождение подтверждено чтением | медленный touch-drag теряет движение; исправлено только в `native_window.rs` |
| F-5 | `scripts/runtime-digest.mjs:15-19` | подтверждено чтением | `TwoButtons.ui` встраивается в wasm, но не входит в дайджест |
| F-6 | CI и `scripts/full-gate.mjs` | исправлено в гейте | ветки Android и iOS компилируются шагами `mobile-android` и `mobile-ios` (при установленных targets); в CI их по-прежнему нет |
| F-7 | `crates/forma/src/lib.rs:237` и `runtime.rs:1651-1657` | расхождение | разные границы прокрутки у `Button` и `Runtime` |
| F-8 | `crates/forma/src/binding.rs:1639-1658` | не проверено | записи применяются последовательно через `?`; поздняя ошибка оставляет ранние записи |
| S-1 | `src/main.js` `persist()` | исправлено (тест `test/workspace-save.test.js`) | при HTTP-ошибке и сбое сети статус утверждал, что файл записан в проект. Проект к этому моменту уже в хранилище браузера, так что правка не терялась, но и на диск не попадала. Теперь статус говорит «Сохранено в браузере (запись в проект не удалась…)» |
| S-2 | `src/main.js:209-211` | подозрение | `.ui` из разных папок с одним именем сливаются в `ui/<basename>` |
| S-3 | `mcp/smoke.js` | исправлено | сверял число инструментов с устаревшей константой 34 (фактически 45; README писал 44). Теперь сверяет список имён с таблицей `mcp/server.js`; README исправлен. Сам smoke требует запущенной Studio и в гейте не выполняется |
| S-4 | `packages/svet/scripts/check-wasm-provenance.mjs` | подтверждено чтением | хэширует только `lib.rs` |
| S-5 | `vite.config.js` (переименование) | подозрение | разделитель `'/'` и `startsWith('..')` отклоняет допустимые имена |
| S-6 | `src/main.js` `duplicateTreeItem` | подозрение | redo использует старый путь дубликата |
| S-7 | `src/components.js:74` `literal` | не проверено | не экранирует `\r`, `\t`, `${` |
| S-8 | `packages/svet/package.json` `sideEffects` | латентно | не перечислены модули с регистрацией при импорте |
| G-1 | `crates/gpu-runtime/src/metal.rs:2104,2168` | подозрение | `objc_msgSend` с возвратом `CGRect` на x86_64 требует `_stret` |
| G-2 | `crates/gpu-runtime/src/lib.rs:541-566, 656` | подозрение | ошибки валидации и MSL проглатываются, затем `expect` |
| G-3 | `crates/gpu-runtime/src/metal.rs:914-918` | подозрение | кэш пайплайнов по одному 64-битному хэшу без сравнения исходника |
| G-4 | `crates/gpu-runtime/src/metal.rs:744,777` | подозрение | оборачивается возможно nil-буфер |
| G-5 | `crates/gpu-runtime/src/lib.rs:728` | подозрение | `offset + len` без проверки переполнения |
| G-6 | `crates/gpu-runtime/src/lib.rs:169-178`, `vulkan.rs:48-70` | подтверждено чтением | заявлены возможности без реализации; выдуманные адаптеры |
| G-7 | `crates/gpu-shader/src/front/wgsl.rs:848-877, 801` | подозрение | вычисление констант без вычитания, деление не той ассоциативности, неизвестная длина массива = 1 |
| G-8 | `crates/gpu-shader/src/front/wgsl.rs:834-837` | подозрение | неизвестный тип превращается в `u32` |
| G-9 | `crates/gpu-shader/src/front/wgsl.rs:108,114,131` | подтверждено чтением | позиция `ParseError` всегда 1:1 |
| G-10 | `crates/gpu-shader/src/back/msl.rs:1878` | подозрение | `arrayLength` угадывается |

## Зависимости (cargo-deny)

Приняты в `deny.toml` с причиной; пересмотреть при обновлении зависимостей.

| ID | Крейт | Откуда | Суть |
|---|---|---|---|
| RUSTSEC-2026-0194, -0195 | quick-xml 0.39.4 | winit → smithay-client-toolkit → wayland-scanner (proc-macro, Linux) | уязвимости разбора XML; у нас разбирает только протокольные файлы Wayland при сборке |
| RUSTSEC-2026-0192 | ttf-parser 0.25.1 | прямая зависимость `forma` | не поддерживается, обновления нет |
| RUSTSEC-2026-0370, -0371, -0372 | zvariant, zbus_names, zvariant_derive | accesskit_unix → atspi (Linux) | не поддерживаются, обновления нет |
