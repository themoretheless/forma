# Замороженные снимки исходников (`*-source`)

Метод: каждый файл снимка сопоставлен с живым файлом по тому же пути (или по имени файла для плоских снимков).
Сравнение побайтное; для различающихся файлов посчитаны символы (`fn`, `struct`, `enum`, `trait`, `const`),
определённые в снимке и отсутствующие во всём живом коде `crates/`. Истории до импорта в репозитории нет,
поэтому «влито» означает «живой код содержит всё, что есть в снимке, либо более позднюю версию того же».
Сырые данные: `.forma/baseline/snapshots.json` (не в git).

| Снимок | Файлов | Идентичны | Отличаются | Нет в живом | Символов только в снимке | Вердикт |
|---|---:|---:|---:|---:|---:|---|
| compute-core/benchmarks/tensor-attention-initial-source | 16 | 7 | 9 | 0 | 2 (`down64`, `up64` в WGSL) | проверить: помощники f64 в шейдере |
| compute-core/benchmarks/tensor-low-attention-f16-cache-source | 1 | 0 | 1 | 0 | 0 | влито, удалять |
| compute-core/benchmarks/tensor-low-attention-guarded-f16-source | 3 | 2 | 1 | 0 | 0 | влито, удалять |
| compute-core/benchmarks/tensor-low-attention-initial-source | 223 | 183 | 39 | 1 (`Cargo.lock`) | 1 (`low_attention_dtype` в WGSL) | проверить один символ, иначе удалять |
| compute-core/benchmarks/tensor-low-attention-query-cache-source | 3 | 0 | 3 | 0 | 1 (тест `query_cache_covers_…`) | НЕ влито: вариант с query-cache и его тест отсутствуют в живом коде |
| compute-core/benchmarks/tensor-low-attention-specialized-source | 4 | 2 | 2 | 0 | 0 | влито, удалять |
| compute-core/benchmarks/tensor-low-normalization-initial-source | 11 | 4 | 5 | 2 (отладочные `debug_low_stats*.rs`) | 0 | влито; два отладочных файла не являются продуктовым кодом, удалять |
| compute-core/benchmarks/tensor-low-scatter-initial-source | 1 | 0 | 1 | 0 | 0 | влито, удалять |
| compute-core/benchmarks/tensor-normalization-initial-source | 12 | 6 | 6 | 0 | 0 | влито, удалять |
| compute-mlx/benchmarks/low-matmul-shared-source | 1 | 0 | 0 | 1 (`Cargo.toml` бывшего workspace) | 0 | только манифест чужого workspace, удалять |
| compute-mlx/qualification/compiled-source | 87 | 54 | 33 | 0 | 35 | проверить: помощники заменены `native/lowering/*` |
| compute-mlx/qualification/compiled-typed-initial-source | 2 | 0 | 2 | 0 | 0 | влито, удалять |
| compute-mlx/qualification/compiled-typed-source | 104 | 77 | 27 | 0 | 28 | проверить: то же, что compiled-source |
| compute-mlx/qualification/low-matmul-routed-source | 4 | 3 | 1 | 0 | 2 (`VECTOR_SOURCE`, `low_matmul_view`) | проверить: вариант маршрутизации matmul |
| compute-mlx/qualification/low-matmul-simd-source | 5 | 2 | 3 | 0 | 1 (`THREADS`) | проверить: SIMD-вариант |
| compute-mlx/qualification/low-matmul-tiled-source | 81 | 47 | 34 | 0 | 33 | проверить: то же, что compiled-source |

Итог: 8 снимков влиты и удаляются на этапе 1; 1 снимок (`query-cache`) содержит невлитый вариант и остаётся;
7 снимков требуют проверки по публичным операциям (покрывает ли живая реализация через `native/lowering/*`
все операции, которые реализовывали удалённые помощники) до решения об удалении.

## Удалено

Восемь снимков с вердиктом «влито» удалены из дерева (33 файла). Их содержимое доступно в коммите `f2b0afc`:
`git show f2b0afc:crates/compute-core/benchmarks/<снимок>/<файл>`. Манифесты отпечатков
(`*-source-fingerprints.json`) и отчёты прогонов оставлены: они фиксируют хэши и результаты исторических запусков
и продолжают ссылаться на эти снимки по имени.
