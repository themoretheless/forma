# Инвентаризация

Статусы: `todo`, `в работе`, `done`, `отложено`. Запахи взяты из разведки чтением кода; построчный проход каждого
метода по чеклисту ещё не выполнен ни для одного модуля, поэтому все строки в статусе `todo`.
Номера строк в описаниях разведки относятся к коду до форматирования; ориентироваться по именам.

| Модуль | Строк | Тестов | Главные запахи | Приоритет | Статус |
|---|---:|---|---|---|---|
| crates/forma/src/form_document.rs | 4858 | 56 | модель + раскрытие двух уровней + раскладка + эмиссия текста в одном файле; функции по 150–320 строк | 1 | todo |
| crates/forma/src/runtime.rs | 4377 | 41 | загрузка, a11y, фокус, растровый кэш, ~120 wasm-методов в одном типе | 1 | todo |
| crates/forma/src/main.rs | 2015 | 4 | второй оконный хост, `window_event` на сотни строк | 1 | todo |
| crates/forma/src/native_window.rs | 3164 | 19 | хост + генерация UI + эмуляция платформ + ввод | 1 | todo |
| crates/forma/src/binding.rs | 1904 | 5 | `Form::sync`/`update` по 220 строк, `unsafe` с пересекающимися `&mut` | 1 | todo |
| crates/forma/src/form_document/eval.rs | — | 0 | диспетчер builtins одной функцией, ошибки строками | 1 | todo |
| crates/forma/src/form_document/{bindings,compiler,layout,contracts,svg}.rs | — | 0 | нет прямых тестов | 1 | todo |
| crates/forma/src/text.rs | 2298 | 18 | юникод, шейпинг, шрифты, раскладка и растеризатор вместе; цикл с `shape` | 2 | todo |
| crates/forma/src/control_state.rs | 2233 | 13 | 12 независимых моделей в одном файле | 2 | todo |
| crates/forma/src/template.rs | — | 14 | лексер, парсер, кэш; публичный элемент после тестового модуля | 2 | todo |
| crates/forma/src/{raster_cache,display_list,lib}.rs | — | 9 / 0 / 17 | `impl Button` размазан по трём файлам, цикл с `lib` | 2 | todo |
| crates/forma/src/{gpu,mcp,reveal,text_input,native_*}.rs | — | — | не проходились | 3 | todo |
| packages/svet/src/engine/forma.ts | 3483 | через barrel | `compileFormaComponent` ~1400 строк, импорт `gpu` | 1 | todo |
| packages/svet/src/engine/gpu.ts | 3208 | через barrel | шесть ответственностей, `createCanvas3D` ~600 строк | 2 | todo |
| packages/svet/src/engine/reactivity.ts | 2671 | через barrel | горячий путь; повторяющиеся блоки в `signal`/`computed` | 2 | todo |
| packages/svet/src/engine/dom.ts | 2574 | через barrel | `For`, `bindModel`, `bindProp` по 150–260 строк | 2 | todo |
| packages/svet/src/engine/{advanced,forma-expr,forma-reactive,…}.ts | — | — | смешанные ответственности в `advanced`; остальные не проходились | 3 | todo |
| packages/svet/crates/svet-wasm/src/lib.rs | 3305 | 66 | граф, арена GPU, математика, FFI и тесты в одном файле | 2 | todo |
| src/main.js | 2574 | текстовые | 37 импортов, ~20 модульных `let`, 54 `.onX=` | 1 | todo |
| src/components.js | — | да | `compile()` одной замыкающей функцией на ~500 строк | 1 | todo |
| vite.config.js, корневые runners | — | да | dev-бэкенд внутри конфига | 2 | todo |
| src/mcp-commands.js + mcp/server.js | — | текстовые | таблица инструментов в двух местах | 2 | todo |
| остальные 65 файлов src/ | — | частично | не проходились | 3 | todo |
| crates/gpu-shader | 4416 | 0 | нет тестов; транслятор переписыванием строк | 1 | todo |
| crates/gpu-runtime | 9295 | 18 | 111 `transmute`, ноль `// SAFETY:`, crate-wide `allow`, заглушки бэкендов | 1 | todo |
| crates/gpu-compute | 3072 | 25 | мелкие | 3 | todo |
| crates/raster-core | 3936 | 31 | `RasterPipelines::new` 426 строк; тесты пишут в `output/` | 3 | todo |
| crates/math-core | 5916 | 92 | `spatial.rs` на 12 семейств типов, 21 `allow` | 3 | todo |
| crates/math-compute | 5963 | 41 | параллельные реализации gpu/cuda (лидер по дублям) | 3 | todo |
| crates/tensor-core | 9850 | 53 | один `impl Tensor` на 1000 строк | 3 | todo |
| crates/compute-core | 11 322 | 164 | двойной разбор WGSL; дубли в бенч-примерах | 3 | todo |
| crates/compute-mlx | 6859 | 148 | 84 `unsafe` без SAFETY; `backend()` скопирован в 22 тестах | 3 | todo |
| crates/compute-cuda | 16 309 | 180 (хост) | не проверяется на устройстве | 3 | отложено до появления CUDA-хоста для логики |
