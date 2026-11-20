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
  замены `Runtime` при редактировании на `8/32/96/256` контролах (выполнено в ~20× через
  `reconcile_scene_ir`) и квалификация плато RSS, GPU-памяти и задержки ввод → экран в долгом
  OS-окне на 256 контролах сняты ([BASELINE.md](refactor/BASELINE.md), `2026-11-20`, load хоста
  ~8–9,5); осталось повторное снятие времён на спокойном хосте (`load < 1`).
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
- [ ] Дополнительные GPU-бэкенды (по проверяемости в CI, каждый с ADR и тестовой матрицей):
  1. SPIR-V + Vulkan (Linux CI на lavapipe).
  2. HLSL + DX12 (Windows CI раннер).
  3. GLSL + OpenGL, затем WebGPU (`wasm`).
  4. CUDA и RTX (только при наличии NVIDIA-хоста).

## 3. Svet (`packages/svet`)

- [ ] Браузерный ввод и кроссбраузерная матрица (`packages/svet/BROWSER_MATRIX.md`): проверить
  доверенный клавиатурный ввод по словам, CJK IME-композицию, AltGr и не-Apple Ctrl-раскладки в
  `src/vector-preview.js` на Chromium, WebKit и Firefox.
- [ ] Интеграции рантайма и SSR: soak-тест GC across-copy и нескольких realms, захват асинхронного
  контекста после `await`, потоковые SSR/Edge-носители под медленными клиентами и отменой,
  burst-нагрузка роутера и разбор причины исторического отказа HTTP external memory.
- [ ] Декомпозиция движка и внутренние слои: разбить сборку экземпляра с HMR в `instantiateForma`
  (`forma.ts`, ~550 строк), `createCanvas3D` (`gpu-scene3d.ts`, ~880 строк) и остаток
  асинхронной логики в `gpu.ts`; закрепить слои `reactivity` → `dom` → `gpu` → `forma`.
- [ ] Эффективность компилятора и реактивного состояния: не оборачивать чистые статические вызовы
  в реактивные thunk в `compiler.ts` (`hasEagerCall`) и убрать дублирование графовых рёбер между JS
  (`reactivity.ts`) и линейной памятью `svet-wasm`.
- [ ] Воспроизводимое происхождение `svet-wasm` (`S-9`): убрать зависимость байтов `svet-wasm` от
  каталога checkout (включить `svet-wasm` в корневой workspace либо проверять экспорты и дайджест
  исходников вместо чувствительных к пути байтов).
- [ ] Продуктовые бенчмарки реальных приложений: startup, p95 задержка взаимодействия, frame time и
  память.

## 4. Studio и dev-сервер

- [ ] Декомпозировать `src/main.js` (~1932 строки) до чистой композиции над `app/state.js`,
  сервисами с инъекцией портов и реестром команд (удалив скрытый дублирующий шаблон старого
  заголовка); разложить `src/` по фичам (`workspace`, `editor`, `preview`, `canvas`, `inspector`,
  `tree`, `output`, `shell`) с проверкой в `test/module-graph.test.js`.
- [ ] Послойно включить `// @ts-check` + JSDoc для JS в Studio (добавить проверку типов без ломки
  zero-build dev-режима).
- [ ] Характеризация и безопасность раннеров: покрыть граничные случаи `buildNativeRustExport`,
  закрепить оставшиеся тексты `throw` компилятора и заменить обходимый regex-blacklist в
  `design-data-runner.js` на явное доверие проекту на уровне сессии / ОС-песочницу (`sandbox-exec` /
  `bwrap`) или AST-валидацию.

## 5. Вычислительные крейты и финальная чистка

- [ ] Документировать FFI `// SAFETY:` в `compute-mlx` и соседних крейтах; заменить оставшиеся
  достижимые библиотечные `expect` / `unwrap` типизированными ошибками.
- [ ] Внутренний долг крейтов ([INVENTORY.md](refactor/INVENTORY.md)): табличный
  `RasterPipelines::new` и вывод тестов во временные каталоги в `raster-core`, устранение дублей
  `gpu`/`cuda` в `math-compute`, разбиение `spatial.rs` и сокращение `allow` в `math-core`,
  разбиение `impl Tensor` в `tensor-core` и устранение двойного разбора WGSL в `compute-core`.
- [ ] Цепочка поставок и закрытие базовой линии: отслеживать/заменить неподдерживаемые
  транзитивные зависимости (`quick-xml`, `ttf-parser`, `zvariant`), снять покрытие Rust/Svet и
  замеры на спокойном хосте в [BASELINE.md](refactor/BASELINE.md), а также закрыть оставшиеся
  строки `todo` и `в работе` в [INVENTORY.md](refactor/INVENTORY.md).

## 6. Выпуск и очередь выполнения

- [ ] Release candidate на одной чистой ревизии: чистая автономная установка, цепочка Rust → WASM
  → пакеты Studio и `svet-engine`, provenance, exports и types, browser/native/HTTP-гейты и зелёный
  удалённый CI на Linux, macOS и Windows.
- [ ] Опубликовать матрицу `implemented / validated / partial / unsupported` с точными командами
  воспроизведения.

### Очередь выполнения

1. Раздел 1 завершён по функциональности удерживаемого рендера и soak-квалификации (осталось
   переснятие на спокойном хосте `load < 1`, объединённое со сводным закрытием базовой линии в
   разделе 5).
2. Текущий фокус — **раздел 2: GPU-стек (`gpu-shader`, `gpu-runtime`, `gpu-compute`)** и бэкенды
   по проверяемости в CI; далее разделы 3–6 (`Svet`, `Studio`, вычислительные крейты и release
   candidate + матрица возможностей).

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
