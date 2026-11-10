# Инвентаризация

Статусы: `todo`, `в работе`, `done`, `отложено`. Запахи взяты из разведки чтением кода; построчный проход каждого
метода по чеклисту ещё не выполнен ни для одного модуля, поэтому все строки в статусе `todo`.
Номера строк в описаниях разведки относятся к коду до форматирования; ориентироваться по именам.

| Модуль | Строк | Тестов | Главные запахи | Приоритет | Статус |
|---|---:|---|---|---|---|
| crates/forma/src/form_document.rs + visual/scene/render/tests | 548 + 654 + 634 + 465 + 2585 | 56 | разбит по уровням рендера, тесты вынесены; осталось: общие селекторы control-flow для двух уровней, `scene_expand` (~380 строк), `render_scene_with_extents` (~390), `NodeKind` вместо строк | 1 | в работе |
| crates/forma/src/runtime.rs + wasm_api/tests/raster_tests | 1635 + 1102 + 1369 + 302 | 41 | wasm-фасад и тесты вынесены; осталось: загрузка, a11y, фокус, растровый кэш в одном `impl` | 1 | в работе |
| crates/forma/src/main.rs | 2015 | 4 | второй оконный хост, `window_event` на сотни строк | 1 | todo |
| crates/forma/src/native_window.rs | 3164 | 19 | хост + генерация UI + эмуляция платформ + ввод | 1 | todo |
| crates/forma/src/binding.rs | 1950 | 6 | `unsafe` убран (F-1); осталось: `Form::sync`/`update` по 220 строк, разбиение на модули | 1 | в работе |
| crates/forma/src/form_document/eval.rs + eval/builtins/* | 505 + 880 | golden на все builtins, 2 теста цвета | диспетчер builtins разбит на 9 семейств; осталось: `eval_mode` (~280 строк), ошибки строками, тесты путей и операторов | 1 | в работе |
| crates/forma/src/form_document/{bindings,compiler,layout,contracts,svg}.rs | — | 0 | нет прямых тестов | 1 | todo |
| crates/forma/src/text.rs | 2298 | 18 | юникод, шейпинг, шрифты, раскладка и растеризатор вместе; цикл с `shape` | 2 | todo |
| crates/forma/src/control_state/* | 2291 в 15 файлах | 13 | разбит по моделям | 2 | done (перемещение); логика не проходилась |
| crates/forma/src/template.rs | — | 14 | лексер, парсер, кэш; публичный элемент после тестового модуля | 2 | todo |
| crates/forma/src/{raster_cache,display_list,lib}.rs | — | 9 / 0 / 17 | `impl Button` размазан по трём файлам, цикл с `lib` | 2 | todo |
| crates/forma/src/{gpu,mcp,reveal,text_input,native_*}.rs | — | — | не проходились | 3 | todo |
| packages/svet/src/engine/forma.ts | 3483 | через barrel | `compileFormaComponent` ~1400 строк, импорт `gpu` | 1 | todo |
| packages/svet/src/engine/gpu.ts | 3208 | через barrel | шесть ответственностей, `createCanvas3D` ~600 строк | 2 | todo |
| packages/svet/src/engine/reactivity.ts | 2671 | через barrel | горячий путь; повторяющиеся блоки в `signal`/`computed` | 2 | todo |
| packages/svet/src/engine/dom.ts | 2574 | через barrel | `For`, `bindModel`, `bindProp` по 150–260 строк | 2 | todo |
| packages/svet/src/engine/{advanced,forma-expr,forma-reactive,…}.ts | — | — | смешанные ответственности в `advanced`; остальные не проходились | 3 | todo |
| packages/svet/crates/svet-wasm/src/lib.rs | 3305 | 66 | граф, арена GPU, математика, FFI и тесты в одном файле | 2 | todo |
| src/main.js | 2574 | текстовые | 37 импортов, ~20 модульных `let`, 54 `.onX=` | 1 | todo; интеграция OpenSCAD и файловый API удалены, остались недостижимые обработчики строк-папок дерева (`toggleFolder`, `data-folder`, `expandedDirs`) — убрать при разбиении `main.js` |
| src/components.js | — | да | `compile()` одной замыкающей функцией на ~500 строк | 1 | todo |
| vite.config.js, корневые runners | — | да | dev-бэкенд внутри конфига | 2 | todo |
| src/mcp-commands.js + mcp/server.js | — | текстовые | таблица инструментов в двух местах | 2 | todo |
| остальные 65 файлов src/ | — | частично | не проходились | 3 | todo |
| crates/gpu-shader | 4416 | golden по 123 шейдерам | транслятор переписыванием строк; осталось: разбиение `msl.rs`/`wgsl.rs`, позиция ошибок, проверка компиляции MSL на Metal | 1 | в работе |
| crates/gpu-runtime | 9295 | 18 | 111 `transmute`, ноль `// SAFETY:`, crate-wide `allow`, заглушки бэкендов | 1 | todo |
| crates/gpu-compute | 3072 | 25 | мелкие | 3 | todo |
| crates/raster-core | 3936 | 31 | `RasterPipelines::new` 426 строк; тесты пишут в `output/` | 3 | todo |
| crates/math-core | 5916 | 92 | `spatial.rs` на 12 семейств типов, 21 `allow` | 3 | todo |
| crates/math-compute | 5963 | 41 | параллельные реализации gpu/cuda (лидер по дублям) | 3 | todo |
| crates/tensor-core | 9850 | 53 | один `impl Tensor` на 1000 строк | 3 | todo |
| crates/compute-core | 11 322 | 164 | двойной разбор WGSL; дубли в бенч-примерах | 3 | todo |
| crates/compute-mlx | 6859 | 148 | 84 `unsafe` без SAFETY; `backend()` скопирован в 22 тестах | 3 | todo |
| crates/compute-cuda | 16 309 | 180 (хост) | не проверяется на устройстве | 3 | отложено до появления CUDA-хоста для логики |
