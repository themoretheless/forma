# Изменения контрактов

Каждая запись: что изменилось, кого затрагивает, как мигрировать, коммит.

| № | Изменение | Затронуто | Миграция | Коммит |
|---|---|---|---|---|
| 1 | `vector-ui` → `crates/forma` | пути, URL галереи, путь по умолчанию в экспорте | см. раздел 1 | Rename vector-ui to crates/forma |
| 2 | Корневой Cargo workspace | lock-файлы, каталог сборки, профиль release | см. раздел 2 | Put the Rust crates under one Cargo workspace |

Запланировано (см. `PLAN.md`): `vector-ui` → `crates/forma`; общий `Cargo.lock` и `target/`; удаление wasm-класса `Button`;
`Node.kind: NodeKind`; поведение dev-бинарника после слияния хостов; сужение публичного Rust API;
компилятор только из `svet-engine/compiler`; минус 10 wasm-экспортов `svet`; пути модулей Studio и runners;
фасад `createIdeCommands`; пакеты `math-core`/`math-compute`; профиль `svet-wasm`.

## 1. Каталог `vector-ui` переименован в `crates/forma`

- Затронуто: все пути `vector-ui/...` в скриптах, CI, `package.json`, `.mcp.json`, тестах и документации; URL галереи контролов теперь `crates/forma/examples/controls.html`; путь к крейту по умолчанию в native-экспорте (`src/native-export.js`, `src/sample-project.js`) теперь `../crates/forma`.
- Миграция: во внешних проектах заменить `path = ".../vector-ui"` на `path = ".../crates/forma"`; каталог сборки теперь `crates/forma/target`.
- Имя пакета, библиотеки и бинарника (`forma`) не изменилось.

## 2. Корневой Cargo workspace

- Затронуто: `Cargo.lock` теперь один, в корне; каталог сборки `target/` в корне вместо `crates/forma/target`;
  lock-файлы примеров и `gpu-runtime` удалены; профиль `release` объявлен в корневом `Cargo.toml`.
- Миграция: скрипты и CI, читавшие `crates/forma/Cargo.lock` или `crates/forma/target/...`, должны читать
  `Cargo.lock` и `target/...`. Команды `cargo ... --manifest-path crates/forma/Cargo.toml` продолжают работать.
- Подробности и исключения: `docs/adr/0001-cargo-workspace.md`.

## 3. Один релизный тег `vX.Y.Z`

- Затронуто: теги `forma-vX.Y.Z` и `studio-vX.Y.Z` больше не запускают публикацию и не двигают ветку `release`;
  `scripts/versions.mjs check <tag>` принимает только `vX.Y.Z` и требует, чтобы версия совпадала в обоих manifest.
- Миграция: поднимать версию командой `npm run version:set -- all X.Y.Z`, выпускать тегом `vX.Y.Z`.
  GitHub Release один, с двумя архивами. Старые теги остаются в истории.

## 4. OpenSCAD убран из Forma и Studio

- `openscad-ui/` удалён: это была побайтная копия `open-scad-viewer/forma-studio`, оригинал живёт там.
- Studio больше не знает про open-scad-viewer: проекта OpenSCAD нет в списке, по умолчанию (без `?project=`)
  открывается `knowledge-workspace`, у `?project=studio-controls` прежнее поведение.
- Файловый API dev-сервера `/__forma_workspace` (чтение и запись соседней папки `../open-scad-viewer`) удалён
  вместе с деревом папок, которое его использовало. Работа с локальной папкой через «Папка»
  (File System Access API) осталась.
- `npm run studio:native` без аргумента открывает `studio-controls` вместо OpenSCAD.
- Затронуты локальные данные: проект OpenSCAD, сохранённый в браузере под ключами `…:openscad-studio-v5`,
  больше не открывается; данные в хранилище браузера остаются нетронутыми.

## 5. svet в общей релизной версии

- `packages/svet/package.json` (`svet-engine`) и крейт `svet-wasm` больше не держат постоянные номера (1.0.0 и
  0.1.0): они идут в общей версии релиза, `npm run version:set -- all X.Y.Z` поднимает и их, а тег `vX.Y.Z`
  требует совпадения. Выравнивание прошло на 0.3.4, то есть номер `svet-engine` уменьшился с 1.0.0; пакет
  в npm не публиковался, поэтому потребителей сравнения версий нет.

## 6. `gpu-kit`: состояние растеризации одним аргументом

- `MaterialRenderer::prepare` принимает `MaterialRaster { alpha, double_sided, clockwise }` вместо трёх
  отдельных аргументов `alpha`, `double_sided`, `clockwise`. Порядок остальных аргументов прежний.
- Миграция: `prepare(budget, vertices, p, textures, alpha, double_sided, clockwise, cancelled)` →
  `prepare(budget, vertices, p, textures, MaterialRaster { alpha, double_sided, clockwise }, cancelled)`.
  Внутри репозитория вызовов вне тестов `gpu-kit` нет; внешние потребители (код из rrrah) правятся так же.

## 7. Полужирное начертание и отрезки текста

- `text::FontFamily` получил вариант `SansBold`, `template::Template` — поле `links`
  (`Vec<TextLink>`), `text::font_face(4)` теперь возвращает Ubuntu Bold, а не `None`.
- Затронуто: внешний код с исчерпывающим `match` по `FontFamily` и с литералами `Template { .. }`.
- Миграция: добавить ветку `FontFamily::SansBold` (полужирный Ubuntu) и `links: Vec::new()`.
  Встроенный `Text` сцены теперь принимает `fontFamily`, `fontWeight` и `runs`; прежние сцены
  компилируются в те же байты.

## 8. Декомпозиция Rust-ядра Svet на крейты `svet-graph`, `svet-reconcile`, `svet-gpu-arena`

- Затронуто: монолитный `ReactiveGraph` разделён на `svet_graph::ReactiveGraph` (только реактивный DAG),
  `svet_reconcile::LisScratch` (LIS-диффинг) и `svet_gpu_arena::GpuArena` (UMA/DUMA арена, матричная
  математика, clipping и 3D picking). `crates/forma` теперь зависит от `crates/svet-graph` внутри
  корневого Cargo workspace вместо `packages/svet/crates/svet-wasm`; возвращаемый тип телеметрии
  хоста — `svet_graph::context::Telemetry`.
- Миграция: нативным Rust-потребителям подключать `svet-graph` (`svet_graph::context::{Context, Kind, Node, Telemetry}`)
  вместо `svet-wasm`; C-ABI экспорты `svet_*` и реэкспорт `svet_wasm::native::*` сохранены без изменений.

## 9. `gpu-runtime`: убран слой ray tracing, который ничего не строил

- Затронуто: модуль `gpu_runtime::rtx` (`Blas`, `Tlas`, `TlasInstance`, `BlasGeometryDescriptor`,
  `BlasGeometryKind`, `RtxCapabilities`, `RtxTier`) и методы `Device::create_blas`,
  `Device::create_tlas`, `Device::rtx_capabilities` удалены. Они возвращали оценки размеров,
  выдуманный `device_address` и таблицу «возможностей», одинаково обещавшую аппаратный RTX на
  каждом backend; ни одна структура ускорения не создавалась.
- `Adapter::features()` на Metal больше не содержит `EXPERIMENTAL_RAY_QUERY`,
  `EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE` и `RAY_TRACING_PIPELINE`. Буфер с
  `BLAS_INPUT` / `TLAS_INPUT` теперь всегда отклоняется как validation error.
- Миграция: внешнему коду убрать обращения к `wgpu::rtx::*` и не запрашивать эти features на
  прямом Metal-пути. Константы `Features` и `BufferUsages` сохранены ради общей с `wgpu`
  поверхности API.

## 10. Пакеты `osv-math` и `osv-math-compute` переименованы

- Затронуто: имена Cargo-пакетов `osv-math` → `math-core`, `osv-math-compute` → `math-compute`.
  Имена библиотек (`math_core`, `math_compute`) и каталоги не менялись, поэтому `use` в коде
  остаётся прежним.
- Миграция: в зависимостях убрать `package = "osv-math"` / `package = "osv-math-compute"`
  (`math-core = { path = ".." }`), в командах заменить `-p osv-math` на `-p math-core` и
  `-p osv-math-compute` на `-p math-compute`. Записи замеров в `benchmarks/` сохраняют команды
  с прежними именами: это протокол того запуска.

## 11. `form_document::Node.kind` стал `NodeKind`

- Затронуто: поле `Node.kind` имело тип `String`, теперь `form_document::NodeKind`. Встроенные
  имена (`Frame`, `Text`, `If` и остальные) хранятся без аллокации, имя компонента как общий
  `Arc<str>`, поэтому копирование вида узла при раскрытии и рендере сцены не выделяет память.
- Совместимо без правок: `kind: "Frame".into()` (так пишет генератор форм), сравнение
  `node.kind == "Frame"`, передача `&node.kind` туда, где ждут `&str`, `node.kind.as_str()`,
  `format!("{}", node.kind)`, JSON-представление (строка).
- Миграция: там, где нужен именно `String`, писать `node.kind.to_string()`; ключ для
  `BTreeMap<String, _>` брать как `node.kind.as_str()`; присваивание `String` заменить на
  `value.into()`.
- Замер `binding_sync_bench` (5 чередующихся запусков, минимум): аллокаций на рендер сцены
  меньше на 2,4–2,6 % (2058 → 2004 при 8 контролах, 24478 → 23896 при 96), на синхронизацию
  привязок на 1,1–1,3 %; время в пределах шума (−0,5…−1 %).

## 12. `gpu-shader`: убраны бэкенды, которые ничего не транслировали

- Затронуто: модули `gpu_shader::back::{spv, hlsl, glsl, cuda}`, тип
  `MultiTargetShaderArtifact` и функция `compile_shader_multi_target` удалены (вместе с
  реэкспортом из `gpu-compute`). Они возвращали успешный результат с текстом, который только
  выглядел как SPIR-V, HLSL, GLSL или PTX.
- `compile_wgsl_for_target` для `VulkanSpirv`, `Dx12Hlsl`, `CudaPtx` и `OpenGlGlsl` теперь
  возвращает ошибку; `ComputeShaderTarget::is_implemented` отвечает, есть ли бэкенд.
- Миграция: вместо `compile_shader_multi_target(src)?.msl` вызывать
  `compile_wgsl_for_target(src, ComputeShaderTarget::MetalMsl)`. Остальные цели появятся вместе с
  реализацией бэкендов (раздел 4 плана развития).

## 13. Примеры шейдеров переехали из `gpu-shader` в `gpu-compute`

- Затронуто: `generate_wgsl_tiled_matmul_shader`, `generate_wgsl_parallel_reduction_shader`,
  `generate_wgsl_rtx_raytracing_shader` и `generate_wgsl_cad_vertex_transform_shader` больше не
  экспортируются из `gpu_shader` (модуль `cad_kernels` удалён). Компилятор шейдеров не должен
  содержать прикладные ядра.
- Миграция: брать их из `gpu_compute` под теми же именами; для кода, который и раньше
  импортировал их из `gpu-compute`, ничего не меняется.

## 14. Виды на память буфера `gpu-runtime` учитываются

- Затронуто: `Buffer::get_mapped_range`, `get_mapped_range_mut`, `BufferSlice::get_mapped_range`,
  `get_mapped_range_mut`, `Buffer::uma_slice`, `uma_slice_mut`, `Queue::write_buffer_with`.
- Пересечение с живым видом теперь отказ (G-14): методы `Buffer` возвращают `BufferAccessError`,
  методы `BufferSlice` и `uma_slice*` паникуют с текстом «already mapped», `write_buffer_with`
  возвращает `None`. Читающие виды одного диапазона по-прежнему сосуществуют.
- `uma_slice` возвращает `BufferView<'_>` вместо `&[u8]`, `uma_slice_mut` возвращает
  `WriteOnly<'_, [u8]>` вместо `&mut [u8]`: вид должен жить, пока диапазон занят.
- Миграция: уничтожать вид до запроса следующего на те же байты; там, где нужен `&[u8]` с
  временем жизни буфера, держать `BufferView` и брать срез через `Deref`.

## 15. Заготовки бэкендов убраны из `gpu-runtime`

- Затронуто: модули `gpu_runtime::{vulkan, dx12, opengl, webgpu, cuda}` и их типы
  (`VulkanDevice`, `VkMemoryPropertyFlags`, `Dx12Device`, `OpenGlDevice`, `GlProfile`,
  `WebGpuDevice`, `WgslLanguageFeature`, `CudaDevice` и псевдонимы `CU*`) удалены. Они не
  обращались к драйверу: `probe_available()` всегда возвращал `None`, а `reference_profile()`
  описывал выдуманное устройство.
- Миграция: наличие бэкенда узнавать через `ComputeShaderTarget::is_implemented` и
  `gpu_compute::available_backend_report`; CUDA живёт в `gpu-compute` и `compute-cuda`. Типы
  вернутся вместе с реализацией бэкендов (раздел 4 плана развития) и будут заполняться пробой
  устройства.

## 16. Три wasm-экспорта ядра Svet удалены

- Затронуто: `svet_alloc_u32`, `svet_free_u32` и `svet_compute_lis` больше не экспортируются из
  `svet_wasm.wasm` и убраны из `SvetWasmExports`. Движок их не вызывал: вход LIS передаётся через
  `svet_prepare_lis_input` и `svet_compute_lis_buffered`, поэтому выделять память со стороны JS
  не нужно. Экспортов стало 54.
- Миграция: внешний код, вызывавший ядро напрямую, переходит на буферизованную пару.

## 17. Ошибка трансляции MSL типизирована, storage-текстуры поддержаны

- Затронуто: `gpu_shader::back::msl::write_string` возвращает `Result<_, msl::Error>` вместо
  `Result<_, String>`. Варианты: `UnclosedArrayType` и `UnresolvedArrayLength`; текст `Display`
  прежний.
- Миграция: там, где ошибка нужна строкой, вызвать `to_string()`; `compile_wgsl_for_target`
  по-прежнему возвращает строку.
- Новое поведение: `texture_storage_{1d,2d,2d_array,3d}<format, access>` разбирается (все 17
  форматов WGSL), `textureStore` транслируется, а
  `textureDimensions` возвращает столько компонент, сколько измерений у текстуры (раньше всегда
  две).
- `gpu_runtime::TextureFormat` получил целочисленные и snorm-форматы (`R32Uint`, `R32Sint`,
  `Rg32Uint`, `Rg32Sint`, `Rgba8Snorm`, `Rgba8Uint`, `Rgba8Sint`, `Rgba16Uint`, `Rgba16Sint`,
  `Rgba32Uint`, `Rgba32Sint`): исчерпывающий `match` по формату во внешнем коде нужно дополнить.

## 18. Команды копирования `gpu-runtime` проверяют границы

- Затронуто: `CommandEncoder::copy_buffer_to_buffer`, `clear_buffer`, `resolve_query_set`,
  `copy_texture_to_buffer`, `copy_buffer_to_texture`, `copy_texture_to_texture`,
  `Queue::write_texture`.
- Диапазон вне буфера теперь ошибка валидации, команда не записывается (G-15, G-16). Раньше это
  была запись за пределы памяти или аварийное завершение.
- `copy_buffer_to_buffer` с одним и тем же буфером в роли источника и назначения отклоняется,
  как в wgpu. Миграция: копировать через промежуточный буфер.
