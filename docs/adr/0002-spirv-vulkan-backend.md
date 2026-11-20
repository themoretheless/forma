# 0002. Бэкенд SPIR-V и Vulkan

Дата: 2026-11-20
Статус: принято

## Контекст

`gpu-shader` транслировал WGSL только в Metal Shading Language (MSL), а `gpu-runtime` на всех платформах
кроме macOS/iOS возвращал пустой список адаптеров и содержал незаглушённые ветки. Из-за этого
Linux-сборка `crates/forma` была вынуждена подключать отдельную зависимость `wgpu-linux = { package = "wgpu" }`,
держать условную компиляцию `#[cfg(target_os = "linux")]` в `crates/forma/src/gpu.rs` (чтение таймстампов,
проверка простоя очереди, test-hook потери устройства) и не проходила единую валидацию WGSL в `gpu-shader`.

## Решение

1. **`gpu-shader` (`crates/gpu-shader/src/back/spv.rs`, `back/lower.rs`, `compiler.rs`):**
   - Подключён `naga = "=30.0.2"` (с фичей `spv-out`) и `spirv = "0.4"`.
   - Любой WGSL сначала проходит парсинг и семантическую/типовую проверку (`valid::Validator`),
     включая проверку допустимости `@builtin` для стадии и направления.
   - Понижение (`back::lower`) применяет pipeline `override`-константы через `naga::back::pipeline_constants::process_overrides`,
     генерирует двоичный поток слов SPIR-V 1.3 (`compile_wgsl_to_spirv`), проверяет заголовок и структуру
     инструкций через `spirv` (`validate_spirv_words`) и формирует текстовый дизассемблер из бинарных слов.
2. **`gpu-runtime` (`crates/gpu-runtime/src/vulkan.rs`, `portable/*`):**
   - Подключён переносимый бэкенд поверх `upstream-wgpu =30.0.1` (`vulkan`), сохраняющий прямой
     нативный драйвер `MetalDevice` на macOS/iOS без накладных расходов и без двойной регистрации адаптеров.
   - `Adapter::from_portable` и `PortableDevice` реализуют единый контракт `gpu-runtime` (буферы,
     переносимые тени отображений с авто-сбросом при `Drop`, текстуры, compute/render pass,
     `ErrorScope`, `Device::destroy()`, `Surface`, `Queue`).
3. **Унификация `crates/forma`:**
   - Прямая зависимость `wgpu-linux` и три ветки `#[cfg(target_os = "linux")]` в `crates/forma/src/gpu.rs`
     удалены; `forma` на всех платформах работает через единый API `gpu-runtime` (рантайм-ветвление UMA/DUMA
     по `adapter_info().backend == Backend::Metal && memory_architecture.is_unified()`).
4. **CI-матрица:**
   - В `.github/workflows/ci.yml` джоба `crates` на `ubuntu-24.04` устанавливает `mesa-vulkan-drivers`
     (программный растеризатор `lavapipe`) и прогоняет `portable_backends` и `COMPUTE_REQUIRE_GPU=1` для `gpu-compute`.

## Последствия

- Одна зависимость `wgpu` (=30.0.1, внутренне для переносимого слоя `gpu-runtime`) и единый `Cargo.lock` без дубля `wgpu-linux`.
- Идентичная предвалидация WGSL и семантика `Device::destroy()` / `WriteOnly` / `MappedRanges` на Metal и Vulkan.
- Контрактное сужение описано в [MIGRATIONS.md](../refactor/MIGRATIONS.md): `Instance::create_surface` теперь
  требует `HasWindowHandle + HasDisplayHandle + Send + Sync + 'a`.
