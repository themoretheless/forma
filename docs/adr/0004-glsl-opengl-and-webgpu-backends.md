# 0004. Бэкенды GLSL / OpenGL ES и Browser WebGPU

Дата: 2026-11-20
Статус: принято

## Контекст

На системах без Vulkan/DX12/Metal (контейнеры, встраиваемые драйверы Mesa EGL/GLES, среды с виртуальным
OpenGL), а также в целевой архитектуре `wasm32-unknown-unknown` (браузерный WebGPU), `gpu-runtime` должен
предоставлять рабочий переносимый путь без ложных заглушек и без условных `#[allow(dead_code)]`.

## Решение

1. **`gpu-shader` (`crates/gpu-shader/src/back/glsl.rs`):**
   - Подключён вывод `naga/glsl-out` (`Version::Desktop` — GLSL 450 и `Version::Embedded` — GLSL ES 310)
     для каждой точки входа отдельно через `compile_entry_point` и агрегированно через `write_all_entry_points`.
   - Назначены детерминированные последовательные слоты привязок (`sequential_binding_slots`) для OpenGL ES.
2. **`gpu-runtime` (`crates/gpu-runtime/src/opengl.rs`, `webgpu.rs`, `portable/*`):**
   - Для OpenGL / GLES включён профиль downlevel-лимитов при передаче `Limits::default()`, что гарантирует
     успешный запуск на контекстах OpenGL ES 3.1 (проверяется на Linux CI с `WGPU_BACKEND=gl` и `libegl1`/`libgles2`).
   - Для `wasm32-unknown-unknown` реализован полностью асинхронный путь (`PortableDevice::new_async`,
     асинхронные `enumerate_adapters` и `request_adapter` без блокировки потока браузера).
3. **Приоритет Metal на Apple:**
   - На `macOS` и `iOS` прямым бэкендом по умолчанию остаётся `MetalDevice` с zero-copy UMA/ASD,
     signal-driven завершением и эксклюзивным управлением `CAMetalLayer`; переносимый слой на macOS не
     дублирует Metal-адаптеры в обычном `Instance`, а используется на других ОС либо в тестовом режиме
     (`Instance::new_portable_for_testing`).

## Последствия

- Все четыре графических таргета (`Metal`, `Vulkan/SPIR-V`, `DX12/HLSL`, `OpenGL/GLSL`, `BrowserWebGPU`)
  проверяются компиляцией на каждом прогоне, а `OpenGL ES` и `wasm32` входят в регулярную матрицу CI.
- Нулевой долг по предупреждениям компилятора сохраняется на всех четырёх таргетах без добавления `#[allow]`.
