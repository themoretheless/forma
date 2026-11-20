# 0003. Бэкенд HLSL и DirectX 12

Дата: 2026-11-20
Статус: принято

## Контекст

Поддержка Windows ограничивалась кросс-проверкой сборки без компиляции шейдеров в HLSL и без рабочего
графического бэкенда DX12 в `gpu-runtime`. Прежняя конфигурация `gpu-compute` разделяла фичи по
целевым OS (`[target.'cfg(windows)'.dependencies]`), дублируя объявление зависимостей.

## Решение

1. **`gpu-shader` (`crates/gpu-shader/src/back/hlsl.rs`):**
   - Включена кодогенерация `naga/hlsl-out` для моделей шейдеров `ShaderModel::V5_1` и `ShaderModel::V6_0`
     (с привязкой слотов регистров Direct3D и применением `override`-констант на уровне IR).
   - `ComputeShaderTarget::Dx12Hlsl` переведён в реализованное состояние (`is_implemented() == true`),
     с проверкой через `compile_wgsl` на наборе тестовых вычислительных и графических шейдеров.
2. **`gpu-runtime` (`crates/gpu-runtime/src/dx12.rs`, `portable/*`):**
   - Поддержка DX12 подключена через фичу `dx12` и `upstream-wgpu/dx12` на Windows (`x86_64-pc-windows-msvc`),
     при этом единый `gpu-runtime` переводит `Limits::default()` в ограничения `wgpu 30` (включая отключение
     `max_immediate_size`, если вызывающий код не запросил `Features::IMMEDIATE_DATA`).
3. **`gpu-compute` и CI:**
   - Таблица фич `gpu-compute` приведена к единому переносимому профилю `["metal", "vulkan", "dx12", "opengl", "webgpu"]`.
   - В `.github/workflows/ci.yml` джоба `crates` расширена раннером `windows-2022` с прогоном `portable_backends`
     и `gpu-compute` под `WGPU_BACKEND=dx12`.

## Последствия

- Полный стек `gpu-shader` → `gpu-runtime` → `gpu-compute` → `forma` компилируется и проходит интеграционные
  тесты на Windows под DX12 (включая WARP на CI-раннере) без отдельных fork-веток в приложении.
- Требуемая настройка лимитов в `portable/convert.rs` предотвращает отказ `request_device` на адаптерах DX12
  и Vulkan, когда приложение передаёт `adapter.limits()` без `Features::IMMEDIATE_DATA`.
