# Единый план развития Forma

Единственный файл планов по всему репозиторию (`Forma`, `Studio`, `Svet`, `GPU` и вычислительные
крейты) на 2026-11-20 (`4ddf618a`). Здесь собрана только незавершённая работа и действующие
правила выполнения; история выполненных этапов, базовые замеры, дефекты и изменения контрактов
хранятся в [ARCHITECTURE.md](refactor/ARCHITECTURE.md), [BASELINE.md](refactor/BASELINE.md),
[BUGS.md](refactor/BUGS.md), [INVENTORY.md](refactor/INVENTORY.md),
[MIGRATIONS.md](refactor/MIGRATIONS.md), [SNAPSHOTS.md](refactor/SNAPSHOTS.md),
[PRODUCTION_READINESS.md](../PRODUCTION_READINESS.md) и
[packages/svet/DEVELOPMENT.md](../packages/svet/DEVELOPMENT.md).

## 1. Runtime Forma, ввод и рендер

- [ ] Стоимость больших форм и длительных сессий (`load < 1` остаётся открытым): снижение стоимости
  замены `Runtime` при редактировании на `8/32/96/256` контролах (на срезе Wave 7B `2026-11-21`
  `55,53 мкс / 789,4 КБ` против `1 166,18 мкс / 16,65 МБ`, ~21× быстрее через `reconcile_scene_ir`)
  и квалификация плато RSS, GPU-памяти и задержки ввод → экран (`window_bench`: `2 398` present,
  `p50=8,31 мс`, `RSS=99,83→99,89 МБ`, `geometry_uploads=0`) сняты в составе всех 9 последовательных
  бенчмарков ([BASELINE.md](refactor/BASELINE.md), [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md));
  поскольку внешняя нагрузка хоста во время замеров составляла `vm.loadavg` `5.96–17.68`,
  итоговое переснятие времён на спокойной машине (`load < 1.0`) остаётся открытым по точной
  команде в [BASELINE.md](refactor/BASELINE.md).
- [x] Пайплайн удерживаемого рендера (строго по порядку):
  1. Постоянное дерево UI с раздельными флагами изменения `layout`, `paint` и `composition` (смена
     цвета не перестраивает раскладку и узлы; `DirtyFlags`, `reconcile_scene_ir`).
  2. Независимый от backend список рисования с `bounds`, `transform`, `clip-id` и `resource-id`,
     единый для CPU, GPU и диагностики Studio (`DisplayList::items`, `rasterize_rgba`,
     `display_list_diagnostics_json`).
  3. Кэши по элементам/тайлам, dirty rectangles и ограничение слоёв вложенного `clip` границами
     узла вместо выделения на весь viewport (`F-15`) и полной инвалидации при прокрутке (`F-16`).
  4. Общий glyph atlas и кэш SVG под глобальным байтовым бюджетом (`memory-budget`) с
     LRU-вытеснением (`resource_cache.rs`).
  5. Расширение GPU-бэкенда через `wgpu` (`draw_damaged`, `render_rgba`, `GpuWindow`) при
     сохранении CPU-пути как эталона и автоматического fallback (`render_rgba_with_cpu_fallback`).

## 2. GPU-стек (`gpu-shader`, `gpu-runtime`, `gpu-compute`)

- [x] `gpu-shader`: настоящая проверка типов в `gpu_shader::valid::Validator`, чтобы ill-typed
  WGSL не доходил до Metal (`ShaderModule::invalid_reason` + тест zero-Metal-compile); завершена
  декомпозиция крупных функций в `front/wgsl` и `back/msl` (29 тестов `gpu-shader`, золотые
  эталоны пересобраны с отсевом неразвёрнутых partials).
- [x] `gpu-runtime` и `gpu-compute`: расставлены `// SAFETY:` помодульно со снятием crate-wide
  `allow(unsafe_op_in_unsafe_fn)` (`#![deny(unsafe_op_in_unsafe_fn)]`), отделена не-Apple
  Metal-заглушка (`metal/stub.rs`) для снятия условного `allow(dead_code, unused_variables)` (0
  предупреждений на Linux/Windows/wasm32), разбиты длинные функции кодирования команд/render pass
  и `lib.rs`, добавлен `Error::source()` в `gpu-runtime` и `gpu-compute`, квалифицированы реальные
  `NSError` сбои GPU (`MTLCommandBufferErrorDomain`), `Device::destroy()` / выбор адаптера /
  `MTLCopyAllDevicesWithObserver` idle-уведомления и signal-driven завершение (`2000` отправок:
  `p50=136 µs, p95=245 µs, p99=380 µs`, плато RSS `14 752 → 14 752 КБ`, `blocking_wait_calls=0`,
  подробности — в [BASELINE.md](refactor/BASELINE.md)).
- [x] Дополнительные GPU-бэкенды ([ADR 0002](adr/0002-spirv-vulkan-backend.md)–[0005](adr/0005-cuda-and-rtx-host-gated-policy.md), CI-матрица в `.github/workflows/ci.yml` и интеграционный набор `crates/gpu-runtime/tests/portable_backends.rs`):
  1. SPIR-V + Vulkan (`gpu-shader::back::spv`, `gpu-runtime::vulkan` / `portable`, снятие `wgpu-linux` в `forma`; Linux CI на `lavapipe`).
  2. HLSL + DX12 (`gpu-shader::back::hlsl`, `gpu-runtime::dx12`; Windows CI раннер `windows-2022`).
  3. GLSL + OpenGL и WebGPU (`gpu-shader::back::glsl`, `gpu-runtime::{opengl,webgpu}`, async `wasm32`, downlevel GLES-лимиты).
  4. CUDA и RTX остаются хост-изолированными за динамической проверкой драйвера NVIDIA (`gpu_runtime::cuda::probe_driver_available`, `gpu-compute::cuda`, `compute-cuda`); выполнение на аппаратуре CUDA без NVIDIA-хоста не запускается и поддельные адаптеры не создаются.

## 3. Svet (`packages/svet`)

- [x] Браузерный ввод и кроссбраузерная матрица (`packages/svet/BROWSER_MATRIX.md`): проверить
  доверенный клавиатурный ввод по словам, CJK IME-композицию, AltGr и не-Apple Ctrl-раскладки в
  `src/vector-preview.js` на Chromium, WebKit и Firefox.
- [x] Интеграции рантайма и SSR: soak-тест GC across-copy и нескольких realms, захват асинхронного
  контекста после `await`, потоковые SSR/Edge-носители под медленными клиентами и отменой,
  burst-нагрузка роутера и разбор причины исторического отказа HTTP external memory.
- [x] Декомпозиция движка и внутренние слои: разложена сборка экземпляра и HMR из `instantiateForma`
  (`forma.ts` -> `forma-hmr.ts`, `forma-view.ts`, `forma-document.ts`), `createCanvas3D`
  (`gpu-scene3d.ts` -> `gpu-scene3d-{controls,webgpu,canvas2d}.ts`) и состояние/асинхронная
  логика `gpu.ts` (`gpu-state.ts`, `gpu-compute-async.ts`); строгие слои
  `wasm` -> `reactivity` -> `dom` -> `gpu` -> `forma` -> `facade` без циклов закреплены в
  `packages/svet/tests/engine-layering.test.ts`.
- [x] Эффективность компилятора и реактивного состояния: чистые статические вызовы (`Math.*` без
  `random`, `Number.*`, `String`/`Number`/`Boolean`/`BigInt`, статический `JSON`, чистые методы
  литералов с учётом лексического затенения и `const`) больше не оборачиваются в реактивные thunk
  в `compiler.ts`; постоянный JS-массив `deps` убран из `EffectEntry`/`ComputedEntry` в
  `reactivity.ts` за счёт `svet_get_sources_ptr`/`svet_get_sources_len` в `svet-wasm` и пула
  временных `stagedDeps` только на активных кадрах стека (`tests/reactivity-edge-dedup.test.ts`).
- [x] Воспроизводимое происхождение `svet-wasm` (`S-9`): в `packages/svet/scripts/build-wasm.mjs`
  добавлен `--remap-path-prefix` для `repositoryRoot`, `targetRoot` (`CARGO_TARGET_DIR`) и sysroot;
  побайтовая идентичность при сборке из двух разных checkout-каталогов и двух разных
  `CARGO_TARGET_DIR` закреплена автотестом `packages/svet/tests/wasm-build-paths.test.ts`.
- [x] Продуктовые бенчмарки реальных приложений: startup, p95 задержка взаимодействия, frame time и
  память.

## 4. Studio и dev-сервер

- [x] Декомпозировать `src/main.js` (~1932 строки) до чистой композиции над `app/state.js`,
  сервисами с инъекцией портов и реестром команд (удалив скрытый дублирующий шаблон старого
  заголовка); разложить `src/` по фичам (`workspace`, `editor`, `preview`, `canvas`, `inspector`,
  `tree`, `output`, `shell`) с проверкой в `test/module-graph.test.js` (`src/main.js` 229 строк).
- [x] Послойно включить `// @ts-check` + JSDoc для JS в Studio (добавлены `jsconfig.json`,
  `src/types.d.ts`, `scripts/typecheck.mjs`, шаг `typecheck-js` в `full-gate.mjs` и CI; `119`
  модулей с `0` ошибок без ломки zero-build dev-режима).
- [x] Характеризация и безопасность раннеров: покрыты граничные случаи `buildNativeRustExport`
  (`S-19`), закреплены оставшиеся 28 веток `throw` компилятора (`test/compile-errors.test.js`),
  а обходимый regex-blacklist в `design-data-runner.js` заменён на однопроходный лексический сканер
  Rust, ОС-песочницу (`sandbox-exec` / `bwrap` + очищенный `env`) и подтверждение доверия сессии
  (`sessionTrust: true` / `revokeTrust()`, `S-20`).

## 5. Вычислительные крейты и финальная чистка

- [x] Документировать FFI `// SAFETY:` в `compute-mlx` и соседних крейтах; заменить оставшиеся
  достижимые библиотечные `expect` / `unwrap` типизированными ошибками.
- [x] Внутренний долг крейтов ([INVENTORY.md](refactor/INVENTORY.md)): табличный
  `RasterPipelines::new` и вывод тестов во временные каталоги в `raster-core`, устранение дублей
  `gpu`/`cuda` в `math-compute`, разбиение `spatial.rs` и сокращение `allow` в `math-core`,
  разбиение `impl Tensor` в `tensor-core` и устранение двойного разбора WGSL в `compute-core`.
- [x] Цепочка поставок и закрытие базовой линии: `ttf-parser` заменён на `skrifa 0.44.0`
  (`crates/forma/src/text/face.rs`), `wayland-scanner` обновлён до `0.31.11` (`quick-xml 0.41.0`),
  сужены фичи `winit`, сняты `RUSTSEC-2026-0194`, `-0195`, `-0192` из `deny.toml` (остаются лишь
  информационные `zbus` `RUSTSEC-2026-0370..-0372` на Linux AT-SPI); исправлены 4 бага границ
  `control_state` (`F-17`); декомпозировано TS-ядро Svet и снято >100 подавлений ESLint (`S-21`);
  снято полное покрытие `llvm-cov` всех 19 крейтов (`82,62 %`), V8-покрытие Svet (`82,02 %`),
  Studio JS (`89,62 %`) и все 9 последовательных бенчмарков в [BASELINE.md](refactor/BASELINE.md);
  все 35 строк [INVENTORY.md](refactor/INVENTORY.md) закрыты в `done` (`0` `todo`/`в работе`). Повтор
  абсолютных времён на спокойном хосте `load < 1` зафиксирован в разделе 1 и `BASELINE.md`.

## 6. Выпуск и очередь выполнения

- [x] Release candidate на одной чистой ревизии (`0.3.8`): чистая автономная установка, цепочка
  Rust → WASM → пакеты Studio (`package:release`) и `svet-engine` (`test:package`), provenance,
  exports/types (`119` `@ts-check` модулей в CI), browser/native/HTTP-гейты, `npm run gate`
  (`30/30`) и кроссплатформенный CI на Linux, macOS и Windows.
- [x] Опубликована матрица `implemented / validated / partial / unsupported` с точными командами
  воспроизведения: [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md).

### Очередь выполнения

1. Разделы 2 (GPU-стек), 3 (`Svet`), 4 (`Studio и dev-сервер`), 5 (вычислительные крейты, цепочка поставок и базовая линия) и 6 (release candidate `0.3.8` и матрица возможностей) завершены.
2. Открытым пунктом вне функциональности остаётся только повторное снятие абсолютных времён 9 бенчмарков в idle-сессии хоста с `vm.loadavg < 1.0` (раздел 1, команда в [BASELINE.md](refactor/BASELINE.md)).

### Действующие инварианты

- Контракты генераторов, `Runtime`, WASM-экспортов, JNI-символов и имён MCP-инструментов меняются
  только с записью в [MIGRATIONS.md](refactor/MIGRATIONS.md).
- Найденный баг фиксируется в [BUGS.md](refactor/BUGS.md) и закрывается вместе с падающим тестом.
- Счётчики предупреждений (`clippy-ratchet`, `eslint`) могут только убывать; `allow` не
  добавляются.
- После изменений в `crates/forma/src`, `Cargo.toml` или `Cargo.lock` пересобирать WASM
  (`npm run build:wasm`) перед финальным гейтом.
- В коммитах по `compute-cuda` без проверки на устройстве явно указывать, что на CUDA-железе
  запуск не выполнялся; сообщения коммитов — обычное предложение без префиксов типа.
