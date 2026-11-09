# Базовая линия

Снята 2026-11-10 на коммите `f2b0afc` , машина Apple M4 Max, 16 ядер, macOS 26.7,
rustc 1.97.1, node 22.23.4. Сырые логи лежат в `.forma/baseline/` и в git не входят.

Оговорка: во время замеров машина была загружена сторонними процессами (load average 95–220).
Поэтому времена и бенчмарки из этого прогона базой не считаются и должны быть перемерены на свободной машине;
результаты «прошёл / не прошёл» от нагрузки не зависят, кроме двух тестов с таймаутом (см. ниже).

## Состояние гейта до изменений

| Шаг | Результат |
|---|---|
| tokenizer | упал: в соседнем `../tokenizer` есть незакоммиченные правки; с `FORMA_TOKENIZER_DIR=.forma/tokenizer-pinned` проходит |
| version-check, studio-build, три JS-бенча, runtime-gpu, svet-wasm, binding-native, language-example, native-host, кросс-проверки Linux и Windows, studio-legacy, wasm-provenance | прошли |
| studio-js | 592 из 592 |
| svet (`npm test`) | 1510–1511 из 1511; `tests/benchmark.test.ts` упирается в таймаут 5 с под нагрузкой |
| forma, `--features gpu --lib --tests` | 307 прошли |
| forma, `--features native --lib --bin forma` | 252 прошли |

Нестабильные под нагрузкой: `packages/svet/tests/benchmark.test.ts` (таймаут), `test/highlight.test.js` (один раз упал в полном прогоне, отдельно проходит).

## Крейты (первая сборка в этом репозитории, `COMPUTE_REQUIRE_GPU=1`)

| Крейт | Тестов прошло | Упало | Примечание |
|---|---:|---:|---|
| gpu-shader | 0 | 0 | тестов нет |
| gpu-runtime | 18 | 0 | с `--include-ignored`, Metal |
| gpu-compute | 25 | 0 | |
| raster-core | 31 | 0 | пишет кадры в `output/` текущего каталога |
| math-core | 92 | 0 | |
| tensor-core | 53 | 0 | `--features conformance` |
| compute-core | 164 | 0 | |
| compute-mlx | 148 | 0 | `COMPUTE_REQUIRE_MLX=1`, 677 с под нагрузкой |
| math-compute | 41 | 0 | `--features gpu,tensor,tensor-wgsl,tensor-mlx` |
| compute-cuda | 180 | 0 | устройства CUDA нет: тесты устройства завершаются пропуском, проверена только хост-часть |

Итого 752 теста, все прошли. Предупреждений компилятора нет.

## Размер (после форматирования)

| Область | Строк | Файлов |
|---|---:|---:|
| crates/forma/src | 37 172 | 31 |
| crates/gpu-shader/src | 4 416 | 13 |
| crates/gpu-runtime/src | 9 295 | 20 |
| crates/gpu-compute/src | 3 072 | 14 |
| crates/raster-core/src | 3 936 | 14 |
| crates/math-core/src | 5 916 | 27 |
| crates/math-compute/src | 5 963 | 26 |
| crates/tensor-core/src | 9 850 | 42 |
| crates/compute-core/src | 11 322 | 57 |
| crates/compute-mlx/src | 6 859 | 47 |
| crates/compute-cuda/src | 16 309 | 78 |
| packages/svet/crates/svet-wasm/src | 3 819 | 2 |
| packages/svet/src/engine | 18 720 | 22 |
| src (Studio) | 19 666 | 71 |

Самые большие файлы: `form_document.rs` 4858, `runtime.rs` 4377, `forma.ts` 3483, `svet-wasm/src/lib.rs` 3305,
`gpu.ts` 3208, `native_window.rs` 3164, `reactivity.ts` 2671, `metal.rs` 2623, `src/main.js` 2574, `dom.ts` 2574.

`public/vector-pkg/forma_bg.wasm`: 20 909 792 байт, 12 655 429 после gzip. Встроенный wasm svet: 53 405 байт.

## Дублирование (jscpd, порог 70 токенов, без тестов и evidence)

213 клонов, 2855 строк из 138 735 (2,06 %). Лидеры: `compute-core/examples/bench_tensor_reduction*.rs`,
пары `math-compute/src/{gpu,cuda}/{bounds,moments,neighbors,distance}.rs`, `gpu-runtime/src/metal.rs`,
`forma/src/main.rs`, `form_document.rs`, `raster-core/src/pipeline.rs`, `tensor-core/src/host_tensor.rs`.

## Циклы импорта (madge)

Studio `src/`: нет. `packages/svet/src/engine`: один, `gpu.ts` ↔ `gpu-resident-bridge.ts` (только по типам).

## Ещё не измерено

Счётчики clippy по линтам, длина и сложность каждой функции, покрытие, `cargo udeps`, `knip`,
времена сборки, `npm run bench` и бенч-примеры на свободной машине, устройство CUDA.
