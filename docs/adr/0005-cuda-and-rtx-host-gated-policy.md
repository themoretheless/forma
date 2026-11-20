# 0005. Политика CUDA и RTX: изоляция и проверка только на NVIDIA-хосте

Дата: 2026-11-20
Статус: принято

## Контекст

Ранее в `gpu-runtime` существовал псевдослой `gpu_runtime::rtx`, имитирующий возможности ray tracing без
реальных структур ускорения (описано в разделе 9 `MIGRATIONS.md`), а `ComputeShaderTarget::CudaPtx` и
флаг `Backends::CUDA` могли ввести потребителей в заблуждение о наличии WGSL-бэкенда для CUDA поверх WebGPU.

## Решение

1. **Явный отказ в `gpu-shader` для `ComputeShaderTarget::CudaPtx`:**
   - `ComputeShaderTarget::CudaPtx.is_implemented()` остаётся `false`.
   - Вызовы компиляции под `CudaPtx` возвращают явную ошибку `"no shader back end for CudaPtx is implemented"` —
     никаких фиктивных или «заглушечных» PTX-строк не порождается (`G-11`).
2. **Политика `Backends::CUDA` в `gpu-runtime` (`crates/gpu-runtime/src/cuda.rs`):**
   - `Backends::CUDA` **никогда** не порождает и не эмулирует графические `wgpu`-адаптеры
     (`enumerate_adapters(Backends::CUDA)` всегда возвращает пустой вектор). Графические/вычислительные
     нагрузки NVIDIA внутри WebGPU-пайплайна выполняются через Vulkan или DX12.
   - Нативное выполнение CUDA-ядер (PTX и hardware RT при наличии) сосредоточено в `gpu-compute::cuda` и
     `compute-cuda` и закрыто динамической проверкой наличия системного драйвера NVIDIA (`probe_driver_available`:
     `/dev/nvidiactl` / `libcuda.so.1` на Linux, `nvcuda.dll` в `System32` на Windows; на macOS/iOS/Android/wasm
     всегда `false`).
3. **Запрет фиктивного RTX:**
   - Псевдо-RTX API и рекламирование `EXPERIMENTAL_RAY_QUERY` на адаптерах без реальной поддержки не
     возвращаются (`G-6`). Любой будущий RT-бэкенд может быть добавлен только при наличии реальных BLAS/TLAS
     и тестов на физическом оборудовании.

## Последствия

- Нет ложнозелёных тестов или выдуманных отчётов о CUDA/RTX-производительности на хостах без GPU NVIDIA.
- Сборка с `--features cuda` компилируется и безопасно тестируется (хост-часть и динамическая проба драйвера)
  в общем контуре на macOS/Linux/Windows без падений и без поддельных адаптеров.
