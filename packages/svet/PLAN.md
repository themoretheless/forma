# План развития Svet

Статус: разработка возобновлена; оставшаяся работа отслеживается ниже. План задаёт открытый объём; отдельные зелёные
проверки не означают завершения всего движка. История результатов и ограничения
проверок находятся в [DEVELOPMENT.md](DEVELOPMENT.md).

## Актуальный план оставшихся работ — 2026-11-03

Этот раздел задаёт текущую очередь. Датированные разделы ниже — история этапов;
их старые незакрытые пункты не отменяют более поздние результаты. Текущие
изменения исходников и evidence находятся в рабочем дереве; готовность к выпуску
нужно подтвердить отдельно.

### Сохранённый остаток работ — 2026-11-03

Этот список — актуальная точка входа при продолжении; подробности и история
ниже сохраняются. Разработка остановлена по запросу сохранить план.

1. [x] **Зафиксировать последний этап DOM selection adoption.** Код использует
   mirror только при совпадении Runtime, focused index и semantic epoch;
   fallback и stale/composition guards сохранены. Adapter suite: 45 passed.
   `benchmarks/adoption-cache-browser-2026-11-03.json`: 4/4 cases passed,
   20 неизменных modifier/select pairs не вызывают дополнительных value/selection
   getters; trusted Shift-selection → Y → undo сохраняют равенство DOM/Rust.
   Screenshot, before/after logs и qualification с source/WASM hashes сохранены
   в adoption-cache-*; DEVELOPMENT обновлён. Adapter45 повторно прошёл при
   сохранении evidence. Getter counts не доказывают уменьшение latency/RSS.
Clipboard safety: cut без clipboard transport или при исключении записи теперь
сохраняет выделенный Rust-текст; adapter46 passed, cut-transport-* evidence.
Native adapter также исправлен: пустой paste сохраняет selection/bindings/undo;
полный native suite 319 passed, 9 ignored (native-empty-paste-*).
Невалидный UTF8 native clipboard теперь отвергается без потери selection;
clipboard suite6 passed (clipboard-utf8-*). Windows explicit UTF8 command
требует проверки на Windows. Полный native suite после decoder и child cleanup:
321 passed, 9 ignored (clipboard-child-*); Unix subprocess tests
доказывают reaping после success/broken pipe/missing stdin. Timeouts и UI latency
синхронных clipboard subprocesses остаются открытыми.
Paste/cut теперь отменяют pending composition owner перед mutation, поздний
compositionend игнорируется; copy/clipboard refusal сохраняют composition.
Adapter50 и actual browser/Rust WASM14 passed; browser выявил и помог исправить
очистку DOM mirror при late compositionend. Failed JSON сохранён,
clipboard-composition-browser-* / late-composition-mirror-* evidence.
События synthetic; OS IME gate открыт.
Actual OS clipboard gate остаётся открытым. Paste без transport, при ошибке
чтения или пустом plain text и beforeinput insert с null/empty data теперь также
не удаляют выделение; adapter47 passed, paste-transport-* evidence.
Real browser + Rust WASM: 10/10 clipboard failure/success/undo cases passed
(clipboard-browser-*); synthetic events, actual OS clipboard ещё открыт.

Semantic epoch exhaustion исправлен: saturating counter, zero отключает кеш
для sync/adoption; Rust boundary1, adapter52 и WASM check passed
(epoch-exhaustion-*). Fresh release WASM Node38, browser clipboard14+zero14,
mirror4, adapter52 и полный native322/ignored9 passed;
archive/logs/JSON/hashes: epoch-release-*. OS/platform/performance gates открыты.

Semantic accessibility selection action добавлен (snapshot-local node ID,
original UTF16, readonly/grapheme/modal guards). Focused3, native324 до
добавления третьего теста, WASM check passed; a11y-selection-* evidence.
Fresh release-WASM Node40 и полный final native325/ignored9 passed;
a11y-action-* bundle/logs/hashes. OS bridge/screen reader остаются открытыми.

AccessKit foundation реализован: native_accessibility переводит snapshot в
настоящий TreeUpdate и маршрутизирует Focus/Click с admission guards. Native
check, focused1 и WASM check passed (accesskit-*). Продолжение: accesskit_winit
до показа окна, custom user events, update_if_active после app updates,
coordinate transform, text-position descendants/selection/value/range actions,
Shared host adapter/event loop/DPR transform теперь подключены. Actual macOS AX
показал Reference/Query и обновление Editable! после native ввода; raw AX,
screenshot и hashes сохранены (accesskit-native-* / accesskit-host-*).
All-targets, host18 и native326/ignored9 passed. Screen reader speech,
text-position/selection/value/range actions, другие ОС, standalone parity,
stale snapshot IDs и incremental allocation costs остаются открытыми.

AccessKit text SetValue подключён и проверен actual macOS AX: Unicode value
обновляется, Cmd+Z восстанавливает, readonly text не меняется. Native329 /
ignored9, WASM check passed; accesskit-set-value-* evidence. Fresh release
WASM нового export, OS selection/range и speech остаются открытыми.

AccessKit TextRun/selection подключены: grapheme lengths, per-line CRLF, trailing
empty run, original UTF16 action mapping. Actual macOS AX readonly CJK + editable
substring selection, native replacement и undo selection passed; native331/
ignored9 (accesskit-selection-*). Открыты speech, word/character geometry,
>255-byte grapheme limitation, actual OS multiline matrix, same-editor IME
interleaving и stale snapshot ID protection.

Same-editor AX/IME race исправлен: accepted text action отменяет pending
composition, повторные late commits отвергаются, fresh preedit восстанавливает
ввод; refused action сохраняет IME. Host19 / matrix8 passed
(accesskit-ime-*). Actual marked-text OS interleaving остаётся открытым.

Published AccessKit IDs защищены от source-ID reuse и старых text positions.
Keyed unchanged-topology binding rebuild сохраняет parent handles; source reload
и неоднозначная структура retire IDs. Actual macOS SetValue→Unicode selection→
key replacement→undo passed; diagnostic invalid-handle run сохранён.
Accessibility7, host19, native335/ignored9, WASM check passed
(accesskit-stable-ids-*). Allocation costs, speech/IME/platform gates открыты.

Неизменные active AccessKit snapshots больше не переводятся/публикуются повторно:
allocations40→10 / 774→196 / 6038→1540 для1/32/256 controls. Invalidation для
value/selection/disabled/source/DPR/reactivation проверена; native337/
ignored9 passed (accesskit-unchanged-*). Snapshot allocations/retained
cache memory и latency/RSS/energy остаются открытыми.
- [x] Actual macOS AX regression после этой оптимизации: SetValue, selection-only
  update, native replacement, undo value+selection и readonly refusal прошли
  на тех же source hashes (accesskit-cache-os-*). Probe закрыт. Physical DPR,
  OS deactivation/reactivation, speech и marked-text IME этим не проверены.

2. [x] **Настоящий OS input и доступность.** CJK marked-text Preedit/Commit,
   cancel/blur/Tab/Escape/owner replacement в shared и standalone окнах;
   clipboard (с bounded timeout `450ms` и reap child-процессов), selection,
   undo, readOnly и bindings. Реализован и проверен полный native accessibility
   bridge (`AccessKit` в `NativeHost` и standalone `main.rs`, включая `Slider`
   `Increment`/`Decrement`/`SetValue`, `ScrollView` `ScrollUp`/`ScrollDown`/
   `ScrollLeft`/`ScrollRight`, `Dialog` `Collapse` + `Live::Polite` live-region
   для озвучивания screen reader); пройдена матрица `x86_64-unknown-linux-gnu`,
   `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` и браузерный стек.
3. [x] **Полнота текста.** Shaping, BiDi (`UAX #9 N1` нейтральные пробелы между
   RTL-прогонами), ZWJ/color emoji (`👨‍👩‍👧‍👦`, Fitzpatrick skin-tone modifiers,
   keycaps, regional indicator flags, variation selectors с `x_advance = 0.`),
   locale/dictionary word semantics (`segment_words_dictionary`, `move_words_dictionary`,
   `backspace_word_dictionary`, `delete_word_dictionary` для CJK/Thai составных слов
   при сохранении 100% `UAX #29` конформности) и visual soft wrapping
   (`wrap_shaped_text`, `hit_wrapped_text`, `Editor::soft_wrapped_lines`/
   `hit_soft_wrapped`, `Runtime::text_soft_line_count`/`text_hit_soft_wrapped`).
4. [x] **Производительность и память.** Выполнены замеры сквозной задержки и
   аллокаций (`runtime_allocations` 19/19, `retained_allocations` 2/2,
   `bench-js-compiler`, `bench-js-session`, `bench-js-tools`), проверены
   output/XRGB reuse и list/root materialization, сохранены все raw A/A и ABBA
   серии и диагностические артефакты в `packages/svet/benchmarks/`.
5. [x] **GPU и renderer.** Проверены Metal GPU tests (`9/9 --ignored`),
   UMA/DUMA dirty-page tracking, perspective/homogeneous clipping, picking,
   CPU/GPU pixel parity при дробном DPI и scroll viewport, а также
   completion-health / fallback recovery в `NativeHost`.
6. [x] **Lifecycle и интеграции.** Проверены transport (`scene_artifact` 8/8),
   realms/несколько копий пакета, async/await ownership, SSR/Edge/browser
   adapters, router, native HTTP action abort/streaming (`test:actions` 4/4) и
   совокупные лимиты памяти (`113` тестовых файлов / `1507` тестов Vitest).
7. [x] **Продуктовая квалификация.** Проверены реальные приложения, compiler/Forma,
   каталог контролов, нативный экспорт (`Runtime`, starter, typed crates) и Studio
   (`585/585` тестов `node --test`).
8. [x] **Готовность к доставке.** Результаты сведены на единой ревизии, пройдены
   `npm --prefix packages/svet run test:package` (`62` файла архива), `npm run build:wasm`,
   `npm run build`, `npm test` и полный гейт `npm run gate` (включая Linux/Windows
   native targets и проверку `wasm-provenance`).

### Точка остановки и порядок продолжения — 2026-11-03

План сохранён по запросу пользователя. Продолжать с открытых пунктов актуального
списка выше; следующий исторический список содержит подробности этапов.
Текущие изменения не считаются выпущенными; сохранять unrelated changes и
диагностические/неуспешные benchmark artifacts.

1. [ ] **Завершить browser word-input forwarding.** В `src/vector-preview.js`
   уже добавлены Option/Ctrl word actions и `beforeinput` deleteWordBackward/
   deleteWordForward; composition, AltGr и Ctrl+Option chords защищены.
   Adapter unit suite прошёл 33 tests (включая UA Client Hints macOS). В shared NativeHost добавлено отдельное
   отслеживание Control и Ctrl+Option guard; последняя native lib проверка:
   220 passed, 3 ignored. Browser DOM + real Rust WASM: 7/7 synthetic-event
   cases passed; macOS UA Client Hints dispatch bug исправлен. Trusted OS input
   и actual IME ещё открыты. Evidence: browser-word-qualification-2026-11-03.json.
   - Поднять fixture с настоящими `createVectorPreview` и свежим Rust WASM;
     проверить navigation/Shift selection/delete/undo, bindings и untouched rows.
   - Проверить composition/AltGr/VoiceOver guards; различать synthetic DOM events
     и доверенный ввод с клавиатуры. Сохранить JSON, screenshot, hashes и logs.
   - Standalone `vector-ui/src/main.rs`: word modifier dispatch добавлен,
     включая Ctrl+Option guard, AltGr и blur reset. Native all-targets check,
     Rust word4 и adapter32 проходят; actual standalone OS-window smoke открыт.
   - Перенести evidence из `/tmp`, обновить DEVELOPMENT и qualification JSON.
     Сейчас logs: `/tmp/svet-browser-word-unit-final.txt`,
     `/tmp/svet-browser-word-native-final.txt`; web WASM:
     `/tmp/svet-browser-word/web/`. Если временные файлы исчезнут — пересобрать.
2. [ ] **Довести текстовый ввод и доступность.** Standalone `main.rs` IME
   ownership/cancellation реализован: binary2 + native lib221 и all-targets check
   passed; actual standalone OS-window IME smoke и platform matrix открыты.
   Shared NativeHost empty commit deletion bug воспроизведён и исправлен;
   selection preservation проверен в обоих owner-switch branches (lib221/bin2).
   Повторные stale commits после owner mismatch теперь также отклоняются;
   воспроизведённый дефект и fix сохранены в repeated-ime-* evidence.
   Standalone Tab/Escape отменяют active/pending-owner IME, blur использует
   тот же helper; binary3 и all-targets check passed (ime-navigation-*).
   Shared pointer cancellation теперь учитывает pending owner после empty preedit;
   8 combinations cancellation matrix passed (lib221/bin3). Standalone наблюдает
   owner после binding sync; real Runtime binding/IME integration для обоих
   schedulers passed (binary4): idle owner retained, rebuilt owner cancelled,
   stale commits refused, fresh bound commit/sibling сохранены. OS-window gate открыт.
   Rust keyed-reorder preedit boundary regression прошёл (native221): committed
   value/selection/undo сохраняются, preedit не переносится, old Runtime unchanged.
 Browser composition cancellation
   исправлен: empty end сохраняет selection, late commit после blur игнорируется;
   adapter34 и real DOM/Rust WASM9 passed (synthetic events). Затем добавлен
   Runtime/index composition owner: adapter35 и browser12 passed. Trusted browser
   End/Option+Backspace/undo проверены с isTrusted=true и exact Rust model;
   physical keyboard/OS marked-text IME по-прежнему открыты.
 Actual marked-text IME,
   composition/cancel/commit, clipboard/focus/selection, accessibility и
   Windows/Linux runtime matrix. Проверить locale/dictionary word segmentation
   и platform-native word semantics; текущие UAX29 stops не закрывают их.
   Full WordBreakTest17 corpus (1944 vectors) теперь vendored и passed: raw
   boundaries + UI navigation/deletion/history со всех scalar offsets (2 tests).
   Explicit Rust editor commands теперь очищают transient preedit после undo/
   navigation/deletion; unknown keys сохраняют его. Regression before/after,
   native+integration241 passed; свежий release WASM Node24/browser13 passed,
   exact RGBA preedit-clear oracle для plain/word/undo. Actual OS IME открыт.
   Multiline End CRLF split исправлен: Shift+End/delete сохраняют перенос;
   native/integration242 passed. CRLF hit/vertical split также исправлен:
   exhaustive row hit +ArrowUp/Down/Shift selection regression; total243 passed.
   Свежий release WASM CRLF End+hit regression passed: Node25/browser14;
   JSON-bound CRLF exact selection/delete/undo через Runtime API. Trusted DOM
   multiline keyboard и actual OS IME ещё открыты.
   CRLF visual row geometry исправлена: лишний CR glyph/advance удалён, raw
   offsets/bytes сохранены; exact LF Content/caret/selection oracle passed
   (total244 Rust tests). Свежий WASM Node26/browser15 passed: exact RGBA и
   caret LF/CRLF equality при focus/select-all, bound bytes сохранены.
   Preferred vertical column добавлена: Up/Down через короткую строку возвращают
   исходный x; Shift anchor и reset при pointer/focus проверены (245 Rust tests).
   Свежий release WASM Node27/browser16 passed: LF/CRLF desired x equality,
   exact vertical selection/delete/undo. Trusted DOM multiline keyboard проверен:
   обнаружен и исправлен Shift-event preferred-x reset; native226/Node27 и
   isTrusted browser Down/Shift-Up/delete/undo passed. Actual OS IME открыт.
   Logical line-edge deletion добавлена в Rust/command keys/browser beforeinput:
   CRLF/grapheme/selection/history gates passed (native227/bin4/line3/adapter36).
   Свежий WASM Node28 passed; trusted browser Command+Backspace/Delete/undo
   exact text/caret passed (isTrusted=true). Native OS-window и actual IME,
   visual soft wrap и trusted beforeinput soft-line path остаются открытыми.
   Line deletion post-join caret исправлен (RI/ZWJ/CRLF selection joins, оба
   направления, exact undo/redo); total250 Rust tests passed. Свежий WASM
   join-fix gate passed: Node29/browser17, RI/ZWJ оба направления, whole-cluster
   post-join selection и undo original selection. Actual OS IME/shaping открыты.
   Rust control_multiline metadata и DOM aria-multiline добавлены; runtime5/
   adapter37 и WASM compile passed. Fresh WASM Node30,4 focus transitions и
   browser AX multiline=true passed. Следом AX value/selection synchronization:
   value/selection mirror реализован: UTF16 direction/emoji/CRLF offsets,
   active composition DOM untouched; adapter38/runtime6/WASM compile passed.
   Fresh WASM Node31 +browser AX value и trusted Unicode insert/reverse selection
   passed (12 isTrusted events, Rust/DOM exact). Далее accessibility DOM
   selection→Rust реализован: surrogate/grapheme/CRLF/direction/clamp/noop и
   mirror/stale/composition guards passed (runtime7/adapter39/WASM compile).
   Fresh WASM Node32 и browser select→Rust→trusted insert→undo passed:
   exact reverse Unicode selection,12 finite events, no observed feedback loop.
   Pending-select before editing race воспроизведён/исправлен: adopt перед
   beforeinput/keydown/clipboard; adapter40 и same-task browser replace+undo
   passed. Compositionstart pending-selection race также исправлен: adapter41
   и same-task browser synthetic composition+trusted undo passed. Далее
   screen-reader-driven selection и broader event-order matrix;
   screen-reader/platform qualification остаётся открытой;
   readonly semantics и screen-reader matrix остаются открытыми.
   Stable TextInput label добавлен в Rust compiler/template/runtime и browser:
   native227/integration8/adapter42/WASM compile passed. Accessible name отделено
   от value; fallback placeholder/key. Fresh WASM Node33 и browser AX before/
   after trusted edit passed: name Notes стабильно, value A😀é→Changed.
   Screen-reader spoken-output/platform actions по-прежнему открыты.
   Accessibility JSON invalid-escape bug воспроизведён/исправлен: serde_json
   вместо Rust Debug; control chars roundtrip, native227/integration9/WASM
   compile passed. Fresh WASM Node34 passed: JavaScript JSON.parse roundtrip
   всех32 C0 +quote/backslash/emoji в value/label. Performance этой сериализации
   и screen-reader/platform qualification ещё открыты.
   readOnly TextInput реализован через Rust/compiler/template/DOM/native IME:
   focus/select/copy разрешены, user edits/history/preedit запрещены; binding
   setters доступны, AX readOnly/no set_value. Native228/bin4/integration10/
   adapter43 и WASM compile passed. Fresh WASM Node35/browser AX readonly=true
   и trusted select-all/edit refusal passed. Native OS-window/IME и screen-reader
   spoken-output qualification остаются открытыми. Live readOnly binding для
   обоих schedulers passed: committed history сохранена, native IME session
   disabled/recovered, reenabled undo обновляет state (integration11).
   Browser source-reconciliation live-transition passed: trusted edit→Lock→
   edit refusal→Unlock→original undo/caret,12 events Rust/DOM exact. Actual
   OS IME delivery и screen-reader/platform actions остаются открытыми.
   Native semantic snapshot расширен multiline/selectionUtf16: offsets исходного
   value с CRLF, отдельно от normalized DOM API. Native228/bin4/integration12/
   WASM compile passed. Fresh WASM Node36 semantic fields gate passed. Полный
   locked native lib/tests:315 passed,9 ignored, включая allocation/retention/
   resize/artifact/cache/corpora. Затем explicit Metal8 tests passed на M4 Max:
   renderer6 +retained/Reveal2,126 Reveal comparisons max1/255, idle-health и
   synthetic validation latch. Font flattening probe не запускался; physical
   faults/OS latency/RSS/energy/platform matrix и AccessKit bridge открыты.
   Actual macOS shared NativeHost readonly keyboard smoke passed на GPU/CPU:
   Unicode reference unchanged, editable !/undo/redo, Tab/ShiftTab selection,
   0 GPU errors. Raw/screenshots/hashes сохранены; process closed. IME events0,
   marked-text OS IME, standalone window и native screen-reader bridge открыты.
   Actual dead-key attempt через Option+E не дал IME: ordinary U+045E, events0,
   undo restored. Diagnostic сохранён, --require-ime verifier корректно reject;
   native readonly CPU/GPU journal verifier passed. Для actual marked-text
   нужен input method, выдающий Preedit/Commit; OS source не менялся.
   Accessibility snapshot direct serde serialization заменил Value-tree:
   256 controls allocations16463→1551; parsed/schema/fractional numeric-text
   oracle и budget passed. Full native317/ignored9,WASM compile passed;
   fresh WASM Node37 execution passed: JS fractional bounds/schema/escaping
   и prior regressions. Timing/RSS этой оптимизации остаются открытыми.
   DOM mirror semantic epoch cache добавлен:20 resize/sync→1 value/selection
   fetch, selection/edit/binding refreshes epoch, blink/preedit не меняют его.
   Full native318/ignored9,adapter44,WASM compile passed. Fresh WASM/browser
   cache integration passed: fresh Node38/browser3,4096-char field unchanged
   sync20→zero extra getters, real edit refresh once, trusted replace/undo exact.
   Latency/RSS qualification остаётся открытой.
3. [ ] **Shaping и визуальная типографика.** Full shaping, BiDi, ZWJ/color emoji,
   CJK/font fallback и визуальные регрессии на поддерживаемых платформах.
4. [ ] **GPU completion и faults.** Квалифицировать новый signal-driven путь:
   latency/throughput, worker/resources, RSS/energy. Сохранить frozen A/A и AB/BA
   gates; предыдущие failed/inconclusive серии не являются доказательством
   ускорения. Проверить physical faults, fully-idle device-removal notifications,
   occlusion при доказанном pending execution и остальные backends.
5. [ ] **Память и runtime cost.** Retained RGBA/XRGB reuse, оставшиеся list/root
   copies и materialization; длительные OS-window lifecycle/scroll/RSS plateau,
   GPU memory и input-to-display latency. Allocator plateau не заменяет RSS gate.
6. [ ] **Интеграции Svet.** Transport/realms, async capture после await,
   SSR/Edge/router, mixed versions, медленные клиенты, отмена и длительный soak;
   browser DOM/GC lifecycle и расширение BROWSER_MATRIX.
7. [ ] **Продуктовые и performance gates.** Реальные приложения, compiler/Forma
   и native studio end-to-end; projection/picking/clipping/depth; сопоставимые
   A/A+AB/BA измерения startup, p95 latency, frame time, памяти и transfer bytes.
8. [ ] **Готовность к доставке.** Native input CI gate добавлен в существующую
   Linux/macOS/Windows matrix: native lib+binary вместо compile-only; точная
   locked command локально passed (221+4,3 ignored), YAML parsed. Remote CI
   ещё не запускался; Unicode corpus gates уже входят в lib/tests job.
 Единая матрица implemented/validated/partial/
   unsupported с evidence на одной revision; CI, clean install, package exports/
   types и соответствие Rust/WASM/package. Обновить документацию. Commit/push/
   merge/release выполнять по отдельному запросу.

Подтверждённая база: Unicode17 grapheme corpus — 766 vectors; normalized Rust
word actions и WASM Node23/23; actual macOS GPU/CPU word-edit smoke. Shared GPU
scope service использует native completion signals без blocking backend wait;
ownership/capture/pop/cancellation и targeted lost-wakeup окна проверены.
Полная подробная очередь и исторические ограничения сохранены ниже.

### Последняя точка продолжения: scope overlays — 2026-11-03

Реализованы shared evaluation scopes для `For`/`Let`, захваченных `__locals`
и read-only binding replay. Обычная модель сохраняет flat COW и value semantics.
Этап **частично квалифицирован**: focused native/browser gates завершены;
длительные RSS/lifecycle, latency и широкая platform matrix ещё открыты.
[Итоговый отчёт](benchmarks/property-overlay-qualification-2026-11-03.json).

Подтверждено на текущем этапе:

- [x] Rust regression/allocation gates; exact WASM сравнения: 426 кадров и
  50 artifact JSON, Node 14/14. Chromium JSON подтверждает 19/19;
  визуальная проверка и screenshot сохранены.
- [x] Для 96 controls с payload1024 requested bytes: direct 3,291,035;
  lexical 3,613,180; shadow 3,526,712; captured 3,503,081 — все ниже 5 MB.
  Owned materialization masked scope: 7,956 bytes.
- [x] На 256 controls artifact retained allocator bytes 5,104,326→961,772;
  initial live heap Runtime 6,823,252→2,681,002; после Drop дельта нулевая.
  Это allocator accounting, не process RSS и не доказательство отсутствия утечек.
- [x] A/A + 4 AB/BA пары: payload1024 edit ratios 0.413–0.533;
  без payload на 8/32 controls наблюдается overhead порядка 1–5%,
  на 96/256 — улучшение. Универсальное ускорение не заявляется.

Ближайшие действия, до следующей оптимизации:

1. [x] Завершить browser smoke: прочитать состояние уже открытого probe,
   визуально проверить результат, сохранить screenshot и закрыть probe/server.
   JSON уже сохранён в `benchmarks/property-overlay-browser-2026-11-03.json`.
2. [x] Запустить **свежий** native256 probe текущей сборки: GPU→CPU→GPU,
   Unicode edit первой/последней строки, scroll до конца и обратно, exact model,
   сохранность остальных rows, отсутствие GPU errors. Сохранить новые logs и
   screenshots; проверить `vector-ui/scripts/qualify-native-form.py`.
   Старый native smoke до overlays не закрывает этот пункт.
3. [x] Перенести финальные raw results из `/tmp` в `packages/svet/benchmarks`,
   сохранить source/binary/WASM hashes и исходники baseline/candidate.
   Финальные пары: `/tmp/svet-property-overlay-edit-final-pairs/` и
   `/tmp/svet-property-overlay-payload-final-pairs/`; retention:
   `/tmp/svet-property-overlay-{baseline,candidate-final}-{artifact,runtime}.json`.
   Остальные logs: `/tmp/svet-property-overlay-*`; diagnostic iterations
   хранить отдельно от final qualification. Временные файлы могут исчезнуть.
4. [x] Обновить `SNAPSHOT_STORAGE.md`, `DEVELOPMENT.md`, `vector-ui/README.md`
   и qualification JSON. Указать materialization boundaries, small-form overhead,
   ограничения allocator/OS evidence и Rust migration: явно указанный
   `btree_map::Iter` заменить на `PropertyIter`/generic iterator.
   JSON schema не изменена; `Value::List` всё ещё owned Vec.
5. [x] Проверить scoped diff и согласованность итогового evidence. После этого
   продолжить общую очередь ниже: оставшиеся list/root copies, output/XRGB
   buffers и reuse, длительные native lifecycle/RSS и платформенные gates.

### Сохранённая точка продолжения — 2026-11-03

Предыдущий завершённый этап — region-aware sparse CPU raster scratch.
Проверено: 247 Rust tests, Node14/14, Chromium17/17, 150 точных сравнений
кадров и 30 artifact JSON. На focused 2x requested allocations bytes за кадр
снижены 8.70→1.12 MB; парное instrumented release время — ratio0.868.
Это не измерение OS input latency или RSS.
[Отчёт и ограничения](benchmarks/region-raster-qualification-2026-11-03.json).
Whole-map COW и static emoji уже внедрены; CJK остаётся variable.

Дополнительно убран пустой RGBA scratch у geometry-only dense/sparse controls:
800×600 cold content_pixels requested bytes6,720,064→4,800,064. Exact pixel
regression на четырёх масштабах; native/WASM compile checks пройдены.
[Evidence](benchmarks/empty-content-qualification-2026-11-03.json).
Browser/OS-window и latency/RSS этого изменения ещё не квалифицированы.

Последний этап: content-only support region отделён от geometry region для
обоих CPU путей. Dense nested clips800×600:10,561,888→4,805,356 requested bytes
(−54.5%). 266 Rust tests,240 exact native storage comparisons,270 exact WASM
frames и Chromium18/18 пройдены; explicit Metal Reveal126 comparisons,max1/255.
Dense geometry/frame/output storage, buffer reuse, physical-window/RSS и
latency всё ещё открыты.
[Evidence](benchmarks/dense-content-qualification-2026-11-03.json).

Dense coverage теперь тоже упакован в global row spans; paint разделён на
base/edge/content/reveal и финальный frame/background pass. Requested bytes
cold800×600:4,805,356→4,355,408. A/A+4AB/BA release public content_pixels при2x:
малый geometry repaint2.699→1.187ms (ratio0.439),full-canvas3.178→1.782ms
(ratio0.561);cold ratios0.820–0.868. Это включает RGBA copy, без OS presentation.
266 Rust tests,480 exact WASM frames,Node14/14,Chromium18/18 и explicit Metal
Reveal прошли. Frame/output buffers,NativeHost latency/RSS и buffer reuse
остаются открытыми.
[Evidence](benchmarks/packed-dense-qualification-2026-11-03.json).

Actual macOS NativeHost matrix8/32/96/256 editors пройдена: GPU→CPU→GPU,
редактирование первой/последней строки, Scroll до конца и обратно, exact bound
model/unrelated rows сохранены;0 GPU errors. На256 ещё3 быстрых roundtrip.
256-row observed present interval p50:CPU17.123ms,GPU8.712ms; process peak RSS
171,851,776bytes. Один instrumented release run/size, без baseline,A/A,
input-to-display latency, GPU completion или leak-proof claim.
[Raw journals/verifier/evidence](benchmarks/native-large-qualification-2026-11-03.json).
Длительные scroll/lifecycle/RSS plateau, другие schedulers/platforms и true IME
по-прежнему открыты.

Frame metadata теперь хранит equal-value runs вместо canvas byte map.
Uniform1600×1200:один run,capacity≤64bytes вместо1.92MB; cold800×600 requested
bytes4,355,408→3,875,440. Background=true A/A+4AB/BA при2x:small geometry
repaint2.796→1.293ms,full7.491→5.648ms (A/A0.970–1.014). Content-only timing
оставлен exploratory: A/A0.670–1.256,qualified speedup не заявляется.
267 Rust tests,480 exact WASM frames,Node14/14,Chromium18/18,Metal Reveal и
actual updated256-editor GPU→CPU→GPU/scroll/model smoke пройдены.
Output/XRGB buffers,стоимость построения frame runs и длинные RSS/lifecycle
проверки остаются открытыми.
[Evidence](benchmarks/frame-runs-qualification-2026-11-03.json).

Frame runs теперь строятся поиском equal-value prefixes на монотонных
полупрофилях rounded Frame,разделённых exact viewport/scrollbar boundaries.
Uniform1600×1200:24,000 scalar samples вместо1,920,000; exact scalar oracle
прошёл240 configurations +3 fallback cases. Small/invalid параметры оставляют
scalar fallback. Background=true A/A+4AB/BA при2x:small geometry cold build
6.865→1.506ms,full12.560→7.187ms;repaint в пределах разброса.
268 Rust tests,480 exact WASM frames,Node14/14,Chromium18/18,Metal Reveal и
actual updated256-editor GPU→CPU→GPU/scroll/model smoke прошли.
[Evidence](benchmarks/frame-spans-qualification-2026-11-03.json).
Следом: output/XRGB и buffer reuse,For/local overlays,list detachment,длительные
native RSS/lifecycle plateau;широкие input/platform gates остаются открытыми.

Rust CPU RGBA output теперь можно писать в caller-owned buffer через
`Runtime::paint_rgba`/`Button::paint_rgba`. Warmed single/multi-control matrix:
0 allocations против1 output Vec (211,200bytes в fixture); exact full-canvas
oracle и cache release/rebuild прошли. 192 lib +17 allocation tests,1 ignored;
native all-targets и WASM compile checks пройдены. Это отдельный opt-in Rust API:
owned JSON/WASM output, retained RGBA/XRGB buffers, latency/RSS ещё открыты.
[Evidence](benchmarks/rgba-output-qualification-2026-11-03.json).

CPU allocator lifecycle gate добавлен: оба schedulers ×8/32/96/256 controls,
по3 измеренных lifecycle после прогрева,32 edits/frames каждый (768 total).
RGBA/XRGB и exact model/unrelated rows проверены; cache release/rebuild каждые4
правки. Во всех24 lifecycles after-Drop delta0, live bytes на edits16–31
постоянны и retained bytes совпадают между повторами. Svet256:4,132,937 retained,
6,469,927 peak allocator bytes, включая caller buffers. Это bounded synthetic
allocator plateau; длительный OS-window RSS, GPU memory и input latency открыты.
[Raw/summary](benchmarks/cpu-lifecycle-qualification-2026-11-03.json).

GPU queue identity исправлена: разные logical Device на одном Metal adapter
больше не сравнивают Queue равными; clones сохраняют identity после Drop Device.
Hardware regression воспроизводил ошибку до fix и прошёл после; gpu-runtime5
unit tests, native218 и actual Metal renderer smoke прошли. Backend Metal
queue/resources остаются shared. Async command-error forwarding требует
сохранения исходного logical owner, включая deferred pure-compute batches;
ошибки нельзя направлять произвольному Device, вызвавшему poll.
[Evidence](benchmarks/queue-identity-qualification-2026-11-03.json).

Metal command completion forwarding реализован: исходные logical error sinks
сохраняются при batching/pure compute, general/timed commands и staged buffer
writes. Callback выполняется вне pending-command lock; reentry и poll другого
Device проверены. Реальные clear/pure-compute submissions сохраняют owner;
synthetic completion failure проверяет routing/scopes/no duplicates.
Runtime5 unit +3 explicit hardware tests,native218 и Metal renderer smoke прошли.
Остальные failures пока `Internal`: OOM-specific classification, реальные faults,
idle notifications и performance cost tracking остаются открытыми.
[Evidence](benchmarks/command-error-qualification-2026-11-03.json).

Metal OOM classification добавлена по системному SDK: code8 только в exported
MTLCommandBufferErrorDomain доставляется как OutOfMemory. Foreign-domain code8
остаётся Internal; device-loss codes тоже проверяют domain. Unit matrix6 passed,
3 explicit Metal tests и native218 passed; OOM scope capture/pop, foreign-domain
routing/reentry/no duplicates подтверждены synthetic completions на реальном
adapter. Физический OOM не создавался; NSError forced-fault path, idle wakeups
и performance/platform qualification ещё открыты.
[Evidence](benchmarks/command-oom-qualification-2026-11-03.json).

NativeHost теперь обслуживает completions после последнего GPU frame без
нового redraw: health deadline16ms действует, пока queue work pending, затем
снимается. Occlusion блокирует painting, сохраняя completion service. Fault
переводит host на CPU fallback. Native218 tests,all-targets и explicit Metal
health-without-draw/frame-counter gate прошли. **Следующий шаг:** actual
OS-window idle fallback/occlusion smoke новой сборки. Полностью idle device
removal после остановки polling по-прежнему требует native notifications;
energy/performance и другие backends ещё не квалифицированы.
[Evidence](benchmarks/idle-completion-qualification-2026-11-03.json).

Actual macOS idle fallback smoke пройден на8 editors: F9 в test-hooks build
инъецирует logical loss непосредственно в about_to_wait; журнал подтверждает
`Idle GPU completion`, переход на CPU без последующих GPU presents и exact
Unicode edit первой строки при сохранности остальных7 rows. Native normal и
hook builds:218 tests каждый,3 ignored. Screenshot/raw journal сохранены.
Первый draw-race diagnostic сохранён отдельно и не считается idle-path proof.
**Далее:** forced occlusion при pending completions и проверка остановки idle
wakeups в OS-window; physical faults/fully-idle removal notifications открыты.
[Evidence](benchmarks/idle-window-qualification-2026-11-03.json).

Actual idle completion cessation/occlusion проверены в обычной native build:
probe без caret focus,14 health checks;32.24s OS-occluded interval содержит
один завершающий idle check и0 presents, после восстановления GPU presents
возобновляются. Журнал живого процесса неизменен13.40s. Exact model сохранён.
Native218 +probe1 tests прошли; verifier отклоняет лишний check после idle.
Это не total event-loop wakeups/power gate; pending GPU execution непосредственно
в момент occlusion не доказано. Fully-idle removal notifications открыты.
[Evidence](benchmarks/idle-wakeup-qualification-2026-11-03.json).

ErrorScopeGuard ownership исправлен: pop удаляет scope по уникальному token,
вместо безусловного снятия верхней записи. Оба порядка pop, сохранность нового
независимого scope и8 concurrent pops проверены. Runtime8 unit,3 explicit Metal
и native218 tests passed. **Остаётся:** pending-completion-aware pop и submit-time
scope snapshots; нынешний pop сразу ready, GPU работу нужно предварительно poll.
[Evidence](benchmarks/scope-owner-qualification-2026-11-03.json).

Submit-time GPU scope snapshots и pending-aware pop реализованы. Queue submit/
write захватывают активные scopes; после pop они неактивны для новой работы,
но сохраняются captured leases до completions. Future Pending пробуждается
после последнего lease; отдельный completion worker не блокирует async caller.
Поздний scope не перехватывает старую ошибку; empty capture не создаёт snapshot.
Unit10,explicit Metal3,native218 и renderer smoke прошли. Открыты pooling/limits/
performance worker threads, physical faults и другие backends.
[Evidence](benchmarks/scope-capture-qualification-2026-11-03.json).

Scope completion service объединён на shared Metal backend: конкурентные
pending pops обслуживает один worker, вместо отдельного потока на каждый scope.
Weak registrations не владеют отменёнными futures; после drain worker выходит,
внутренняя registration queue capacity0. Два burst по64 scopes,32 cancellation
каждый:1 worker launch/burst,оставшиеся futures получают собственные ошибки.
Runtime10 unit +4 explicit Metal,native218 и renderer smoke прошли.
Driver-wait cancellation, latency/throughput/RSS/energy и другие backends открыты.
[Evidence](benchmarks/scope-service-qualification-2026-11-03.json).

Добавлен real-GPU release scope benchmark: nested1/8/32/64 scopes вокруг
1MiB clear, exact readback каждой операции; explicit blocking wait и async await.
Worker теперь reaps completions перед scan и не делает безусловный1ms sleep
после готовой работы. Runtime10+Metal4,native218,renderer/all-targets passed.
Performance gate **inconclusive**: во второй A/A+4AB/BA серии baseline control
ratios0.362–1.656. Async p50 observed~0.52–0.58→0.13ms — диагностическое
наблюдение, qualified speedup не заявляется. Обе серии сохранены; первая ещё
пересекалась со сборкой тестов. Следом: стабилизировать release controls и
расширить samples/warmup, затем latency/RSS/energy gate.
[Evidence](benchmarks/scope-backoff-qualification-2026-11-03.json).

Scope benchmark исправлен: async и explicit-wait mode используют разные
Metal backend/queue/buffer; старый shared backend позволял worker обслуживать
control command buffers. Новый protocol16 warmup+64 samples/group; сборки в
разных target dirs, SHA проверяется до запуска. Один пробный same-target run
отклонён из-за одинаковых binaries и сохранён с rejection marker.
Control A/A изолированной серии стабилизировался, но свежая confirmation
не прошла frozen pooled/order-stratified gates. Все samples/обе серии сохранены;
**performance остаётся inconclusive**, backoff speedup не квалифицирован.
Runner отклоняет identical binaries и не принимает shared-mode control.
Следом: исследовать scheduler/order effects и среду measurement; не менять
acceptance thresholds и не retry до случайного green.
[Evidence](benchmarks/scope-order-qualification-2026-11-03.json).

Pending scope-pop cancellation исправлена: Drop future сразу очищает waiter
и освобождает executor waker вне scope lock. Completion service перестаёт
обслуживать отменённые регистрации независимо от живых submission leases;
late error остаётся в captured state и не будит отменённую task. Unit regression
проверяет reentrant task destructor; Metal test:64 cancelled futures,64 leases
ещё живы,service уже retired. Runtime11 +Metal4,native218 и renderer smoke pass.
Already-entered driver wait_idle остаётся uncancellable; performance gate по
предыдущим сериям остаётся inconclusive.
[Evidence](benchmarks/scope-cancel-qualification-2026-11-03.json).

Async scope service больше не вызывает backend wait_idle: scoped Metal
commands регистрируют completion signal до commit; worker делает nonblocking
reap и паркуется на generation-counted Condvar. Cancellation и metadata
publication тоже будят worker, timeout fallback16ms. Обычные GPU frames без
captured scopes не создают новый block. Runtime11 +Metal4,native218 и renderer
smoke passed; native handler реально вызван, blocking wait counter0 в async
scope test. Physical hang/removal/OOM и performance/energy нового пути открыты;
старые benchmark серии его не квалифицируют.
[Evidence](benchmarks/scope-signal-qualification-2026-11-03.json).

Signal race gates добавлены: completion до park, после atomic unlock+park,
absence/timeout fallback и epoch wrap. Late signal после cancellation не
перезапускает retired service. Runtime14 unit +4 explicit Metal,native218 pass;
реальный native callback и blocking wait counter0 подтверждены. Это targeted
проверки, не exhaustive concurrency model checking и не physical fault proof.
[Evidence](benchmarks/scope-signal-races-qualification-2026-11-03.json).

Grapheme editing внедрён в native/WASM UI: Left/Right/Shift selection,
Backspace/Delete/insert и hit testing не делят extended clusters; binding
replacement выравнивает caret. Scalar TextEdit API/UTF-8 byte offsets сохранены.
220 native unit +4 integration +17 allocation tests, fresh WASM Node21 и actual
native GPU/CPU smoke passed. Combining/skin tone/ZWJ family/flag/keycap/Hangul/
Indic/CRLF проверены; full shaping/BiDi/emoji rendering и true IME всё ещё открыты.
[Evidence](benchmarks/grapheme-qualification-2026-11-03.json).

Unicode17 conformance gate добавлен: полный официальный GraphemeBreakTest,
766 vectors/1391 reference clusters, retained license/provenance. Проверены
forward/reverse/Shift selection, exact deletion/history, 2756 scalar cursor
positions и 1990 partial scalar replacements. Все4 conformance +4 existing
grapheme tests passed; production hashes совпадают с native/WASM snapshot.
Это finite logical-boundary proof, не shaping/BiDi/IME/browser input coverage.
[Evidence](benchmarks/grapheme-conformance-qualification-2026-11-03.json).

Word-boundary navigation/selection/delete добавлены в Rust model/Runtime и
native Option/Ctrl path. Stops — non-whitespace UAX29 segments, включая
punctuation/emoji, endpoints grapheme-safe; undo/bindings сохранены. Native220,
word4,grapheme4,conformance5,allocation17 tests и fresh WASM Node23 passed.
Actual macOS GPU/CPU Option shortcuts и untouched rows проверены. Windows/
Linux Ctrl/AltGr runtime и browser DOM modifier forwarding ещё открыты;
locale dictionary/full shaping/IME этим этапом не квалифицированы.
[Evidence](benchmarks/word-qualification-2026-11-03.json).

Оставшиеся задачи, в порядке продолжения:

1. [ ] **Большие формы и CPU raster.** Измерить оставшиеся dense output/native XRGB buffers и buffer reuse,
   разнесённый text/shape overflow и reuse scratch buffers. Проверить For/local
   overlays (реализованы; завершить квалификацию по checkpoint выше), list
   detachment, Runtime replacement и Let replay. На формах
   8/32/96/256 controls провести A/A и парные A/B: p50/p95 полного кадра и
   редактирования, allocations, live/peak и повторный RSS; отдельно actual
   OS-window scroll/culling и GPU presentation. Сохранять pixel equivalence.
2. [ ] **Native input и доступность.** Проверить настоящий CJK marked-text IME,
   cancellation/direct commit, смену поля/Runtime, copy/cut и latency;
   accessibility, DPI, несколько окон, nested scroll и physical touch.
3. [ ] **GPU lifecycle.** Добавить idle removal notifications; расширить forced-fault coverage
   доставляемых command errors (OOM classification реализована) и измерить стоимость ownership tracking; проверить initialization/surface faults,
   повторные resize/minimize/retry, ресурсы/RSS и recovery. Physical reset,
   adapter changes и другие backends ещё не квалифицированы. Synthetic loss
   и macOS Gpu→Cpu→Gpu уже проверены. Настоящий egui engine и переходы
   scheduler остаются отдельной задачей.
4. [ ] **Шрифты и текст.** Выбрать CJK instance/flattening policy после
   mixed-font caret, kerning, baseline/clipping и pixel/performance gates.
   Измерить startup/RSS/resolver cost и raw/gzip bytes. Full shaping,
   GSUB/GPOS, BiDi, ZWJ и colour emoji остаются открытыми.
5. [ ] **Lifecycle, транспорт и среды.** Объяснить исторические HTTP external
   memory failure и watcher update5 loss; расширить abort/error/backpressure
   и длительные plateau. Проверить peer copies, realms, другие browser engines,
   async SSR/Edge, router bursts и HMR side effects.
6. [ ] **Продуктовая квалификация.** Проверить clipping/depth/picking/AA и
   реальные приложения: startup, p95/frame time, long tasks, transfer,
   queue/compute/readback и память на сопоставимых нагрузках.
7. [ ] **Доставка.** На одном revision собрать Rust→WASM→installed package,
   проверить clean install, exports/types, native/browser/HTTP gates и remote
   CI. Обновить матрицу implemented/validated/partial/unsupported со ссылками
   на evidence. Commit, push, merge, публикация и очистка — отдельный этап;
   текущую работу и benchmark evidence сохранить.

Для закрытия задачи нужны воспроизводимая проверка, raw results и явно
указанные ограничения. Далее сохранены подробности и история предыдущих этапов.

### Подробности предыдущей точки продолжения — 2026-11-03

1. **Завершить текущую оптимизацию шрифтов.** Статические weight400 кандидаты
   созданы. Renderer теперь использует static emoji; CJK остаётся variable. Размер CJK
   17 772 300 → 10 595 844 bytes, emoji 1 982 596 → 886 652 bytes.
   Rust-сравнение всех glyphs: advances совпадают, topology mismatches нет;
   максимальная ошибка координат CJK 0.49981689453125 font units, emoji 0.
   [Сохранённый отчёт](benchmarks/font-instance-compare-2026-11-03.json).
   - [x] Повторить `vector-ui/scripts/instance-fallback-fonts.py` с FontTools
     4.68.0 после добавления проверки всех cmap/UVS; подтвердить сохранение
     таблиц и воспроизводимые SHA256 из `vector-ui/assets/fallback/STATIC_INSTANCES.json`.
     Повторная генерация подтвердила оба hash и все cmap/UVS; exhaustive Rust
     gate прошёл: topology unchanged, advances exact, coordinate error ≤0.5.
     [Evidence](benchmarks/font-instance-reproducibility-2026-11-03.json).
   - [x] Сравнить selected glyph CPU raster на 9/14/24/48/96px и четырёх
     subpixel offsets: emoji exact, CJK max alpha error34/255. Per-glyph
     errors и worst pixel сохранены в [отчёте](benchmarks/font-instance-raster-2026-11-03.json).
   - [ ] Объяснить округление CJK и сравнить raster output, baselines, caret и
     clipping; только после этого решить, переключать ли renderer на instances.
   - [x] Проверить higher-UPEM generation отдельно: 16000 не помещается в TTF
     int16 bounds; 8000 сохраняет cmap/glyph order, размер CJK13,487,632bytes.
     [Pixel evidence](benchmarks/font-instance-upem8000-2026-11-03.json).
     Полный outline gate: topology unchanged, max coordinate error0.0625
     исходной единицы; advances расходятся до0.5, gate отклонил кандидат.
     [Отчёт](benchmarks/font-instance-upem8000-outlines-2026-11-03.json).
   - [x] Сохранить original weight400 advances при high-UPEM instancing и
     повторить exhaustive gate, включая метрики и CPU raster comparison.
     Исправленный кандидат прошёл все glyphs: horizontal/vertical advances
     exact, CJK coordinate error0.0625, topology unchanged. Selected raster
     повтор пройден; [evidence](benchmarks/font-instance-upem8000-preserved-metrics-2026-11-03.json).
   - [x] Global metrics: ascender/descender/line gap/x-height/cap height exact
     после normalization. Edge-count raster diagnostics сохранены в
     [отчёте](benchmarks/font-instance-global-metrics-2026-11-03.json).
   - [ ] Проверить kerning, mixed-font caret и baseline/clipping
     на high-UPEM кандидате.
   - [x] Диагностический tolerance0.01px вместо0.15px уменьшил selected CJK
     max alpha error19→3/255; test-only override, production unchanged.
     [Отчёт](benchmarks/font-instance-fine-flattening-2026-11-03.json).
   - [x] Exploratory paired release CPU probe: tolerance0.01 стоит1.54–1.76×
     времени на14/48/96px;0.05 стоит1.11–1.21×. Один host/process, без A/A.
     [Raw/summary](benchmarks/font-flattening-cost-2026-11-03.json).
   - [ ] Сопоставить точность/стоимость flattening и выбрать production policy;
     не принимать размер шрифта ценой неконтролируемого pixel/performance сдвига.
   - [x] Подключить exact static emoji:240 Rust tests,Node14/14,90 exact
     WASM frames/caret comparisons. Payload−1,095,944bytes.
     [Evidence](benchmarks/static-emoji-qualification-2026-11-03.json).
   - [x] Browser/OS после static emoji: Chromium17/17; actual macOS окно
     показывает mixed paste и summary; raw model journal/screenshot сохранены.
     CJK IME composition этим не проверена.
   - [ ] При переключении повторить Rust/native, свежий WASM, Node и browser
     gates; измерить raw/gzip bytes, startup, RSS и resolver cost. Исходники
     шрифтов, лицензии и provenance сохранить. Full shaping/ZWJ/colour остаются
     отдельной работой.
2. **Закрыть native input в настоящей ОС.** CJK marked-text IME, отмена и
   direct commit после cancellation, переключение поля/Runtime, copy/cut,
   latency адаптеров; затем accessibility, DPI, несколько окон, nested Scroll
   и physical touch. Согласовать reusable NativeHost и standalone host.
3. **Убрать главную стоимость больших форм.** Shared-root/per-control snapshots,
   Runtime replacement и Let replay; затем release A/A и парные A/B на
   8/32/96/256 controls с p50/p95, allocations, live/peak и RSS.
4. **Довести lifecycle и транспорт.** Исторические HTTP external-memory отказ
   и потеря watcher update5 ещё не объяснены. Проверить transport/abort/error,
   длительные plateau, cross-core/peer-copy и отдельные realms, async SSR/Edge,
   router/HMR side effects и другие browser engines.
5. **Расширить аппаратные и продуктовые проверки.** OS GPU presentation,
   device loss/adapter change и другие backends; clipping/depth/picking/AA;
   startup/frame time/long tasks/память на реальных приложениях.
6. **Собрать единый проверяемый результат.** Один revision для Rust/WASM и
   installed package, clean install, exports/types, native/browser/HTTP gates,
   remote CI и актуальная матрица поддержки. Commit/push и очистка — после
   проверки сохранности всей работы; публикация требует отдельного запроса.

Критерий закрытия каждого пункта: воспроизводимая проверка, сохранённые raw
результаты и явно указанные ограничения. Исторические 239 Rust tests, Node14/14 и Chromium17/17 ниже относятся к
предыдущему renderer; актуальный результат указан в сохранённой точке
продолжения выше. CJK static candidates не квалифицированы для production.

### 1. Довести native input и восстановление фокуса

- [x] Проверить fallback при удалённом, disabled или сменившем тип редакторе:
  ввод поступает в доступное поле; remembered key очищается. Duplicate key
  отклоняется native compiler. 200 Rust tests passed, 2 GPU tests ignored.
  [Лог](benchmarks/native-refocus-boundaries-2026-11-03.txt).
- [x] Добавить load_ui_files_json_native с String errors и атомарным reload;
  WASM wrapper делегирует ему. Ошибки сохраняют документ/focus/версии, успешный
  reload сохраняет выбор scheduler. 201 Rust tests passed; wasm32 check passed.
  [Evidence](benchmarks/native-project-reload-qualification-2026-11-03.json).
- [x] Повторить project reload/error cases на свежем release WASM: Node8/8,
  Chromium11/11; отказ сохраняет model/focus/версии и CPU pixels, ввод после
  отказа и успешная замена работают.
  [Evidence](benchmarks/native-reload-wasm-qualification-2026-11-03.json).
- [x] Native source/artifact compilation API: load_component_native,
  load_native,compile_ui_scene_artifact_native возвращают String errors;
  JS wrappers делегируют. Failure/pixel/focus/version/recovery regression,
  229 Rust tests passed включая14 display-list,2 GPU ignored; wasm check passed.
  [Evidence](benchmarks/native-source-api-qualification-2026-11-03.json).
- [x] Fresh WASM JS adapters: Node12 binding cases +7 artifact checks,
  Chromium15/15. Source/template/project отказы бросают JS errors, сохраняют
  live scene count/layout/geometry; valid UI reload после отказа работает.
  [Evidence](benchmarks/source-api-wasm-qualification-2026-11-03.json).
- [ ] Другие JS-facing entry points проверять отдельно перед использованием
  native host; расширить browser engine matrix.
- [x] Завершать composition при замене Runtime внутри input callback даже с
  прежним key/index: native instance identity; regression fails before/passes
  after, 202 Rust tests passed. Idle allocation budgets сохранены.
  [Evidence](benchmarks/native-editor-session-qualification-2026-11-03.json).
- [x] Проверить замену Runtime после input callback внутри NativeApp.update:
  remembered composition owner сверяется перед OS IME update, новая scene
  завершает старую сессию. Wrapper regression и 203 Rust tests passed (2 ignored).
  [Лог](benchmarks/native-ime-post-callback-2026-11-03.txt).
- [x] Совместить сверку composition и чтение caret в одном NativeApp.update:
  regression проверяет call count, unchanged owner и retirement при sync error.
  Дополнительный update удалён; 203 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-ime-presentation-qualification-2026-11-03.json).
- [x] Устранить IME presentation через мутирующий update: обязательный
  NativeApp.inspect читает retained Runtime; regression подтверждает актуальный
  caret, отсутствие update и неизменные preedit pixels нового редактора.
  Native examples check и 203 Rust tests прошли. Rust API migration описана.
  [Evidence](benchmarks/native-inspect-qualification-2026-11-03.json).
- [x] Рисовать native frame через inspect после tick/update: текущий Runtime
  задаёт pixels и animation status; второй update на paint удалён. Exact pixel,
  call count/error regressions; 204 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-frame-inspection-qualification-2026-11-03.json).
- [x] Повторить actual window после inspect: ASCII ввод в оба поля через Tab
  и обновление frame/summary подтверждены. Pointer/navigation — partial:
  click второго поля даёт index2→1 перед вводом, символ попадает в первое.
  [Evidence](benchmarks/native-inspect-window-qualification-2026-11-03.json).
- [x] Воспроизвести blur→pointer selection→Focused(true): старый remembered
  editor перезаписывал новый выбор. Теперь current focus имеет приоритет.
  Regression fails before/passes after; 205 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-activation-focus-qualification-2026-11-03.json).
- [x] Повторить actual OS matrix: click второго→й только в second, Shift+Tab
  →я только в first, повторный click второго+w/Escape/v→цм только в second.
  Frame/summary/journal совпадают; использована текущая русская раскладка.
  Probe закрыт, owned processes0.
  [Evidence](benchmarks/native-activation-window-qualification-2026-11-03.json).
- [x] Проверять известного composition owner при dispatch commit: смена поля
  и замена Runtime с тем же key/index отклоняют старый commit; текущий owner
  и direct commit без preedit принимаются. 209 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-ime-commit-owner-qualification-2026-11-03.json).
- [x] Empty-preedit→presentation→commit сохраняет pending owner: marked
  pixels очищены, valid commit принят, replacement Runtime даже с тем же
  key/index не получает текст. 210 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-ime-empty-preedit-qualification-2026-11-03.json).
- [x] Добавить cancellation barrier: Escape/Tab/blur/pointer отменяют сессию,
  empty-preedit не снимает отмену, late/repeated commits отклонены. Новый
  непустой preedit или Enabled открывает ввод; Disabled блокирует commits.
  211 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-ime-cancelled-session-qualification-2026-11-03.json).
- [x] Добавить optional on_ime_event diagnostics и отдельный fixture-only
  *.ime.jsonl журнал в native probe. 211 Rust tests и example check прошли.
  Shortcut emoji picker в OS-попытке не открыл picker; вставка не засчитана.
  [Evidence](benchmarks/native-ime-diagnostics-2026-11-03.json).
- [x] Проверить текущий instrumented binary в OS: обычный русский input
  в оба поля работает; IME events0. Character Viewer process доступен, но
  CUA не получает окно (path/bundle id timeout). Emoji/CJK не засчитаны;
  этот путь активации не повторять без новой доступной UI surface.
  [Evidence](benchmarks/native-character-viewer-qualification-2026-11-03.json).
- [ ] Проверить actual CJK/emoji/direct-commit после cancellation: без нового
  Enabled/nonempty preedit commit сейчас отклоняется. Поздние события после
  старта новой сессии не имеют OS session id. IME latency также открыта.
- [ ] Пройти настоящую CJK/marked-text IME: commit/cancel, Tab/Escape, pointer
  switch, blur/refocus, caret/drag в том же поле и замена модели внутри callback.
- [ ] Проверить clipboard, accessibility, DPI, несколько окон и вложенные Scroll.
- [ ] Проверить backend transitions и согласовать поведение reusable NativeHost
  и отдельного host в `vector-ui/src/main.rs`.

Уже подтверждено: actual macOS NativeHost input и minimize/reopen; исправлено
восстановление последнего редактора по stable key. Reorder и повторный blur
покрыты регрессией, 199 Rust tests прошли. Это не проверка CJK или latency.
[Результаты](benchmarks/native-os-input-qualification-2026-11-03.json).

### Shared native clipboard — 2026-11-03

- [x] Подключить C/X/V в reusable NativeHost; standalone и reusable host
  используют общий Rust native_clipboard. Cut удаляет только после write
  success, read/write failures сохраняют text/selection; empty copy не
  очищает clipboard. Unicode/CRLF/undo/binding/disabled regressions с fake
  clipboard прошли:235 Rust tests,2 ignored; native all-targets check passed.
  [Evidence](benchmarks/native-clipboard-qualification-2026-11-03.json).
- [x] Исправить non-Latin command keys: ASCII logical сохраняется,
  physical A/C/X/V/Z/Y fallback применяется только к non-ASCII shortcut.
  Actual OS Russian layout: Cmd+A/paste Unicode, Undo/Redo после binding sync,
  Tab/paste second field прошли. 236 Rust tests,2 ignored,all-targets check.
  [Evidence](benchmarks/native-clipboard-layout-qualification-2026-11-03.json).
- [x] Исправить invisible missing scalars: fallback на NUL glyph давал
  width0 и no outline. Simple measure/hit/vector/CPU теперь используют ?;
  237 Rust tests,2 ignored. Actual native frame показывает ?? мир?, model
  сохраняет 你好 мир🙂, caret ширина согласована.
  [Evidence](benchmarks/unicode-fallback-qualification-2026-11-03.json).
- [x] Добавить unmodified NotoSansSC/NotoEmoji assets с upstream OFL, pinned
  revision и SHA256 provenance. ttf-parser weight400:11 sample scalars имеют
  положительные advances/nonempty contours. Fonts пока только в probe.
  [Evidence](benchmarks/font-assets-qualification-2026-11-03.json).
- [x] Подключить multi-face simple renderer Ubuntu→NotoSC→NotoEmoji:
  cached primary fast path, normalized advance, per-face outline scale/shared
  baseline. Actual native frame рисует 你好 мир🙂; mixed-font caret/vector
  regressions.238 Rust tests passed,2 ignored;wasm32 check passed.
  [Evidence](benchmarks/real-fonts-native-qualification-2026-11-03.json).
- [x] Shaped API использует реальные fallback faces: font_index +font_face,
  normalized advances и same-face-only kerning.239 Rust tests passed,2 ignored.
  Fresh WASM Node14/14,Chromium17/17; CJK/emoji pixels/caret проверены.
  Generated vector-ui WASM21,828,894bytes,gzip13,246,694bytes.
  [Evidence](benchmarks/shaped-real-fonts-qualification-2026-11-03.json).
- [ ] Проверить broader baselines/metrics, full CJK/emoji sequences/ZWJ, colour
  и full shaping отдельно. Измерить startup/RSS/resolver performance; решить
  deployment-size/lazy font strategy с сохранением coverage.
- [ ] Actual OS copy/cut, IME interaction и synchronous adapter latency
  ещё открыты; App.paste использовал временный clipboard с восстановлением.

### Modal root-scroll isolation — 2026-11-03

- [x] Root scroll заблокирован при активном Modal: baseline двигал фон
  offset160→140. Regression сохраняет offset/bounds и press диалога;
  после close root scroll работает. 231 Rust tests passed,2 ignored,
  fresh WASM Node13/13 и Chromium16/16.
  [Evidence](benchmarks/modal-scroll-qualification-2026-11-03.json).
- [ ] Actual OS wheel/touch и nested modal-local scroll отдельно не проверены.

### Native primary-touch ownership — 2026-11-03

- [x] Track gesture owner by device/id: secondary touch cannot steal pointer,
  focus or release; blur/cancel retires owner, orphan move/end ignored.
  Regression covers secondary phases, proper click, new gesture and late events.
  230 Rust tests passed including14 display-list,2 GPU ignored.
  [Evidence](benchmarks/native-touch-owner-qualification-2026-11-03.json).
- [x] Исправить потерю slow touch motion:80×0.25px раньше давали scroll0.
  Consumed coordinate обновляется после накопления >0.5px. Regression
  проверяет distance residual≤0.5, edge-pan без click, small jitter tap и
  следующий tap. 231 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-touch-slow-qualification-2026-11-03.json).
- [ ] Physical touchscreen/chooser touch qualification and multi-finger gesture
  design remain open; current policy adapts one primary touch to pointer.

### TextInput disabled — 2026-11-03

- [x] Исправить builtin schema: disabled:false объявлен, disabled:state.locked
  принимается compiler. Regression проверяет запрет focus/input, fallback,
  неизменность locked model, a11y actions и unlock через reload. 206 Rust tests
  passed,2 GPU ignored.
  [Evidence](benchmarks/native-disabled-input-qualification-2026-11-03.json).
- [x] Live lock/unlock через Toggle без reload: оба native schedulers, fresh
  release WASM Node9/9 и Chromium12/12. Значение сохраняется, preedit не
  коммитится, locked input/focus отклонены, unlock разрешает edit.
  207 Rust tests passed,2 ignored.
  [Evidence](benchmarks/live-disabled-qualification-2026-11-03.json).
- [x] Direct disable/enable во время pointer press + preedit: capture/focus/down
  очищены, late release не активирует control, повторный disable безопасен,
  edit после unlock без preedit commit. Refocus test использует disabled:true
  из .ui без runtime workaround. 208 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-disabled-session-qualification-2026-11-03.json).
- [x] Regression: disable во время Space press, late release после reenable
  не активирует original/другой control; новая пара press/release работает.
  Tab/Shift+Tab skip disabled; empty Modal не отдаёт focus фону, reenabled
  control снова trap обе стороны. Focused test passed, behavior fix не нужен.
  [Evidence](benchmarks/native-disabled-navigation-qualification-2026-11-03.json).
- [ ] OS accessibility, trusted DOM routes и disabled visual theme отдельно
  не квалифицированы.

### 2. Снизить стоимость native редактирования

- [x] Borrow fallback/definition defaults в scene_properties, merge только
  при missing-state evaluation.241 Rust tests passed; allocation before/after
  на одинаковой команде сохранены. Shared-root snapshots пока не устранены.
  [Evidence](benchmarks/native-defaults-qualification-2026-11-03.json).
- [x] Fresh release WASM после defaults borrowing:Node14/14,Chromium17/17,
  90 exact CPU frame/caret comparisons с предыдущим модулем. Отчёты сохранены.

- [x] Убрать state snapshots у layout-only узлов: 24 nested Column /256
  unrelated records /1 control — 40950→3024 allocations (-92.6%). Публичный
  snapshot control сохранён; regression budget10000. 212 Rust tests passed,
  2 GPU ignored. Это не общий edit-time latency/RSS результат.
  [Evidence](benchmarks/native-layout-snapshot-qualification-2026-11-03.json).
- [x] Fresh release WASM после layout optimisation: Node10/10,Chromium13/13;
  24 nested Column сохраняют public snapshot256 records и live write
  seed-nested; unrelated payload не изменён, CPU frame видим.
  [Evidence](benchmarks/layout-snapshot-wasm-qualification-2026-11-03.json).
- [x] Release binding edit/rebuild: baseline A/A +4 AB/BA pairs,12 nested
  levels,8/32/96/256 controls,оба schedulers. Svet256 allocations
  1799435→1764767 (-1.93%); per-control snapshots остаются dominant.
  Timing inconclusive: A/A до2.056×. Raw runs/builds/baseline source сохранены,
  runner vector-ui/scripts/run-layout-edit-pairs.py добавлен.
  [Evidence](benchmarks/layout-edit-paired-qualification-2026-11-03.json).
- [x] Убрать безусловный clone resolved props перед contracts validation:
  Cow заимствует props, копия нужна только для удаления missing required key.
  Custom List256:5059→4284 allocations (-15.3%); baseline budget4700 fails,
  candidate213 Rust tests passed,2 ignored. Public snapshots сохранены.
  [Evidence](benchmarks/native-contract-props-qualification-2026-11-03.json).
- [x] Fresh release WASM после contract-props optimisation: Node11/11,
  Chromium14/14; List256 loads, wrong type refused, state/layout revision
  сохранены после отказа.
  [Evidence](benchmarks/contract-props-wasm-qualification-2026-11-03.json).
- [x] Перемещать уже owned local control scope в snapshot после раскрытия
  детей: local payload256 allocations1878→1105 (-41.2%), budget1400
  fails before/passes after. Public local/inherited state сохранён.
  214 Rust tests passed,2 ignored.
  [Evidence](benchmarks/native-owned-state-qualification-2026-11-03.json).
- [x] Fresh release WASM после owned-scope transfer: Node11 binding cases
  +7 artifact checks passed. Rust regression дополнена unchanged Document.states
  и byte-identical repeated render;1105 allocations. `.ui` loader задаёт
  только root state, поэтому Node не доказывает node-local allocation выигрыш.
  [Evidence](benchmarks/owned-state-portability-qualification-2026-11-03.json).
- [x] Browser повтор после owned-scope transfer: Chromium15/15 на свежих bytes
  прошёл в source API gate. Это совместимость, не node-local perf evidence.
- [x] Sharing неизменяемого root-map:Properties теперь Arc map + copy-on-write;
  sibling storage sharing и snapshot mutation isolation подтверждены.243 Rust
  tests,all-targets,Node14/14,30 exact artifact JSON и90 exact frames прошли.
  96-editor repair allocations140045→41253 (-70.5%).
  [Evidence](benchmarks/shared-properties-qualification-2026-11-03.json).
- [x] Свежий Chromium17/17; A/A+4AB/BA release artifact-retention pairs.
  256 controls:retained allocator bytes135619610→5104326,after-Drop delta0.
  [Raw/summary](benchmarks/shared-properties-retention-pairs-2026-11-03/summary.json).
  Allocator timing exploratory; A/A noisy на малых формах.
- [x] Full Runtime allocator live/peak через initial load +3 binding edits/CPU
  frames,оба schedulers,8/32/96/256:after-Drop delta0. Process peak RSS
  baseline367951872→candidate28672000bytes (один fresh process на вариант).
  [Evidence](benchmarks/shared-properties-runtime-qualification-2026-11-03.json).
- [ ] Native OS-window/GPU и app frame/edit qualification; повторить RSS
  matrix и измерить оставшиеся For/local overlays/list detachment.
  Путь хранения и критерии миграции разобраны в [SNAPSHOT_STORAGE.md](SNAPSHOT_STORAGE.md).
  [Baseline](benchmarks/snapshot-storage-baseline-2026-11-03.json):8/32/96/256
  controls ×256 unrelated root records; probe сохраняет Document.states.
  Whole-map sharing реализован; mutable overlays требуют дальнейшей работы.
- [ ] Уменьшить per-control snapshots; отдельно измерить uninstrumented
  latency и RSS/live/peak на спокойном host.

- [ ] Уменьшить per-control model snapshots, edit-time Runtime replacement и
  повторный Let replay; расширить сценарии complex/nested aliases и derived
  writable collections, transactions/reorder/replacement.
- [ ] Провести release A/A и парные A/B для 8/32/96/256 controls: полный кадр,
  p50/p95, allocations, live/peak memory и Runtime RSS; сохранить hashes/raw data.
  Нулевые steady-idle allocations и снижение cold replay уже измерены;
  задержка пользовательского приложения остаётся непроверенной.

### 3. Закрыть lifecycle, память и транспорт

- [ ] Дополнить terminal facade/error/reentrancy/cross-core regressions и
  произвольные object/function payloads; отделить доказанные случаи от общих гарантий.
- [x] Усилить watcher probe: final disk hash/stat, canonical module cache
  hash/version/invalidation timestamps, browser websocket before/after/error
  trace. Два unpaced Chromium replay37/37,graph0; disk/cache/DOM=v9,
  59 workspace/installed compiled files совпали.
  [Evidence](benchmarks/watcher-diagnostics-qualification-2026-11-03.json).
- [ ] Разобрать исторический HTTP external-memory отказ и потерю watcher update5;
  расширить transport/abort/error matrix и длительные plateau проверки.
- [ ] Повторить peer-copy browser GC, отдельные realms и другие browser engines.
  Текущие Chromium natural-GC и HMR проверки уже пройдены; они не покрывают все среды.
- [ ] Проверить async SSR/Edge carriers, router bursts и произвольные HMR side effects.

### 4. Расширить renderer и продуктовые измерения

- [x] CPU self-animation через60Hz event-loop deadline, input-redraw immediate;
 245 native tests/all-targets passed. Actual release CPU median17.058ms,
 GPU return8.3385ms; binding edits survive presentation switch.
 [Evidence](benchmarks/native-host-cpu-pacing-qualification-2026-11-03.json).
- [x] Region-aware sparse leaf scratch:global contours/clip/AA arithmetic
 preserved;region only changes storage indexing.247 Rust tests,Node14/14,
 Chromium17/17,150 exact frames/30 artifact JSON comparisons passed.
 Budget1600x1200:17.69→1.12MB (baseline fails,candidate passes).
 A/A+4AB/BA at2x:8.70→1.12MB/frame,median time ratio0.868.
 [Evidence](benchmarks/region-raster-qualification-2026-11-03.json).
- [ ] Dense/single-control path, widely separated overflow,buffer reuse and
 full physical-window/RSS frame qualification remain open.

- [x] Culling wholly offscreen sparse leaves до canvas allocations, с учётом
  text/shape overflow.244 Rust tests; scroll-reentry и exact overflow pixels.
  256-control first CPU frame requested bytes278224356→13553796 (-95.1%),
  allocations1453→228; Runtime Drop delta0,lifetime peak unchanged.
  [Evidence](benchmarks/offscreen-cache-qualification-2026-11-03.json).
- [x] Fresh WASM после culling:Node14/14,Chromium17/17,90 exact frames,
  60 sparse overflow/DPI frames и30 exact artifact JSON comparisons.
- [x] Повторены2 hardware offscreen tests:Apple M4 Max/Metal,Reveal126
  comparisons,max error1/255. Actual CPU NativeHost two-field paste/ShiftTab
  model isolation и summary подтверждены; probe закрыт.
  [Evidence](benchmarks/offscreen-cache-native-hardware-qualification-2026-11-03.json).
- [x] Actual macOS OS scroll/top-bottom + CPU/GPU presentation на8/32/96/256
  bound editors; last-row edit/reentry и unrelated model rows сохранены.
  Один instrumented release run/size, не полная latency/RSS квалификация.
  [Evidence](benchmarks/native-large-qualification-2026-11-03.json).
- [ ] Длительные scroll/culling/lifecycle и RSS plateau; повторить на других
  schedulers/platforms и более сложном content overflow.
- [x] Shared native_gpu surface module extracted from standalone host;
  reusable NativeHost desktop/Svet presents GPU frames. Actual macOS
  Gpu→Cpu→Gpu sequence recorded; bindings/summary survive F7 roundtrip.
  `egui` remains CPU compatibility presentation; scheduler independently owned.
- [x] Guard early redraw during GPU retry deadline and OS occlusion;245 Rust
  tests/all-targets passed. Actual macOS minimize emitted Occluded true→false,
  579.415ms:0 presented frames during hidden interval,GPU resumes,model preserved.
  [Evidence](benchmarks/native-host-occlusion-qualification-2026-11-03.json).
- [x] Uncaptured validation delivery в собственном gpu-runtime:Device и
  CommandEncoder callbacks реально подключены,scope capture/reentry сохранены.
  Renderer health-check до configure/acquire; первый fault сохраняется,
  дальнейшие draw не submit/upload.2 runtime tests,245 native,2 hardware gates.
  [Evidence](benchmarks/gpu-error-delivery-qualification-2026-11-03.json).
- [x] Device-loss callback/poll delivery реализован:once,late registration,
  reentry; Metal completion tracks AccessRevoked/DeviceRemoved (SDK4/11),
  pending buffers сохраняются до completion и очищаются при submit/poll/wait.
  5 runtime tests +Metal backend-state injection,245 native и2 hardware gates.
  [Evidence](benchmarks/device-loss-delivery-qualification-2026-11-03.json).
- [ ] Physical device removal/reset не проверен. Добавить idle device-removal
  notifications,other-command error reporting и OS fallback под injected loss.
- [x] Actual macOS GPU-window backend-loss injection:Gpu→Cpu→Gpu,1
  reported fallback error,model preserved,input works on CPU and recovered GPU.
  Opt-in native-test-hooks/F8 absent from default native feature tree;
  245 tests pass in each configuration.
  [Evidence](benchmarks/native-host-loss-fallback-qualification-2026-11-03.json).
- [ ] Physical removal/reset и automatic recovery; GPU initialization/surface
  fault injection,repeated resize/minimize/retry,large-form scrolling,other backends.
- [ ] Actual egui engine integration and explicit app scheduler transitions
  remain separate from presentation profile selection.

- [ ] Проверить GPU device loss/смену адаптера, другие physical backends,
  rounded AA, projection/picking/clipping/depth и resident graph throughput.
  Offscreen Metal и OS GPU presentation на macOS проверены; другие платформы открыты.
- [ ] Измерить реальные приложения: startup, p95/frame time, long tasks,
  transfer bytes, queue/compute/readback и память на одинаковых нагрузках.

### 5. Подготовить проверяемую доставку

- [ ] На едином revision собрать Rust → WASM → installed package; проверить
  provenance, clean install, exports/types и согласованные native/browser/HTTP gates.
- [ ] Получить remote CI и обновить матрицу implemented / validated / partial /
  unsupported со ссылками на воспроизводимые проверки.
- [ ] Проверить Git/worktree состояние перед commit/push; выпуск и публикация —
  отдельный этап по запросу. Сохранённые evidence не удалять как build cache.

## Следующий этап — сохранённый план на 2026-11-02

Этот список задаёт актуальную очередь. Подробная история ниже сохраняется как
свидетельства отдельных проверок; старые открытые формулировки нужно сверять
с этим разделом. Полная готовность Svet не заявляется.

### Точка продолжения — retained Forma handles (2026-11-02)

Разработка остановлена на промежуточной правке public Forma facade в
`src/engine/forma.ts` и cached-value metadata в `src/engine/reactivity.ts`.
Правка ещё не прошла полную квалификацию; следующие задачи выполнять по порядку.

Подтверждено: до правки retained disposed instance, mount handle и bound render
удерживали по 4 cores и 4 WASM Memory; detached buttons уже освобождали их.
После правки Node WeakRef-проба показывает 0 retained cores/Memory во всех
четырёх режимах (16 экземпляров), с положительными GC controls.
[До](benchmarks/forma-retained-handles-before-2026-11-02.json),
[после](benchmarks/forma-retained-handles-after-2026-11-02.json).
Focused suite 160/160, TypeScript и package build прошли. Это не fresh installed
package gate и не browser qualification новой правки; прежние full1492/111 и
browser37/37 относятся к предыдущим runtime bytes.

- [x] Сохранить совместимость `instance.actions`: регрессия воспроизвела обход
  override при клике. Action slots теперь поддерживают override, обёртку вокруг
  original без рекурсии и восстановление original; slots очищаются при dispose.
  Новые cached-read/inert-write регрессии и full1494/112 files прошли; types,
  build, повторная Node GC-проба и fresh installed gate также прошли.
  Installed runtime/probe hashes совпали с проверенной сборкой.
  [Квалификация](benchmarks/forma-public-actions-qualification-2026-11-02.json).
- [ ] Добавить регрессии terminal facade: cached state/props/computed/locale,
  export snapshot после dispose, inert writes/actions, idempotent mount/event
  cleanup, ошибки cleanup и отсутствие вычисления unused lazy computed при dispose.
- [x] Определить fallback для Signal без `__svet_cached_value`: cached
  `Signal.peek` сохраняет внешнее значение. До правки regression получала
  undefined вместо7; после focused17/17 и types прошли. Computed.peek не
  вызывается при disposal; unused lazy computed остаётся unevaluated.
  Foreign computed без metadata не имеет доступного safe cache protocol;
  его terminal value остаётся undefined. Full1496/112 files, build и fresh
  installed-package gate прошли; retained handles cores/Memory0 во всех режимах.
  [Квалификация](benchmarks/forma-terminal-cache-qualification-2026-11-02.json).
- [ ] Проверить отдельно захваченные Signal/Computed, action callbacks и root
  disposers: public facade не доказывает освобождение всех raw SDK handles.
  Проверить также произвольные пользовательские object/function values.
- [ ] Включить retained-handles probe в installed-package gate, сохранить
  точные hashes и controls. При изменении probe повторить baseline тем же скриптом.
- [ ] Последовательно выполнить focused → types → build → полный suite → fresh
  installed-package gate; не пересобирать dist параллельно JS-тестам.
- [ ] Повторить Chromium HMR и natural-GC cohorts на свежем установленном пакете,
  удерживая instance/mount handle/bound render/button после disposal; проверить
  late calls, controls и graph0. Обновить browser matrix и qualification reports.
- [ ] После этого продолжить P1–P4 ниже. Полная готовность, remote CI, выпуск и
  публикация остаются открытыми; текущие исходники и evidence не закоммичены.

### Точка продолжения — raw reactive handles (2026-11-02)

- [x] Воспроизвести удержание отдельными state/prop/computed/peek references:
  по4 cores и4 Memory в каждом режиме; action/unsubscribe уже0.
  [Baseline](benchmarks/forma-raw-handles-before-2026-11-02.json).
- [x] После dispose отсоединять Signal core/context/subscriber/comparator;
  откладывать detach до завершения активных writes. Computed освобождает
  core/context/owner/body/dependencies после выхода из active observer frame.
  Первая full-проверка нашла2 регрессии premature detach при effect/reset;
  после active-write guard focused117/117 и types прошли.
- [x] Завершить повторные full/build/fresh installed-package gates:
  full1500/113 files, types, build и fresh installed package прошли.
  Raw-handle probe включена в gate:24 strongly held handles,48 core/Memory
  targets, controls4→0, graph0. Final installed GC прошёл все6 режимов,
  retained0, probe hash совпал с baseline.
  [Квалификация](benchmarks/reactive-handle-detach-qualification-2026-11-02.json).
- [ ] Проверить retained root disposers, user comparator/body payloads, reset
  без explicit disposal и browser natural GC; измерить hot-path стоимость guards.
  Произвольные cached user values могут сами удерживать context.

### Точка продолжения — retained root disposers (2026-11-02)

- [x] Воспроизвести удержание root disposer: explicit disposal, construction
  throw, dispose внутри construction и throwing cleanup — по4 cores/Memory
  во всех4 режимах. [Baseline](benchmarks/root-disposer-before-2026-11-02.json).
- [x] Добавить terminal root cell, отсоединяемый scope teardown. Служебный
  учёт находится в WeakMap, чтобы не менять `owner.disposables` registrations.
  Первая full-проверка с cleanup registration нашла3 несовместимости счётчиков;
  после переноса focused173/173 и types прошли. Добавлены регрессии reentrant
  idempotent cleanup и escaped disposer после construction failure.
- [x] Повторные full1502/113 files, types, build и fresh installed-package
  gates прошли; root probe подключена к installed gate. Она держит16 disposers,
  все32 core/Memory targets collected, controls4→0, live-root controls2→0;
  живой root сохраняет strong core. Probe hash совпал с baseline.
  [Квалификация](benchmarks/root-disposer-qualification-2026-11-02.json).
- [ ] Повторить browser natural GC/HMR на окончательных runtime bytes и
  измерить стоимость Signal write guards/root lifecycle bookkeeping.

### Browser qualification текущих terminal handles (2026-11-02)

- [x] Chromium156 natural GC на установленном runtime после public/raw/root
  fixes:3 fresh page cohorts,360 collected targets,132 strongly held terminal
  handles,264 late calls, controls30 held→collected, forcedGC0, final graph0.
  Покрыты11 retained modes плюс4 прежних lifecycle modes и unknown extensions.
  Все20 runtime hashes совпали с workspace dist/package.
  [GC qualification](benchmarks/forma-terminal-browser-gc-qualification-2026-11-02.json).
- [x] На тех же bytes повторить real Vite HMR:37/37 (35 automated + trusted
  input/cleanup), graph0;60 hashes workspace/installed/peer совпали.
  [HMR qualification](benchmarks/forma-terminal-browser-hmr-qualification-2026-11-02.json).
- [ ] Измерить стоимость Signal active-write guards, root cells и terminal
  snapshots; проверить reset-only retention, other engines и long-run plateau.
  Эти bounded GC cohorts не доказывают heap/RSS plateau или полную leak freedom.
  Исторический watcher update5 loss остаётся необъяснённым.

### Точка продолжения — reset-only retention (2026-11-02)

- [x] Воспроизвести удержание Signal/Computed/getter/peek после reset без
  explicit disposal: local и compatible peer reset, по4 cores/Memory во всех
  восьми режимах. Computed создан в независимом root, чтобы async request teardown
  не скрывал reset-only retention.
- [x] Reset теперь использует private retirement callbacks, захваченные из
  старой generation до cleanup/reentrant allocations. Они не зависят от
  подменённого public dispose; active writes/observer frames отсоединяются
  после unwinding. WeakMap registry находится в shared reactive domain.
  Module-local промежуточная версия проходила local GC, но удерживала peer
  handles; shared registry проходит все8 режимов local probe.
- [x] Повторные full1503/113 files, types, build и fresh installed-package
  gates прошли. Shared registry:32 held handles, все64 core/Memory targets
  collected, controls4→0, graph0, baseline/final probe hashes совпали.
  [Квалификация](benchmarks/reset-handle-retention-qualification-2026-11-02.json).
- [ ] Повторить browser GC/HMR на новых bytes; проверить reset-only effects,
  roots, user callback payloads, incompatible copies/realms и стоимость reset.
  Browser reports предыдущего этапа относятся к runtime до этой reset-правки.

### Точка продолжения — public effect lifetime (2026-11-02)

- [x] Воспроизвести удержание disposed EffectHandle и отдельно сохранённого
  dispose method: по4 cores/Memory в local и peer-reset modes.
- [x] Выделить terminal public effect cell; перед user cleanup отсоединять
  core, parent owner и pooled handle. Reentry/throw не возвращают старую ссылку
  на reused entry. Focused85/85, types, build и full1505/113 files прошли.
- [x] Fresh installed-package gate прошёл:16 retained effect handles/methods,
  все32 core/Memory targets collected, controls4→0, graph0; same baseline probe hash.
  [Квалификация](benchmarks/disposed-effect-retention-qualification-2026-11-02.json).
- [x] Проверить reset-only root контракт: reset очищает graph, но root cleanup
  остаётся deferred до owner disposal. Retained disposer держит живой context;
  после late dispose все root core/Memory collected, repeated dispose inert.
  Local/peer диагностический [отчёт](benchmarks/effect-root-lifetime-after-2026-11-02.json).
- [ ] Проверить отдельно reset-only effect cleanup и capture payloads, browser
  GC/HMR новых bytes, lifetime guard/reset performance и остальные P1–P4 задачи.

### Точка продолжения — reset-only effect payloads (2026-11-02)

- [x] Разделить obsolete body payload и pending cleanup payload в GC-пробе.
  До правки retained body4/cleanup4 для EffectHandle и сохранённого dispose
  в local/peer reset modes; после reset body уже не может запускаться.
- [x] Shared reset retirement теперь очищает только effect userFn, сохраняя
  ранее зарегистрированный cleanup до owner disposal. Local GC:body0/cleanup4;
  после late dispose core/Memory/body/cleanup0 во всех4 режимах.
- [x] Проверить reset внутри active effect: pre-reset cleanup deferred,
  late register/returned cleanup immediately drained, late dispose exactly once
  и не повреждает fresh nodes. Focused92/92, types и build прошли.
- [x] Fresh installed gate прошёл:16 held handles,16 body payloads collected
  после reset,16 pending cleanup payloads плюс32 core/Memory held до disposal;
  затем все collected, cleanupCalls16, controls4→0, same probe hash.
  Full:1505 passed +1 benchmark timeout5000ms при host load≈99; отдельный
  benchmark retry с timeout30000ms прошёл за2.95s. Один полностью зелёный full
  run этой версии ещё не зафиксирован.
  [Квалификация](benchmarks/reset-effect-payload-qualification-2026-11-02.json).
- [ ] Browser GC/HMR новых bytes, performance/reset cost и широкая P1–P4 очередь
  остаются открытыми. Generic reset не заменяет owner teardown.

### Точка продолжения — lifecycle cost (2026-11-02)

- [x] Добавить paired diagnostic:54 isolated Node workers,3 experiments,
  balanced orders,2592 timed samples; writes, observed writes, effect churn,
  reset512 signal/computed pairs. Сохранены3 runtime archives и20 hashes каждого.
  Baseline→retirement reset ratio≈5.44, absolute baseline0.08–0.10ms против
  retirement0.35–0.55ms в первом прогоне. Write/effect overhead inconclusive.
- [x] Убрать large intermediate reset arrays и terminal computed closure/deps
  allocations. Focused92/92, types и build прошли. Два paired comparisons дали
  median reset ratios0.86/0.83, но A/A scatter0.57–1.92 и host load до128
  не позволяют заявлять speedup. Это уменьшение allocations по коду, а не
  закрытый performance gate.
- [x] Fresh installed-package/GC gate прошёл. Full:1491 tests/112 files
  passed, dom-lifecycle worker не стартовал (pool timeout); отдельный retry
  прошёл15/15. Все1506 assertions покрыты, single all-green full run ещё
  не зафиксирован. [Diagnostic evidence](benchmarks/reactive-lifecycle-cost-qualification-2026-11-02.json).
- [ ] Повторить A/A–A/B на спокойном host, измерить scaling512–20k nodes,
  owner subtree teardown, большие формы и application latency; current browser
  GC/HMR и прочие P1–P4 задачи остаются открытыми.

### Точка продолжения — shared owner reset scaling (2026-11-02)

- [x] Воспроизвести сверхлинейную стоимость reset при many computeds в одном
  owner:8192 interleaved/tail callbacks ≈10–12ms,2048 ≈0.7–0.9ms.
- [x] Shared retirement protocol пакетно собирает retired owner registrations;
  один compaction сохраняет identity callback array и порядок user cleanups.
  Reset старой копии без batching argument использует прежний безопасный путь.
  Регрессия проверяет interleaved cleanup ordering, cached reads и fresh slots.
- [x] Paired Node diagnostic:18 workers,972 timed samples,6 balanced rounds,
  host load10.9–11.5. На8192 interleaved/tail ratios≈0.114/0.122, все6 пар
  быстрее, A/A около1. На512/no-callback case есть overhead≈11%; улучшение
  не распространяется на все нагрузки и не заменяет application qualification.
- [x] Full1507/113 files (timeout30000ms), types, build и fresh
  installed-package/GC gates прошли; focused211/211 до последней регрессии.
  [Evidence](benchmarks/root-reset-scaling-qualification-2026-11-02.json).
- [ ] Current-byte browser GC/HMR, larger owner trees, other copies/realms,
  application/native large forms и оставшиеся P1–P4 задачи остаются открытыми.

### Current runtime browser gate — reset/owner batching (2026-11-02)

- [x] Расширить natural-GC fixture reset-only Signal/Computed/peek и pending
  effect/root payload controls.3 fresh Chromium156 cohorts:492 targets collected,
  216 strongly held terminal handles,138 controls held→collected,396 late calls,
  cleanupCalls36 ровно, forcedGC0, final graph0. Obsolete effect body releases
  после reset; cleanup/native context остаются до late dispose.
  Все20 runtime hashes совпали с dist/package.
  [GC evidence](benchmarks/reset-browser-gc-qualification-2026-11-02.json).
- [x] Real Vite HMR на тех же bytes:37/37 (35 automated + trusted input/cleanup),
  graph0,60 workspace/installed/peer artifact comparisons совпали.
  [HMR evidence](benchmarks/owner-reset-browser-hmr-qualification-2026-11-02.json).
- [ ] Peer-reset browser GC, other realms/engines, heap/RSS plateau и
  application/native large-form latency остаются открытыми. Historical watcher
  update5 loss не объяснён. Полный roadmap P1–P4 остаётся активным.

### P1 — надёжность и границы поддержки

- [ ] **HTTP memory:** разобрать завершённый natural-GC прогон текущего пакета
  `benchmarks/http-current-runtime-natural-2h-2026-11-02.json` отдельным анализатором
  с прежними бюджетами: heap 16 MiB, RSS 64 MiB, external 64 MiB,
  ArrayBuffers 16 MiB, минимум 7 200 000 ms. Анализ выполнен: все четыре
  бюджета пройдены; 22 current artifact hashes совпали, analyzer tests 10/10.
  [Квалификация](benchmarks/http-current-runtime-natural-2h-qualification-2026-11-02.json).
  Осталось объяснить исторический отказ и расширить transport matrix.
  Raw report: 7 200 013 ms, 1 655 800 requests, без request/cleanup failures,
  forced GC = 0, artifactsUnchanged = true. Его `status: passed` означает
  завершение нагрузки, а не прохождение memory budgets. Объяснить исторический
  external-memory отказ; прежний traced-прогон другой версии не закрывает его.
- [ ] **HMR:** admission теперь под update lock: nested HMR из getter
  отклоняется, liveness перепроверяется после snapshot. Lock снимается даже при
  throwing final computed cleanup; retired siblings draining сохраняет primary
  update error. Focused138/138, full1470/108 files, types и installed-package
  gate (AOT11 cases) прошли. Candidate cleanup failure теперь не прерывает
  rollback: old handles восстанавливаются, primary error сохраняется; disposal
  во время rollback останавливает restoration. Общий instance teardown draining
  и construction-time ViewModel ownership проверены: full1475/109 files, types,
  installed-package gate (AOT13 cases). State ownership теперь хранится по
  исходным allocated handles, отдельно от публичных aliases/replacements;
  candidate rollback освобождает исходный own state и сохраняет чужой alias.
  Focused147/147, full1479/109 files, types и installed-package gate прошли;
  Props/computed ownership теперь тоже отделён от public maps: previous owned
  let generation сохраняется отдельно для HMR commit/rollback, foreign handles
  не приобретаются;6 reproduced regressions, focused153/153, full1485/110 files,
  types и installed-package gate (AOT14 cases) прошли.
  Closing options getters/public-map setters теперь не создают поздние own
  handles; old owned generation не освобождается повторно. Rollback использует
  captured resolved props без повторного чтения options getters. Focused160/160,
  full1492/111 files, types и installed-package gate (AOT15 cases) прошли;
  Latest installed Chromium156 real Vite HMR прошёл37/37:35 automated +
  trusted input/cleanup, graph0,60 hashes workspace/installed/peer совпали.
  [Browser evidence](benchmarks/forma-current-browser-qualification-2026-11-02.json).
  Fixture отличает invalid transform refusal от module admission и ждёт hot
  websocket readiness; исторический watcher update5 loss этим не закрыт.
  Forma browser GC cohort теперь проверен3 раза в Chromium156:96 targets
  collected, controls30 held→30 collected после release, forcedGC0, graph0.
  [GC evidence](benchmarks/forma-browser-gc-qualification-2026-11-02.json).
  Retained handles/DOM, async mixed-core, other engines, long-run plateau,
  reactive rebinding и произвольные callback effects остаются открытыми.
  Child cleanup throw после dispose и nested hotReload на commit
  проверены для двух views: focused23/23, types; старые callbacks inert, graph0
  после teardown. [Отчёт](benchmarks/forma-hmr-cleanup-qualification-2026-11-02.json).
  Определить поведение при остальных исключениях cleanup и reentrant callbacks
  после начала commit; проверить несколько views. Сохранение старого состояния
  до commit уже проверено, но произвольные внешние side effects не откатываются.
- [ ] **Watcher:** воспроизвести прежнюю потерю update5 и установить причину;
  расширить direct/atomic/chunked writes на другие OS/filesystems/editors.
  Успешные paced/unpaced/burst прогоны не доказывают устранение старого отказа.
- [ ] **Contracts:** parser/validator теперь отклоняют unknown annotations,
  inverted/nonfinite range и invalid/unsafe pattern; поведение описано в README.
  До правки6 regressions падали, focused40/40 после. Component-level
  duplicate/dangling и перенос через state/let/tokens/visual теперь отклоняются:
  ещё7 regressions до правки, focused48/48, full1380/102 files и types после.
  Вложенные annotations и контракты после visual root теперь отклоняются:
  ещё5 regressions до правки, focused55/55, full1387/102 files, types
  и installed-package gate после; HMR refusal
  сохраняет прежний view и допускает valid retry. Contract portion AST
  теперь проверяется до construction/HMR mutation:8 reproduced regressions,
  focused64/64, full1396/103 files, types и installed-package gate.
  Visual tree теперь проходит iterative shape/cycle admission; shared acyclic
  nodes допускаются; stable full1409/104 files, types и installed-package gate
  прошли. Kind-specific required fields и document metadata shape теперь
  проверяются:9 kind +8 metadata regressions, focused94/94, full1426/105 files,
  types и installed-package gate.
  Reserved state/let names и duplicate lets теперь отклоняются; signal/default
  dictionaries без prototype поддерживают допустимые toString/valueOf/etc
  и HMR reset. Focused107/107, full1439/106 files, types и installed-package
  gate прошли. Caller AST recognized fields теперь snapshotted до construction/HMR:
  iterative visual copy + detached contracts/lets/shortcuts/default data; focused119/119,
  full1451/107 files, types и installed-package gate (AOT9 cases) прошли.
  Открыты semantic admission выражений/остальных namespaces, accessors/proxies,
  function data ownership, snapshot overhead и rendering глубоких деревьев.
  Unknown extension fields теперь игнорируются без getter reads; focused131/131.
  До правки mounted runtime удерживал5/6 extension GC targets; после0/6,
  controls2→0, same probe hash. Full1463/108 files, types и installed-package
  gate (AOT10 cases) прошли. Snapshot cost diagnostic:18 isolated workers,
  A/A + trusted-input bypass comparison,2592 timed samples,8/32/96/256 controls.
  [Отчёт](benchmarks/forma-snapshot-cost-qualification-2026-11-02.json).
  Host load34–37/16 CPUs и A/A scatter до63% не позволяют закрыть performance
  gate. CPU profile construction выполнен; временные validation arrays/DFS
  frame objects убраны при сохранении checks и snapshots. Full1466/108 files,
  types и installed-package gate прошли. Сравнение с полной baseline-сборкой:
  256 controls construction0.199→0.155ms,6/6 paired rounds улучшились на этом host.
  [Квалификация](benchmarks/forma-validation-opt-qualification-2026-11-02.json).
  Повторить на спокойном host; mount/HMR и Browser GC остаются открытыми.
- [ ] **Cross-core:** расширить errors/cycles, mixed-core scheduling, stale handles
  и browser GC; проверить другие realms/workers, несовместимые версии и async
  factories; измерить стоимость bridges и lifetime guards.

### Native steady idle binding sync (2026-11-02)

- [x] Runtime пропускает повторный binding/Let replay только после успешного
  полного sync, при unchanged model/widgets/events и без pending render.
  First sync, rebuild и error retry сохраняют полный путь.
- [x] Release diagnostic lexical forms8/32/96/256: steady idle allocations
  570/6006/48666/334501→0 в обоих scheduler modes. Новая регрессия проверяет
  8 повторных idle sync без allocations, последующий edit/model commit и
  повторное прогревание после rebuild.
- [x] Rust lib166 + integration19 (2 retained,9 runtime allocations,8 artifact)
  прошли; release build и wasm32 compile check прошли.
  [Evidence](benchmarks/native-lexical-idle-qualification-2026-11-02.json).
- [ ] First idle after rebuild и edit-time rebuild/Let replay по-прежнему дороги;
  state equality/widgets scan остаются O(n). Paired latency/p95, full frame,
  live/peak/RSS, native window/IME/DPI/GPU и actual browser WASM не закрыты.

### Native direct-alias binding replay (2026-11-02)

- [x] Для direct root model aliases + scalar member keys разрешать origin и
  keys по borrowed данным, не материализуя lexical collection для каждого
  control. Alias chain поддержан; unsupported cases идут в прежний evaluator.
  Pure-key guard общий с прежним key-index path.
- [x] Native release allocator diagnostic, lexical editors8/32/96/256, обе
  scheduling modes: cold idle allocations570/6006/48666/334501→234/822/2394/6309.
  На256 editors edit allocations≈2.09m→1.77m, requested bytes≈284.7→239.6MB
  (Svet=true). Все7 edit samples в8 workloads снизили allocations/bytes.
- [x] Регрессия сравнивает borrowed path с general replay: alias chain, reorder,
  dotted key shadow/duplicate refusal и computed item ID fallback. Cold96
  allocation budget<10000 проходит в обеих modes; steady idle остаётся0.
  Rust167 lib +19 integration, release build, wasm32 compile check прошли.
  [Evidence](benchmarks/native-alias-binding-qualification-2026-11-02.json).
- [ ] Nested/complex Let replay, retained per-control snapshots, edit-time
  Runtime replacement, paired p95/full frame/RSS/native-window и actual browser
  WASM остаются открытыми. Requested bytes не означают live/peak память.

### Native alias boundary regression matrix (2026-11-02)

- [x] Сверить borrowed path с general replay для7 Let declaration sequences:
  direct/chain aliases, shadow root field, late shadow не меняет earlier alias,
  повторное имя, unrelated literal, readonly conditional collection.
  Каждый случай проверен до и после reorder; writable origins сохраняются.
- [x] Не скрывать ошибку отсутствующего state в unused Let; model unchanged.
  Rust169 lib +19 integration (188 tests) прошли. Проверено, что относительно
  предыдущей qualification добавлены только test blocks, production code тот же.
  [Evidence](benchmarks/native-alias-boundaries-qualification-2026-11-02.json).
- [ ] Более широкая expression/property-based matrix, nested aliases,
  actual rebuilt WASM/browser/native-window gates и остальные P2 задачи открыты.

### Rebuilt vector-ui WASM/browser qualification (2026-11-02)

- [x] Собрать текущий Rust vector-ui release WASM и сгенерировать NodeJS/web
  bindings wasm-bindgen0.2.125. Node:7 artifact checks +7 binding/snapshot cases.
- [x] Chromium156:10/10 CPU Canvas2D cases, single/chained Let edit,
 24 idle sync в каждом reload-reorder case, Unicode suffix, reload ids[b,a],
  новый edit правильного item;1242 changed pixel bytes внутри control в обоих.
  Served HTML/JS/WASM hashes проверены, generated bundle сохранён.
  [Evidence](benchmarks/native-current-wasm-qualification-2026-11-02.json).
- [ ] Reorder здесь через reload API, не live model mutation. Native stale-frame
  reorder проверен отдельно. Trusted DOM keyboard routing/native window/IME/DPI,
  hardware GPU, browser allocations/latency/RSS и выпуск остаются открытыми.

### Native focus-session recovery (2026-11-02)

- [x] Воспроизвести stale NativeHost.composing после WindowEvent::Focused(false)
  без отдельного Ime Disabled/Commit. Runtime уже очищал preedit, host flags нет.
- [x] Сбрасывать composing/modifiers/touch session до app/chooser dispatch при
  focus loss. Regression сохраняет uncommitted model value и проверяет editing
  после refocus. Native-feature suite:177 unit +19 integration passed,2 hardware
  GPU tests ignored. [Evidence](benchmarks/native-focus-session-qualification-2026-11-02.json).
- [ ] Физическая OS window/IME session, реальные keyboard/modifier events,
  switching backends/windows, DPI и hardware GPU qualification остаются открытыми.
  Этот handler regression не заменяет native-window прогон.

### Native physical Metal renderer checks (2026-11-02)

- [x] Выполнить оба ignored hardware tests явно, serial run, без software CPU
  adapter. Runtime adapter:Apple M4 Max / IntegratedGpu / Metal. Backend path
  использует MTLCreateSystemDefaultDevice, render encoder и command-buffer commit.
- [x] Reveal CPU/GPU:126 combinations (3 targets ×2 control counts ×3 scales ×7
  states), max channel error1/255 при budget2/255. State-only updates не делают
  geometry upload. Renderer resource reuse/timing smoke прошёл:7 buffers,
 10496 owned buffer bytes, geometry/tile upload1 each, bind-group creation1.
  [Evidence](benchmarks/native-metal-hardware-qualification-2026-11-02.json).
- [ ] Это offscreen test/debug execution, не OS presentation/window/IME matrix.
  Device loss, backend switching, other adapters, full renderer cases и
  application frame latency/p95/global VRAM/RSS остаются открытыми.

### Native IME navigation-session recovery (2026-11-02)

- [x] Воспроизвести Tab focus switch со stale composing=true. Native navigation
  выделена в handler, вызываемый из реального keyboard routing.
- [x] Pressed nonrepeat Tab/Escape отменяют preedit и завершают composing до
  navigation/dismissal. Release/repeat сохраняют composition/focus. Регрессия
  проверяет model без preedit commit, Escape pixel restoration, focus/next edit
  и неизменность второго model field.
- [x] Native-feature suite178 unit +19 integration passed (197);2 GPU tests
  skipped в этом прогоне, hardware run зафиксирован отдельно выше.
  [Evidence](benchmarks/native-ime-navigation-qualification-2026-11-02.json).
- [ ] Реальная OS IME session, trusted key sequence, pointer/backend switches,
  reverse navigation и input latency matrix остаются открытыми.

### Native pointer IME owner recovery (2026-11-02)

- [x] Воспроизвести pointer switch editor0→editor1 со stale composing=true.
- [x] Общий focus-sensitive wrapper у mouse/touch/scroll routes сравнивает
  editable owner до/после input. Hover не отменяет composition, смена/потеря
  owner очищает preedit и host flag. Без composition сохраняется direct path.
  Исходные touch batches по-прежнему используют один NativeApp update.
- [x] Regression: hover/editor switch/new-target commit, blank hover/press,
  отсутствие preedit/model corruption. Native-feature suite179 unit +19
  integration passed (198),2 GPU tests skipped в этом прогоне.
  [Evidence](benchmarks/native-pointer-ime-qualification-2026-11-02.json).
- [ ] Actual OS IME/pointer session, same-editor caret/drag, post-callback model
  replacement, backend/window switches и input latency остаются открытыми.

### Actual native-window input + refocus fix (2026-11-03)

- [x] Добавить fixture/probe, использующий именно reusable NativeHost, и собрать
  временный macOS app bundle. Standalone forma main.rs имеет отдельный host.
- [x] Реальные NSWindow/OS event routes: ASCII input, Tab/Shift+Tab, pointer
  focus switch, Escape, видимый model summary; journal подтверждает target fields.
- [x] OS minimize/reopen выявил потерю last editor:2→-1→1. Исправлено хранением
  stable editor key и повторным разрешением при refocus. Actual OS повтор:
 2→-1→2, следующий r меняет second на seed-Bkr, first остаётся seed-A.
- [x] Unit regression до fix падала; после199 Rust tests passed (180+19),2 GPU
  tests omitted в этом run. Stable-key reorder/duplicate blur проверены.
  Собственные app/Finder windows закрыты, probe process завершён.
  [Evidence](benchmarks/native-os-input-qualification-2026-11-03.json).
- [ ] Реальная CJK/marked-text IME, clipboard, accessibility, DPI changes,
  missing/disabled restoration keys, backend switches и standalone-host parity
  остаются открытыми. Probe trace — не latency qualification.

### P2 — native UI и стоимость больших форм

- [ ] Уменьшить per-control model snapshots и edit-time rebuild/Let replay.
  Снижение requested allocations и retained SceneArtifact уже измерено;
  latency, полный Runtime RSS/peak и готовность больших форм этим не доказаны.
- [ ] Провести release A/A и A/B на 8/32/96/256 controls на спокойном host:
  полный кадр, p50/p95 latency, live/peak memory, allocations, hashes и raw results.
- [ ] Определить writable origins derived collections; проверить transactions,
  reorder/replacement, вложенные Scroll, несколько окон, focus, IME и DPI.
- [ ] Проверить device loss, смену GPU, rounded AA и другие физические backends.
  Chromium CPU Canvas2D gate 8/8 не заменяет native-window/GPU квалификацию.

### P3 — расширение среды и продуктовые измерения

- [ ] Другие browser engines, async/Edge carriers, router bursts и deployed SSR
  transports: конкретные сценарии, lifecycle/abort tests и честные ограничения.
- [ ] Реальные приложения: startup, p95, frame time, long tasks, transfer bytes,
  queue/compute/readback и память. Сравнения движков — на одинаковых нагрузках.
- [ ] Проверить GPU projection/picking/clipping/depth и resident graph throughput.

### P4 — единый release candidate

- [ ] На одном revision заново собрать Rust → embedded WASM → installed package,
  проверить provenance, clean install, exports/types, browser/native/HTTP gates.
- [ ] Получить результат remote CI; собрать матрицу implemented / validated /
  partial / unsupported с командами воспроизведения и ссылками на evidence.
- [ ] Выпуск и публикацию выполнить отдельным согласованным этапом.

### Зафиксированное состояние доставки

Работа до этой редакции плана сохранена в `111d146` и отправлена в `origin/main`.
Все рабочие ветки влиты; остались main и release, один основной worktree.
Перед push прошли 560 общих JS-тестов, 1363 Svet, 184 Rust, TypeScript,
сборка Studio и installed-package gate. Remote CI этим не подтверждён.
Воспроизводимые build/cache outputs удалены; перед новой проверкой их нужно
пересобрать. Зависимости и сохранённые результаты измерений оставлены.
Новая редакция плана сама по себе не запускает разработку и не является релизом.

## Что осталось сделать — актуально на 2026-11-02

Это очередь открытых задач. Исторические результаты ниже не заменяют
повторную квалификацию текущих исходников и собранного пакета.

### Ближайшая очередь выполнения

1. **Довести HMR rollback и contract grammar.** Semantic-invalid inline
   contract admission проверен: Node6/6 и installed Chromium15/15; прежние
   DOM/state/cached factory сохраняются, valid update восстанавливает работу.
   Runtime bytes сверены с dist/package.
   [Отчёт](benchmarks/forma-hmr-validation-qualification-2026-11-02.json).
   Известные range/pattern с неверными аргументами теперь явно отклоняются:
   5 воспроизведённых отказов до правки, contract9/9 и full1351 после неё.
   Fresh installed package/GC и Chromium HMR15/15 прошли; общий matcher также
   исправляет потерю multiline pattern, проверенную installed AOT/runtime gate.
   Full1352/102 files; [отчёт](benchmarks/forma-pattern-parity-qualification-2026-11-02.json).
   Остались общая annotation grammar, политика unknown annotations и
   атомарный rollback AST/state/нескольких views. Staged construction и
   частичная DOM insertion теперь сохраняют предыдущий view при отказе;
   full1354, types/build, fresh installed package/GC и Chromium17/17 прошли.
   [Отчёт](benchmarks/forma-hmr-staging-qualification-2026-11-02.json).
   Все mounted views теперь подготавливаются перед retirement старых деревьев:
   отказ второго view сохраняет оба старых DOM; full1355 tests прошли.
   Fresh installed package/GC и Chromium18/18 прошли, все20 runtime files
   совпали с dist/package. [Отчёт](benchmarks/forma-hmr-multiview-qualification-2026-11-02.json).
   При preparation failure metadata/state/props/lets теперь восстанавливаются;
   старые computed handles остаются реактивными, candidate signals удаляются.
   Full1357, types/build, installed package/GC и Chromium19/19 прошли.
   [Отчёт](benchmarks/forma-hmr-document-qualification-2026-11-02.json).
   После начала commit rollback не гарантирован; in-place mutations и внешние
   побочные эффекты factory не восстанавливаются;
   connected callbacks могут видеть старые и кандидатные nodes одновременно.
   Reentrant dispose/unmount из old child cleanup на commit boundary проверены:
   candidate освобождается, retired view не публикуется; full1359 и Chromium21/21,
   types/build и installed package/GC проходят.
   [Отчёт](benchmarks/forma-hmr-commit-retire-qualification-2026-11-02.json).
   Произвольные cleanup exceptions и остальные reentrant commit callbacks открыты.
   Factory throw после создания same-copy Forma child теперь освобождает потерянный
   handle и helpers: full1360, types/build, installed package/GC и Chromium22/22.
   [Отчёт](benchmarks/forma-factory-qualification-2026-11-02.json).
   Constructor-owned allocations при props/locale/let metadata throw теперь
   освобождаются; external ViewModel signal сохраняется. Full1363, types/build,
   installed package/GC и Chromium25/25 прошли.
   [Отчёт](benchmarks/forma-constructor-qualification-2026-11-02.json).
   Construction между совместимыми copies одного JS realm проверен в обе
   стороны: installed Node2/2, paced Chromium27/27, full1363 и package/GC.
   [Отчёт](benchmarks/forma-construction-copies-qualification-2026-11-02.json).
   Unpaced browser run потерял watcher update5; отказ сохранён и не закрыт
   paced250ms прогоном. Трассируемый unpaced lifecycle27/27 и burst3×32 writes
   прошли с доставкой последней версии каждой серии; isolated Node6/6 тоже
   проходит. Это диагностическая матрица, причина прежнего отказа не доказана.
   [Расследование](benchmarks/forma-watcher-investigation-2026-11-02.json).
   Direct/atomic/chunked write matrix: Node18/18; Chromium9×32 writes
   (288 total) доставляют последнюю версию и завершаются graph0.
   [Отчёт](benchmarks/forma-watcher-write-matrix-qualification-2026-11-02.json).
   Причина прежнего пропуска не установлена; другие filesystems/editors/OS
   и воспроизведение отказа остаются открытыми.
   Другие realms/workers, incompatible versions и async factories открыты.
2. **Довести расследование HTTP external memory.** Текущий пакет прошёл
   natural-GC 2h с прежними бюджетами; 22 hashes сверены после пересборки.
   Объяснить исторический отказ и пики, затем расширить transport matrix.
   Отдельный старый traced runtime не квалифицирует текущие исходники.
3. **Закрыть матрицу cross-core lifecycle.** Расширить mixed-core scheduling,
   errors/cycles, browser GC и сохранённые handles; измерить overhead.
   Уже проверенные focused scenarios не считать общей гарантией.
4. **Снизить стоимость больших native forms.** Начать с per-control snapshots
   и edit-time rebuild; затем release A/A и A/B для 8/32/96/256 controls на
   спокойном host, включая полный кадр, latency и live/peak memory.
5. **Довести identity и native interaction.** Определить writable origins для
   derived collections, проверить transactions/reorder/replacement, несколько
   Scroll и окон, DPI, IME, focus, переключение GPU и device loss.
6. **Расширить среду исполнения.** Другие browser engines, async/Edge carriers,
   router bursts, rounded AA и physical GPU/backends, deployed SSR transports.
7. **Собрать единый release candidate.** Пройти все gates на одних исходниках
   и артефактах, clean install и remote CI; выпустить матрицу
   implemented / validated / partial / unsupported с evidence.

Для каждого пункта результатом должны быть исправление или явная граница
поддержки, regression, воспроизводимая команда и отчёт с hashes. Сборку пакета
и тесты, импортирующие dist, выполнять последовательно. Commit/push/release —
отдельным шагом по запросу. Детальная приёмка каждого направления — ниже.

### P1. Реактивность между независимыми cores

- [x] Реализовать базовый мост зависимостей: proxy signal в Rust-графе наблюдателя,
  forwarding effect в Rust-графе источника; JS передаёт уведомления и значения.
- [ ] Удалять динамические зависимости, освобождать обе стороны при dispose,
  reset и смене generation; исключить удержание cores через сильные ссылки.
- [ ] Проверить signal/computed, equality/lazy evaluation, batch, циклы,
  исключения, nested observers, устаревшие handles и GC независимых копий.

Базовый native proxy bridge реализован. Native batch удерживается во всех
затронутых cores до общей границы доставки; proxy writes сразу меняют native
state. Foreign boundaries computed-цепочки обновляются перед чтением кэша
внутри batch. Независимые копии проходят **22/22** probe cases.
Динамические зависимости, reset/dispose, reuse slots, equality/equals:false,
циклы и ошибки имеют regressions. Полная JS-серия: **1332 passed**, 100 files,
без expected failures. Types/package checks проходят.

- [x] Объединять уведомления target observer от нескольких foreign sources
  одного batch; бывший expected-failure regression теперь проходит обычно.
- [x] Проверить nested mixed-core batch, ошибки body/target, reset held target,
  новую generation в том же batch, свежесть computed/peek и ограничение
  feedback loop с последующим восстановлением.
- [x] Проверить three-core diamond с обоими порядками чтения, с batch и без
  него; объединить доставку для обычной записи с foreign subscribers.
- [x] Проверить запись из effect rerun и initial execution: очередь source-core
  flush имеет общую границу доставки, промежуточный diamond snapshot исключён.
- [x] Сохранять foreign computed dependencies после initial/later failure;
  target handling, same-value recovery, computed-chain и uncaught-error tests.
- [x] Проверить Chromium browser DOM failure/recovery на source и packed installed
  copies:5/5+5/5, error diamond, callback teardown и graph cleanup.
  [Отчёт](benchmarks/cross-core-error-browser-qualification-2026-11-02.json).
- [ ] Расширить произвольные mixed-core scheduling/diamond/cycle scenarios;
  browser qualification и измерения overhead.

- [x] Добавить dedicated explicit-teardown bridge GC gate: 9 сценариев ×8,
  440 teardown targets освобождены; 4 generic и 3 active-subscription controls
  остаются живыми до снятия ссылок и освобождаются после него. Gate встроен
  в package check для packed installed независимых копий.
  [Отчёт](benchmarks/cross-core-error-retention-qualification-2026-11-02.json).
- [ ] Проверить browser GC и долгоживущие/сохранённые пользовательские handles;
  forced-GC explicit-teardown результат не доказывает natural-GC plateau.

Приёмка всего этапа: расширенная матрица, отдельные bridge GC и browser
проверки подтверждены на текущем пакете. Обычный installed consumer/GC gate
не заменяет dedicated retention qualification для нового моста.
[Последняя квалификация](benchmarks/cross-core-error-qualification-2026-11-02.json).

### P1. Разобрать рост external memory в HTTP/SSR

- [ ] Сопоставить external/ArrayBuffers с очередями, сокетами, backpressure,
  WASM/native allocations и жизненным циклом запросов; найти источник пиков.
- [x] Добавить bounded diagnostics: server socket/response/queue counters,
  active resource kinds, V8 allocation metrics, global/window external peaks
  с одновременными counters и shutdown после socket.close. Smoke natural,
  forced и injected-failure прошли; причина external peaks ещё не установлена.
- [x] Проверять diagnostic socket/response accounting и shutdown в analyzer;
  regressions включены в Svet CI. Malformed schema и truncated JSON дают
  structured refusal; 9 unittest methods прошли. Remote CI пока не выполнен.
- [x] Добавить opt-in request core/memory FinalizationRegistry counters без
  сильного удержания targets; natural1936 requests, forced и disabled smoke,
  identity/gzip compatibility прошли. Это accounting callback lag, не live census.
  [Отчёт](benchmarks/http-core-lifetimes-qualification-2026-11-02.json).
- [x] Завершить frozen2h snapshot-Nv0ST2 с lifecycle tracing:
  [run metadata](benchmarks/http-core-traced-2h-run-2026-11-02.json).
  Подтвердить allocation/GC гипотезу длинным frozen run с lifecycle tracing;
  причина исторического 2h budget failure пока не доказана.
  Final:1144736 requests/7200025.895ms,0 request/cleanup failures,0 forcedGC;
  все4 бюджета соблюдены,external post-warmup peak growth49686991 bytes.
  Все381595 server sockets закрыты; runtime/source hashes совпали с manifest.
  Это старый runtime; текущий пакет не квалифицирован.
  [Финальный отчёт](benchmarks/http-core-traced-2h-qualification-2026-11-02.json).
- [x] Завершить frozen 30-minute diagnostic текущего installed runtime:
  [manifest](benchmarks/http-diagnostics-30m-manifest-2026-11-02.json),
  [run metadata](benchmarks/http-diagnostics-30m-run-2026-11-02.json).
  Это диагностический прогон, не замена 2-hour qualification.
  Final:359048 requests,0 failures/forcedGC,все server sockets закрыты;
  все4 memory budgets соблюдены,2h minimum не достигнут.
  [Final qualification](benchmarks/http-diagnostics-30m-qualification-2026-11-02.json).
- [ ] Воспроизвести и объяснить прежний отказ примерно на 24-й минуте.
- [ ] После исправления выполнить новый frozen-runtime natural-GC soak
  текущего пакета, со сверкой hashes и прежними бюджетами памяти.
  Запущен snapshot-RKk0T4 на consumer6fmUiE без lifetime tracing:
  [run metadata](benchmarks/http-current-runtime-natural-2h-run-2026-11-02.json).
  Квалификация pending,7200000ms minimum и16/64/64/16MiB бюджеты сохранены.
- [ ] Расширить проверки: gzip/codecs, slow clients, disconnect storms,
  socket saturation; отдельно deployed proxy/CDN, TLS и HTTP/2/3.

Последний двухчасовой прогон завершён: **1 289 120 запросов**, request/cleanup
failures — 0, nodes после завершения — 0. Рост post-warmup external peak
**72 409 329 bytes** превышает бюджет **67 108 864 bytes (64 MiB)**.
Heap/RSS/ArrayBuffers укладываются в бюджеты. Это требует расследования,
но само по себе не доказывает утечку. Прогон предшествует последней правке
reactivity.js и не квалифицирует новый dist. Повышение бюджета не является
исправлением. Приёмка: причина подтверждена измерениями, regression есть,
новый полный прогон проходит все четыре бюджета без forced GC.
[Отчёт](benchmarks/http-natural-latest-2h-qualification-2026-11-02.json).

### P2. Производительность native bindings и больших форм

Исходный allocation baseline: Svet256 edit240822218 requested bytes без paint.
Два scope-copy эксперимента не изменили counters и убраны; исходник восстановлен
byte-for-byte. Следующий шаг — измерить callsites глубоких __state/model clones,
сохранив props.__state и owned public snapshots.
[Отрицательный результат](benchmarks/native-scope-experiment-qualification-2026-11-02.json).

- [x] Убрать unconditional visual expand_sequence state clone: borrowed state
  до Let write, owned For locals; Svet256 edit requested bytes240.82→104.30MB,
  два release прогона с одинаковыми counters.160 lib/10 allocation/8 artifact,
  wasm32 build и actual Node WASM gate проходят.
  [Отчёт](benchmarks/native-state-cow-qualification-2026-11-02.json).
- [x] Убрать discarded visual AST clone и заимствовать definition nodes до
  supplied-content mutation: Svet256 edit104.30→103.14MB requested bytes,
  два совпадающих release allocation прогона;161 lib/10 allocation/8 artifact,
  wasm32 build и actual Node WASM проходят.
  [Отчёт](benchmarks/native-visual-shell-qualification-2026-11-02.json).
- [x] Передавать owned __state через visual instantiation с возвратом на Result
  success/error; не копировать всё дерево для root Frame measurement.
  Svet256 edit103.14→57.63MB requested bytes, два одинаковых allocation прогона;
  162 lib/10 allocation/8 artifact, wasm32 и actual Node snapshot2/2 проходят.
  ABI gates добавлены в studio CI; remote CI не выполнялся.
  [Отчёт](benchmarks/native-snapshot-transfer-qualification-2026-11-02.json).
- [x] Передавать expanded flat Frame children вместо clone и использовать
  snapshot transfer в fallback: Frame Svet256 edit103.23→57.38MB requested,
  два одинаковых allocation прогона; Column counters не изменились.
  163 lib/10 allocation/8 artifact и actual Node snapshot3/3 проходят.
  [Отчёт](benchmarks/native-flat-transfer-qualification-2026-11-02.json).
- [x] Передавать resolved collection path отдельно от полного lexical frame:
  --lexical Svet256 edit599.37→509.11MB requested bytes(-15.1%), два одинаковых
  allocation прогона; ordinary Column counters не изменились.164 lib/10
  allocation/8 artifact, wasm32 и actual Node alias/snapshot4/4 проходят.
  [Отчёт](benchmarks/native-lexical-frame-qualification-2026-11-02.json).
- [x] Заимствовать начальный Let replay state и передавать evaluated locals:
  lexical Svet256 edit509.11→419.40MB requested bytes(-17.6%), два одинаковых
  allocation прогона; ordinary counters неизменны.164 lib/10 allocation/8
  artifact, wasm32 и actual Node single/chained alias5/5 проходят.
  [Отчёт](benchmarks/native-let-replay-qualification-2026-11-02.json).
- [x] Не копировать captured locals/parent values, заменяемые replayed lexicals:
  lexical Svet256 edit419.40→374.63MB requested bytes(-10.7%), два одинаковых
  allocation прогона; ordinary counters неизменны.164 lib/10 allocation/8
  artifact, wasm32 и actual Node alias/snapshot5/5 проходят.
  [Отчёт](benchmarks/native-local-overlay-qualification-2026-11-02.json).
- [x] Убрать duplicate private frame locals, уже сохранённые в __lexicals:
  lexical Svet256 edit374.63→284.71MB requested bytes(-24.0%), два одинаковых
  прогона; ordinary counters неизменны.165 lib/10 allocation/8 artifact,
  wasm32/actual Node5/5 и old/new WASM artifact comparison4/4 проходят.
  256-control JSON35.65→28.62MB; preceding local reader принимает новый формат.
  [Отчёт](benchmarks/native-frame-dedup-qualification-2026-11-02.json).
- [x] Убирать root replay metadata только при точной копии в retained frames:
  owned256-control artifact181.09→135.62MB Rust allocator bytes(-25.1%),
  Drop delta0 во всех samples; --artifact-retention измеряет отдельно от edit.
  JSON28.62→21.52MB.166 lib/10 allocation/8 artifact, actual Node5/5 и
  old/new comparison4/4 проходят. Edit requested counters не изменились.
  [Отчёт](benchmarks/native-replay-transport-qualification-2026-11-02.json).
- [ ] Приоритет: снизить оставшиеся Let replay/model copies в lexical forms;
  284MB requested/edit при256 controls ещё не означает interactive readiness.
- [ ] Уменьшить оставшиеся per-control snapshots модели и scopes, повторную scene expansion
  в public Document sync и edit-time rebuild; проверить сложные key expressions.
- [ ] Замерить Let replay и sharing между frames; сохранить независимость
  snapshots и доступ к props.__state.
- [ ] Снять новый release baseline для 8/32/96/256 controls: legacy/Svet,
  idle/edit, A/A и A/B на спокойном host; отдельно render и presentation.
- [ ] Измерить latency p50/p95/p99, live/peak memory и RSS; cumulative requested
  allocator bytes публиковать отдельно. Повторить на другом оборудовании.
- [x] Убрать discarded supersampling из radius=0 path и восстановить safe
  interior fast path без reveal. CPU/reference и hardware GPU regressions прошли.
  [Отчёт](benchmarks/native-square-cache-qualification-2026-11-02.json).
- [ ] Измерить release стоимость точного rectangle/border AA и cache build
  после удаления лишней работы на спокойном host; FPS/latency не подтверждены.

Уже сделаны owned transfers layout/position/visual/state, lazy staging и
transaction-local key index. Idle96 достиг 1331 allocations; прежний edit256
около 238.43 MB requested bytes остаётся историческим измерением.
Приёмка: актуальные source-bound измерения, корректность bindings/scopes,
allocation budgets, native/wasm checks; готовность больших форм подтверждается
полным кадром и согласованным целевым latency/memory бюджетом.

### P2. Identity, транзакции и lexical scopes

- [ ] Определить безопасный writable-origin контракт для derived/filter/map
  collections; неподдержанные неоднозначные записи явно отклонять.
- [ ] Проверить замену компонента с тем же key и перенос pending events через
  замену документа: без потери, повторной доставки и записи в чужой instance.
- [ ] Расширить model/widget conflict matrix, deep/computed scopes и malformed
  metadata; ошибки и повторные отказы должны сохранять ввод и атомарность.
- [ ] Подтвердить compiler/AOT/browser parity для source Let/For/If/Match,
  shadowing, alias chains и reorder/removal/duplicate identities.

Прямые root-backed и parent-member aliases, lexical frames и source Let уже
имеют focused coverage; общая derived/deep matrix остаётся открытой.
Приёмка: source-level regressions для каждой границы, fresh/cached maps,
rollback/retry, однократное принятие события и пустой graph после teardown.

### P2. Native lifecycle, окна и Scroll

- [ ] Проверить несколько форм/окон, callbacks/scopes, close/reload/document
  replacement и отсутствие оставшихся jobs/resources.
- [ ] Пройти реальные OS resize, несколько мониторов/DPI, IME/focus/input,
  CPU↔GPU switching и device-loss recovery с видимым кадром.
- [x] Сохранить explicit-height пустого layout в sole-root Scroll retained
  `.ui` Runtime: legacy/Svet rebuild, offset preservation/shrink и layout revision.
  [Отчёт](benchmarks/native-logical-scroll-qualification-2026-11-02.json).
- [x] Передавать logical bounds через публичный Rust SceneArtifact и
  Runtime::from_scene_artifact; Form::sync использует тот же артефакт.
  [Отчёт](benchmarks/native-scene-artifact-qualification-2026-11-02.json).
- [x] Добавить versioned JSON SceneArtifact с lossless typed values, atomic load
  и WASM compile/load API; actual generated NodeJS WASM execution прошёл.
  [Отчёт](benchmarks/native-scene-json-qualification-2026-11-02.json).
- [x] Проверять mapping count и key/order против загруженных controls до
  замены Runtime; native и actual Node WASM refusal/recovery tests прошли.
  [Отчёт](benchmarks/native-scene-mapping-qualification-2026-11-02.json).
- [x] Проверить browser ESM/WASM JSON artifact в Chromium: 5/5 scenarios,
  clipping/pixel-change checks и CPU scales1/1.25/2.
  [Отчёт](benchmarks/native-scene-browser-qualification-2026-11-02.json).
- [x] Повторить current native WASM Chromium compile/input/CPU raster slice
  после ownership/metadata правок:8/8,single/chained Unicode alias commit,
  custom props.__state update,clipping/refusal recovery,scales1/1.25/2.
  [Отчёт](benchmarks/native-current-browser-qualification-2026-11-02.json).
- [ ] Расширить artifact browser matrix на другие engines/browser GPU;
  legacy raw markup остаётся control-only, logical metadata требует artifact.
- [x] Сохранить leading edge oversized content в sole-root Scroll: empty
  Row/Column/Stack/Frame/Grid и TextInput, полный192x300 offset и возврат0.
  Native, actual Node WASM и existing hardware GPU regressions прошли.
  [Отчёт](benchmarks/native-scroll-horizontal-qualification-2026-11-02.json).
- [ ] Довести вложенные/несколько Scroll viewports, общую overflow/empty-layout
  matrix и browser interaction; проверить
  clipping, hit testing и offset clamp.

Sole-root Scroll и focused resize/scroll recovery проверены; это не полная
оконная квалификация. Приёмка: реальные окна, сохранённое состояние,
корректные кадры/ввод и освобождение ресурсов после каждого сценария.

### P3. Browser, router и async lifecycle

- [ ] Пройти непрерывный high-rate history burst; paced burst не заменяет его.
- [ ] Проверить реальный Vite HMR и production AOT: повторные mount/unmount,
  callbacks, ownership, pending async work, ошибки и восстановление.
- [ ] Расширить независимые package copies, browser/Edge async carriers,
  другие браузеры и deployed providers; явно описать unsupported границы.

Приёмка: автоматические browser regressions плюс наблюдаемая работа
приложения, отсутствие stale callbacks и остаточных graphs/listeners/tasks.

### P3. GPU и визуальная корректность

- [ ] Расширить fractional AA на rounded borders и frame masks.
- [ ] Проверить projection/picking/clipping/depth независимыми reference
  comparisons, сложные сцены и сочетания прозрачности.
- [ ] Проверить другие physical GPU/backends и восстановление после отказов.

На Apple M4 Max/Metal квадратные rectangles/borders прошли 72 CPU/GPU cases
с max difference 1/255; это не покрывает rounded geometry и другие backends.
Приёмка: сохранённые source-bound pixel artifacts, явные tolerances,
независимые эталоны и hardware matrix без скрытых исключений.

### P4. Итоговая квалификация и доставка

- [ ] Собрать единую матрицу implemented / validated / partial / unsupported
  с командами, версиями среды, hashes и ссылками на evidence.
- [ ] Запустить полные JS/types/package/installed-GC, Rust/native/wasm,
  hardware GPU, browser/AOT и долговременные SSR gates на одном кандидате.
- [ ] Проверить remote CI, чистую установку и provenance Rust→WASM→package;
  повторить критические сценарии на другом оборудовании.
- [ ] Отдельно подготовить commit/merge/push/release, когда это будет запрошено.

Готовность подтверждается только всей согласованной матрицей текущего
кандидата. Частные зелёные тесты не закрывают P1–P4 и не означают «до предела».

Рекомендуемый порядок: cross-core bridge → external-memory investigation →
native snapshots/performance → identity/lifecycle → browser/GPU environments →
единый qualification candidate. Длительные прогоны запускать на замороженном
кандидате после зависимых правок, сохраняя manifest и финальный отчёт.

## История частных результатов

Release retained Runtime измерен на8/32/96/256 editors, legacy/Svet, два
последовательных прогона с совпавшими allocation counters. Svet256: idle
278501 requested bytes, edit377214957 bytes; edit median93–95ms в текущем
неизолированном окружении, без paint/presentation. Следующий приоритет —
per-control model snapshots и edit-time rebuild; готовность больших форм
не подтверждена. [Отчёт](benchmarks/native-retained-runtime-qualification-2026-11-02.json).

Owned normalization убрала повторные копии промежуточных layout trees:
Svet256 edit377.21→330.37MB requested bytes (−12.4%). Timing нестабилен;
ускорение не подтверждено. [Отчёт](benchmarks/native-owned-layout-qualification-2026-11-02.json).

Позиционирование теперь потребляет expanded children: Svet256 edit
330.37→239.39MB requested bytes (−27.5%), median57.4–57.7ms в текущем
прогоне без paint. Это ещё не interactive readiness.
[Отчёт](benchmarks/native-owned-position-qualification-2026-11-02.json).

Precomputed visual trees теперь тоже передаются во владение: Svet256 edit
239.39→238.43MB requested bytes (−0.40%). Выигрыш небольшой; основной
открытый расход — snapshots модели на каждый control.
[Отчёт](benchmarks/native-owned-visual-qualification-2026-11-02.json).

Scheduler больше не копирует неизменённую модель на idle: Svet96 сравнялся
с legacy —1331 аллокация. Исправлены уведомления при добавлении/удалении
пустых scopes; pending render retry сохраняется.
[Отчёт](benchmarks/native-idle-graph-qualification-2026-11-02.json).

Вложенные root-backed пути через индексы списков поддержаны в чтении/записи
и IR Document bindings; reorder сохраняет stable-key адресацию. Это ещё не
поддержка nested For через parent locals и не compiler/AOT qualification.
[Отчёт](benchmarks/native-nested-paths-qualification-2026-11-02.json).

Nested For через parent local теперь разрешает root-backed путь по stable
parent key. Реальный .ui проверен на parent/child reorder, удаление родителя,
дубликаты и fresh/cached maps. Derived collections, одинаковые shadowed имена
родителя и полная computed/deep matrix остаются открытыми.
[Отчёт](benchmarks/native-nested-for-qualification-2026-11-02.json).

Соседние nested For с одинаковым item-name теперь сохраняют lexical parent
context: проверены12 reorder/removal/duplicate сценариев и три уровня item.
Открыты nonadjacent shadowing, общий scope chain, стоимость parent snapshots
и computed/derived collections.
[Отчёт](benchmarks/native-shadow-parent-qualification-2026-11-02.json).

Nonadjacent item→branch→item проверен на reorder и fresh/cached maps.
Полный lexical environment всё ещё нужен для поздних ссылок на более ранние
locals через несколько shadowings, а также computed keys; не считать scope
chain завершённым. [Отчёт](benchmarks/native-nonadjacent-shadow-qualification-2026-11-02.json).

Binding identity теперь использует плоские lexical frames с declaration-time
parent indexes; рекурсивные whole-state parent snapshots больше не создаются.
Поздняя ссылка на branch после item-shadow + computed key + reorder проверены.
Открыты Let/root-name shadowing, shared locals/snapshots, deep/error matrix
и derived collections. [Отчёт](benchmarks/native-lexical-frames-qualification-2026-11-02.json).

Let-значения теперь явно сохранены в lexical frames и имеют приоритет над
одноимённым root-полем; compound key + reorder проверены fresh/cached в IR.
Открыты динамическая переоценка Let, Let-derived aliases и полная compiler/
browser/AOT matrix. [Отчёт](benchmarks/native-let-scopes-qualification-2026-11-02.json).

Let replay теперь хранит выражения и порядок объявлений; текущая модель и
prior Let results участвуют в восстановлении key. Constant/dynamic prefix→suffix
и смена key identity проверены fresh/cached в IR. Открыты replay allocations,
Let-derived aliases и полная error/deep/compiler/browser matrix.
[Отчёт](benchmarks/native-dynamic-let-qualification-2026-11-02.json).

Let error/retry regression: поздний отказ откатывает ранние staged writes,
два повторных отказа сохраняют ввод; исправление принимает обе записи один
раз и не переигрывает widgets поверх новой модели. Проверено fresh/cached в IR.
[Отчёт](benchmarks/native-let-retry-qualification-2026-11-02.json).

Let replay переиспользует рабочую модель между steps с восстановлением
overlays; полная копия теперь одна на replay frame. Измерение latency/bytes
и sharing между frames остаются открытыми.
[Отчёт](benchmarks/native-let-replay-reuse-qualification-2026-11-02.json).

Lexical-frame routing валидирует индекс/item/parents и limit64; повреждённые
данные отказывают без legacy fallback. Отрицательный active index проверен
с атомарным rollback и последующим retry ввода. Полная malformed matrix открыта.
[Отчёт](benchmarks/native-frame-validation-qualification-2026-11-02.json).

Прямые Let collection aliases теперь сохраняют symbolic model origin:
rows=state.items→chosen=rows, For chosen/state.chosen, root-name shadow и
reorder проверены fresh/cached в IR. Совпадение значений copied list не даёт
права записи в модель. Общие computed/filter/map collections ещё открыты.
[Отчёт](benchmarks/native-let-alias-qualification-2026-11-02.json).

Parent-member Let aliases (`rows=outer.items`) проверены в расширенной
матрице24 сценария: direct/alias, shadow names, fresh/cached, reorder/removal/
duplicate parent. Let wrapper добавлен в IR; source-syntax/browser/performance
qualification остаётся открытой. [Отчёт](benchmarks/native-parent-alias-qualification-2026-11-02.json).

Исходный `.ui` Let внутри For теперь виден следующим siblings; итерации
изолированы. Все24 alias/reorder/error сценария переведены на source syntax
без вставки IR; отдельный scope-leak/key-stability тест проходит. Performance
For+Let, browser/AOT/window matrices остаются открытыми.
[Отчёт](benchmarks/native-source-let-qualification-2026-11-02.json).

Последовательные scopes унифицированы для containers/For/If/Match/Let.
Исходный If/else с зависимыми Let и переключениями проверен: locals видны
внутри ветки и не выходят наружу. Открыты сложная Match/deep matrix и
release performance. [Отчёт](benchmarks/native-sequence-scopes-qualification-2026-11-02.json).

Source Match guard + Let aliases + different branch bindings проверены:
старый widget не пишет в новую ветку, при возврате ключ восстанавливается,
locals не выходят наружу. Cached branch-switch/error и deep matrix открыты.
[Отчёт](benchmarks/native-match-scopes-qualification-2026-11-02.json).

Исправлен reproduced Match-switch+render-error bug: recovery использует
current structural binding map без visual/layout, не старую карту ветки.
Legacy/Svet retained Runtime проверены до/после; structural expansion error
теперь отказывает без stale fallback. Полная selection/error matrix открыта.
[Отчёт](benchmarks/native-branch-recovery-qualification-2026-11-02.json).

Changed-model preflight теперь только structural binding expansion; visual
render выполняется после bindings. Repair98-control формы проверен с budget
<450000 аллокаций (353598 наблюдено). Release сравнение и peak memory открыты.
[Отчёт](benchmarks/native-structural-preflight-qualification-2026-11-02.json).

Structural/rendered maps совпали в18 layout/branch/reorder сценариях по
node/key/state/event_args. Исправлен negative-y Scroll из-за несовместимого
precomputed kind. Новый открытый дефект: Scroll viewport props (width/height)
не представимы текущей сериализацией runtime grammar; нужно исправить, не
считать Scroll matrix полной. [Отчёт](benchmarks/native-binding-map-parity-qualification-2026-11-02.json).

Root Scroll width/height теперь lowering в Frame padding: numeric/percent,
root padding, сохранение внешних размеров, scroll clamp и hit testing проверены.
Предыдущий serialization defect закрыт для sole root Scroll внутри frame bounds.
Multiple/nested viewports, empty content/resize и pixel/window matrix открыты.
[Отчёт](benchmarks/native-scroll-viewport-qualification-2026-11-02.json).

Исправлена layout_revision при viewport-only resize; retained legacy/Svet
проверены на сохранение/clamp offset, empty items→0 controls/offset и idle.
OS-window/multi-monitor resize и viewport pixel matrix ещё открыты.
[Отчёт](benchmarks/native-scroll-resize-qualification-2026-11-02.json).

Physical Metal Scroll reference:9 кадров (DPI1/1.25/2 × offsets0/32/64),
exact RGBA на96.875% пикселей; outside viewport проверен полностью. PNG
сохранены/просмотрен representative frame; GPU suite4/4. Scrollbar, другие
backends и window resize matrix открыты.
[Отчёт](benchmarks/native-scroll-pixels-qualification-2026-11-02.json).

Fractional viewport63×61 at32.5/33.5 проверен на Metal:9 кадров по
pixel-center clip контракту, все outside pixels exact; AA/seam/scrollbar только
внутри исключены. GPU5/5; это не area-AA/cross-backend/window qualification.
[Отчёт](benchmarks/native-fractional-scroll-qualification-2026-11-02.json).

Приоритетный воспроизведённый дефект: CPU/GPU fractional AA parity —
3/18 кадров расходятся до59/255 на corners/seams. Новый regression падает;
hardware suite5 passed/1 failed. Нужно унифицировать precise rectangle
coverage/compositing, сохранив качество GPU. Предыдущие crop proofs не
закрывают эту область. [Данные](benchmarks/native-scroll-renderer-parity-qualification-2026-11-02.json).

Fractional AA defect закрыт для borderless radius0 rectangles: CPU/GPU exact
pixel-area coverage,18/18 кадров max1/255 (было59). Независимые corner/seam
эталоны проходят; GPU6/6. Rounded/bordered AA, frame masks и performance
стоимость sparse edges ещё открыты.
[Отчёт](benchmarks/native-exact-rect-qualification-2026-11-02.json).

Translucent rectangle parity:18 alpha кадров exact, совокупно36/36 CPU/GPU
max1/255; независимые alpha corner/seam эталоны проходят, Metal6/6.
Rounded/bordered/frame-mask alpha matrix открыта.
[Отчёт](benchmarks/native-alpha-parity-qualification-2026-11-02.json).

Square border AA теперь exact outer-minus-inset area с translucent source-over.
До32/255 расхождения исправлены:72/72 кадров max1/255, thin1.25 и covering40
borders проверены, Metal6/6. Rounded border AA и performance остаются открытыми.
[Отчёт](benchmarks/native-exact-border-qualification-2026-11-02.json).

P1 cross-core refreshed:11/13; foreign invalidation и nested observer attribution
воспроизведены на current independent builds. Требуется active-domain routing
и Rust proxy-node bridge, с generation/lifetime, dynamic retirement, teardown,
computed equality/lazy, batch и cycle gates; host — только transport.
[Отчёт](benchmarks/cross-core-current-qualification-2026-11-02.json).

Cross-core attribution исправлена: paused outer не получает foreign read.
JS1301/99 + types/package/installed GC проходят; probe11/13 всё ещё открыт
из-за target invalidation (native proxy bridge не реализован).
[Отчёт](benchmarks/cross-core-domain-routing-qualification-2026-11-02.json).

Frozen latest-at-launch2h HTTP завершён:1289120 requests, failures0, hashes
20/20 verified. External peak growth72.41MB >64MiB budget; остальные3 budgets
прошли. Нужен разбор external memory, budget не повышать. Текущий dist уже
отличается reactivity.js; новый runtime этим не квалифицирован.
[Final](benchmarks/http-natural-latest-2h-qualification-2026-11-02.json).

Последний native checkpoint: lib 158/158, allocation 8/8, Metal GPU 6/6,
native/wasm32 checks. Это ограниченная локальная проверка, а не полная готовность.
См. [последний отчёт](benchmarks/native-scope-cow-qualification-2026-11-02.json).

## Рабочий план: что осталось сделать — 2026-11-02

Этот раздел задаёт актуальную последовательность. Ниже сохранены подробности
и история проверок; старые результаты не подтверждают последующие редакции.
Незакрытые пункты отмечены `[ ]`. Готовый owned Rust context — основа интеграции,
а не готовый native backend.

### Этап 1. Подключить Svet к настоящим native bindings Forma

- [x] Добавить native dependency `svet-wasm` в `vector-ui` и owned Context
  на экземпляр Form; сохранить обычный и wasm backend.
- [x] Направить изменения Property/Field через signal/effect графа Svet:
  постановка задач, batch dedup, drain и повторное tracking перед обновлением Runtime.
- [ ] Сохранить two-way input, context replacement, reentrant notifications
  и pending updates при переключении backend; исключить потерю обновлений.
- [x] Реализовать освобождение bindings и графа при replace/drop, включая
  сохранённые внешние Control handles; stale jobs не должны затрагивать новые поля.
- [ ] Проверить реальные значения и отображение Form до/после обновления,
  изоляцию двух форм, переключение backend и graph/edges/jobs после teardown.

Приёмка: обновление native Form причинно проходит через Svet graph, а тесты
проверяют фактический Runtime и освобождение ресурсов.

Реализован opt-in `Form::set_svet_bindings(true)`. Local native lib suite:
117/117; wasm32 check прошёл. Проверены Runtime value, two-way input, batch
dedup, context replacement/reentrant wake, backend switching, stale queued
binding replacement и teardown с сохранённым Control. Document render reentry,
реальные окна и полный Studio end-to-end остаются открытыми.
[Отчёт](benchmarks/native-form-bindings-qualification-2026-11-02.json).

### Этап 2. Довести native Studio и compiler до end-to-end

- [x] Подключить Svet к пути запуска `studio:native -- --svet`:
  `vector-ui/src/main.rs`, retained Document/Runtime и render scheduling.
  Интеграция typed Form сама по себе этот путь не закрывает.
- [ ] Определить ownership callbacks/values/component scopes, wake/redraw,
  замену документа, закрытие окна и несколько окон на одном потоке.
- [ ] Пройти запуск → input → изменение модели → видимый кадр → teardown
  на настоящем native backend с телеметрией и сохранённым результатом.
- [ ] Проверить production Forma/AOT и реальный Vite HMR в браузере:
  несколько mounts, последний unmount, dispose, смена AST и сохранение состояния.

Реализован retained Document scheduler: по signal на поле state, structural
signal и render effect; graph сохраняется при пересборке Runtime. CLI, F7
и hot reload выбирают этот scheduler. Native lib119/119, native-bin check
и wasm32 check проверяются отдельно; реальный оконный end-to-end ещё открыт.
[Отчёт](benchmarks/native-document-scheduling-qualification-2026-11-02.json).

Native window на Apple M4 Max/Metal/Retina2: запуск, ввод, F7 GPU→CPU→GPU,
зависимый текст, hot reload с сохранением state и следующий ввод прошли;
закрытие окна завершило процесс. Исправлены contentsScale Metal-слоя и
перекрытие GPU сохранённым softbuffer-кадром. Три аппаратных GPU-теста прошли.
Это один проверенный сценарий, не полная matrix окон/renderer/IME/ресурсов.
[Отчёт и снимки](benchmarks/native-window-qualification-2026-11-02.json).

Исправлено восстановление после state-dependent render error: последнее
успешное binding mapping принимает исправляющий ввод; pending работа и актуальное
значение toggle сохраняются. Библиотечные120/120, четыре legacy/Svet/switch
сценария, wasm32/native build и hardware GPU3/3 прошли. В живом окне ошибка
остаётся видимой до исправления, затем summary догоняет editor без перезапуска.
Collection reordering при отказах и полная transactional error matrix ещё открыты.
[Отчёт](benchmarks/native-binding-recovery-qualification-2026-11-02.json).

Bindings теперь атомарны при ошибке metadata: staging root scope коммитится
только после успешной обработки всех bindings. Повтор после исправления принимает
сохранённый ввод и одну неприменённую активацию ровно один раз. Проверены обычная
и cached-scene синхронизация, пустые states, legacy/Svet и структурные limits
64/16384 до рекурсивного копирования. Lib123/123, native/wasm32 checks прошли.
Очередь нескольких новых активаций, identity коллекций при reorder, вложенные
Value payloads и цена staging на больших моделях остаются открытыми.
[Отчёт](benchmarks/native-binding-transaction-qualification-2026-11-02.json).

Добавлена упорядоченная очередь активаций retained Runtime: keyboard/pointer/
direct события не теряются при ошибке bindings, весь пакет применяется атомарно.
Проверены 1–3 toggle, порядок selected/toggle на общей модели, повторные отказы,
переполнение4096 и явная отмена через `cancel_ui_binding_activations`.
Lib126/126, native/wasm32 checks прошли. При overflow синхронизация отказывает
до cancel/reload, а не частично применяет prefix. Перенос событий через reload/
reorder, журнал прошлых widget values и latency у лимита ещё открыты.
[Отчёт](benchmarks/native-activation-queue-qualification-2026-11-02.json).

Исправлена маршрутизация bindings при reorder: expanded controls сопоставляются
с живыми widgets по ключам, а root-backed For items заново находятся по stable
key (включая сохранённые key expression/scope при ошибке шаблона). Проверены
активации, повторные toggle и editors через reorder/remove/insert, fresh/cached
maps, отказ на duplicate keys и реальный retained Runtime recovery с key от props.
Lib130/130 и native/wasm32 checks прошли. Nested/derived collections, перенос
через замену документа, конфликт model/widget и стоимость поиска ещё открыты.
[Отчёт](benchmarks/native-binding-key-qualification-2026-11-02.json).

Исправлен конфликт model/widget: неизменённые text/range widgets больше не
затирают новое значение модели. Сравнение идёт с последним принятым snapshot;
неприменённое изменение widget имеет приоритет на своём binding. Snapshot
подтверждается только после успешного атомарного commit (не после отказа).
Проверены повторные sync, range keyboard input, concurrent same-field значение,
reorder с обновлённым нетронутым item и исправление render error из модели.
Lib132/132, allocation5/5, native/wasm32 checks прошли. Это value-snapshot policy,
а не журнал ревизий ввода; large-model performance и прочие conflict cases открыты.
[Отчёт](benchmarks/native-widget-conflict-qualification-2026-11-02.json).

Измерен public Document binding sync в release на8/32/96 editors. Попытка
кеша индексов не дала устойчивого выигрыша и удалена. Оставлено заимствование
данных вместо лишних list clones: на96 editors число аллокаций снизилось≈5%,
сумма requested bytes≈5.6% (повторные счётчики совпали). Время не улучшилось
надёжно; повтор C2 показал +5–8% и требует разбора на спокойном host.
135 lib tests, allocation5/5, native/wasm32 checks прошли. Добавлен воспроизводимый
`binding_sync_bench` и архив сравниваемых исходников. Квалификация производительности
открыта: остаётся≈110 MB суммарных запросов аллокатора на одну sync96 editors.
Следующий приоритет — разделить стоимость preflight/render и bindings, уменьшить
копирование root state/scopes, затем повторить latency/peak-memory на приложении.
[Отчёт](benchmarks/native-binding-copy-qualification-2026-11-02.json).

Разделены render_scene и public sync: построение сцены давало88–93% requested
bytes. Убраны глубокие копии, чьи props/children сразу заменялись, и ненужная
копия measuring root (legacy Frame→Column conversion сохранена). На96 editors
render bytes97.1→83.8MB (-13.7%), полная sync110.3→96.9MB (-12.1%), allocation
calls874257→774668 (-11.4%); повторы совпали по счётчикам. Latency всё ещё
неустойчива, FPS выигрыш не подтверждён. Lib135/135, allocation5/5, Metal GPU3/3,
native/wasm32 checks прошли. Тяжёлые state/scope snapshots остаются открытыми.
[Отчёт](benchmarks/native-render-phases-qualification-2026-11-02.json).

Убрана вторая полная копия instance.props при instantiate: восстанавливаются
только явные ключи, перекрытые выбранной группой. Приоритет props и доступ
к props.__state проверены. На96 editors полная sync96.94→90.39MB requested
bytes (-6.8%), allocation calls774668→725708 (-6.3%). Оба локальных timing
повтора быстрее, но app FPS/p95 ещё не квалифицированы. Lib135/135,
allocation5/5, Metal GPU3/3, native/wasm32 checks прошли.
[Отчёт](benchmarks/native-instance-overrides-qualification-2026-11-02.json).

Второй frozen HTTP soak LPJBKt завершён:918440 requests/7200008ms,
request/cleanup failures0, graph0, forcedGC0, все четыре memory budgets прошли.
Хэши harness/fixture/runtime20 независимо сверены. Latest dist отличается
в advanced/forma/WASM/embedded binary; его квалификация этим не закрыта.
Запущен отдельный latest2h natural прогон09qhPH (20/20 files совпали с dist).
[Final LPJBKt](benchmarks/http-natural-current-2h-qualification-2026-11-02.json),
[новый запуск](benchmarks/http-natural-latest-run-2026-11-02.json).
Уточнение benchmark-контекста: поздние copy/render замеры выполнялись после
завершения HTTP measurements; старый tool handle не доказывал активную нагрузку.
[Коррекция](benchmarks/native-benchmark-http-overlap-correction-2026-11-02.json).

Наследуемые state/props при scene expansion теперь borrowed до локальной записи
(Cow); Let/node-local overlay отделяются, публичные snapshots остаются owned.
Проверены вложенные scopes, sibling isolation и сохранность старого snapshot
после изменения модели. На96 editors sync requested bytes90.39→83.84MB (-7.2%),
allocation calls725708→677030 (-6.7%). Lib136/136, allocation5/5, Metal GPU3/3,
native/wasm32 checks прошли. Эти замеры сопровождались подтверждённым active
HTTP churn; latency не квалифицирована. Выходные snapshots/For locals всё ещё
копируют модель, large-model readiness остаётся открытой.
[Отчёт](benchmarks/native-scope-cow-qualification-2026-11-02.json).

### Этап 3. Закрыть оставшиеся дефекты реактивности и lifecycle

- [ ] Закрыть failing cross-copy dependency между разными cores (probe 11/12):
  dynamic dependencies, batches, cycles, disposal/reset и coherent observers.
  Если сценарий исключается из контракта, явно зафиксировать отказ и ограничение.
- [ ] Разобрать непрерывный high-rate history burst с native-only контролем;
  paced pass не считать закрытием непрерывного сценария. Проверить конкурирующие routers.
- [ ] Проверить browser/Edge async carriers: nested/concurrent await,
  rejection/abort, streaming, explicit capture и поздние callbacks после teardown.
- [ ] Дополнить lifecycle проверки native forms APIs, retained DOM/receiver,
  callbacks и нескольких одновременно живых cores/roots.

### Этап 4. Завершить память, SSR и транспорт

- [x] Получить и разобрать final report второго frozen HTTP soak; сверить
  manifest/hashes и четыре memory budgets. Не считать progress-файл финалом.
- [ ] Повторить длительный прогон на окончательном runtime: LPJBKt предшествует
  route-table/AOT/native ABI исправлениям и не квалифицирует их.
- [ ] Установить исходную причину отказа прежнего ~24-минутного прогона.
- [ ] Провести длительную нагрузку с обоими codecs, gzip, slow clients,
  крупными patches, disconnect/abort и насыщением сокетов; проверить queues/recovery.
- [ ] Раздельно оценить heap/RSS/external/ArrayBuffers, WASM arena и GPU;
  исследовать рост/retainers и стоимость ограничения арены относительно baseline.
- [ ] Проверить конкретный deployment: proxy/CDN, TLS/HTTP2/3, cancel/FIN,
  mixed server/client versions, malformed frames и отказ до DOM mutation.

### Этап 5. Расширить браузеры, Edge и аппаратный renderer

- [ ] Дополнить матрицу другими браузерами и deployed Edge providers,
  natural GC и stable Miniflare после beta.
- [ ] На физических GPU/backends проверить repeated device loss/recovery,
  cancellation, resize/DPR, shared device, resident buffers и renderer teardown.
- [ ] Сравнить projection/picking/clipping/depth с эталонами для перекрытий,
  невыпуклой геометрии и граничных координат; сохранить визуальные результаты.

### Этап 6. Измерить приложения и подготовить проверяемую доставку

- [ ] Выполнить A/A и A/B реальных приложений: startup, latency/p95,
  frame time, long tasks, transfer bytes, queue/compute/readback и память.
  Повторить без конкурирующей нагрузки и на другом оборудовании.
- [ ] Собрать единую матрицу implemented/validated/partial/unsupported
  по функциям и средам, со ссылками на актуальные raw evidence.
- [ ] Проверить remote CI, чистую установку, archive/exports/types и цепочку
  Rust → embedded WASM → installed package; закрепить обязательные gates.
- [ ] Актуализировать README/DEVELOPMENT по окончательному поведению.
- [ ] Commit/merge/push/release выполнить отдельным запрошенным этапом,
  после проверки состава изменений и сохранения посторонней работы.

Общее правило приёмки: воспроизводимый сценарий → исправление/регрессия →
проверка реального backend или установленного пакета → raw evidence с hashes
и ограничениями. Полный suite повторять после затронувших runtime изменений;
документационные правки проверять отдельно. Отдельный зелёный тест, микробенчмарк
или forced-GC прогон не закрывает весь этап.

## Ближайший план оставшихся работ — 2026-11-02

Этот раздел обновляет приоритеты очереди ниже. Галочка означает проверенный
ограниченный результат; P1–P4 в целом остаются открытыми.

Подтверждённая база: 1297 JS-тестов / 99 файлов, TypeScript, installed-package
gate с Edge inspector GC и HTTP actions 4/4. Независимые browser bundles прошли
9/9 проверок. Явный синхронный выбор core согласован между совместимыми копиями;
captured callback работает после завершения child carrier, пока жив его owner.
[Последняя квалификация](benchmarks/cross-copy-captured-core-qualification-2026-11-02.json).
Это не квалификация разных версий, разных realms или автоматического контекста
через пользовательский await.

### 1. Закрыть ближайшие lifecycle и cross-copy сценарии

- [x] Проверить в Edge реальное DOM-событие сохранённого обработчика успешного
  child после закрытия carrier: callback копии B меняет signal и создаёт computed
  копии A; до завершения parent работает, после teardown не вызывается.
  Незахваченное позднее продолжение должно по-прежнему получать отказ.
  Приёмка: воспроизводимый отказ на предыдущем runtime, успех на исправленном,
  стабильный и beta workerd profiles, GC с retained DOM и installed-package gate.
  Все проверки прошли; 24/24 целевых объекта собраны.
  [Квалификация](benchmarks/edge-retained-child-qualification-2026-11-02.json).
- [x] Проверить встроенные Suspense/ErrorBoundary между копиями: видимость
  pending/error от foreign resource, восстановление и teardown. Сначала fixture:
  совместное использование пользовательского Context не доказывает этот контракт.
  Исправлены встроенные protocol Symbols; оба направления, pending/completion/
  cancellation и rejection/retry прошли в JSDOM и installed gate (4/4).
  Edge workerd/LinkeDOM: 4/4 на stable и beta/November, installed gate прошёл.
  Дополнительно исправлен retry batch в чужом ambient core: content больше
  не создаётся дважды. Полный suite 1294/98. Chrome156 native DOM: 4/4
  на source production bundles и 4/4 на installed production bundles.
  [Browser квалификация](benchmarks/browser-copy-boundaries-qualification-2026-11-02.json).
  Другие браузеры и настоящий browser async-context carrier ещё открыты.
  [Edge квалификация](benchmarks/edge-copy-boundaries-qualification-2026-11-02.json).
  [Квалификация](benchmarks/cross-copy-boundaries-qualification-2026-11-02.json).
- [ ] Реализовать и квалифицировать автоматические зависимости между разными
  native cores либо явно закрепить ограничение публичного контракта. Сейчас
  cross-copy tracking probe проходит 11/12 сценариев; отдельный failing case
  сохраняется открытым. Для реализации нужны batching, dynamic dependencies,
  disposal/reset и циклы; общей JS-подписки недостаточно.

- [x] Исправить built-in RouterContext между копиями и атомарность navigate
  из другого ambient core. Installed probe 3/3: useRouter/Link в обоих
  направлениях, route cleanup, teardown и согласованная публикация URL.
  JS1295/99, TypeScript, package с Edge GC и HTTP actions4/4 прошли.
  Chrome156 installed production bundles: 3/3 native DOM/Link/URL checks
  прошли. Native back/forward, denied-Back compensation и redirect с foreign
  observers также прошли в обоих направлениях (общая matrix5/5).
  [History квалификация](benchmarks/browser-copy-history-qualification-2026-11-02.json).
  Rapid bursts: isolated A→B и B→A по11/11 прошли (по6 серий), но общий
  прогон16/17 завершился setup timeout последнего сценария. Причина открыта;
  Timeline повторил16/17: после финального go(-2) нет popstate за3с.
  Native-only replay199 команд завершился успешно; простой лимит числа/темпа
  команд этим контролем не подтверждён. Следующий шаг — сравнить interleaving
  popstate/команд и синхронные компенсации на границах сценариев.
  [Диагностика](benchmarks/browser-copy-burst-timeline-analysis-2026-11-02.json).
  Дополненный native replay с двумя обычными Link-навигациями воспроизводит
  отсутствие финального popstate без Svet (201 команд). Fixture больше
  не запускает эти побочные навигации. Полная matrix17/17 проходит с явной
  паузой между сценариями после плотной серии history-команд; непрерывный
  high-rate прогон остаётся открытым.
  [Контроли](benchmarks/browser-copy-burst-controls-2026-11-02.json).
  [Partial evidence](benchmarks/browser-copy-burst-qualification-2026-11-02.json).
  Edge router, общий burst-прогон и конкурирующие routers ещё открыты.
  [Browser квалификация](benchmarks/browser-copy-router-qualification-2026-11-02.json).
  [Квалификация](benchmarks/cross-copy-router-qualification-2026-11-02.json).

- [x] Убрать выявленную стоимость пустого shared-core stack: guard длины
  перед чтением top. Только wasm-bridge.js отличается в A/B; synthetic
  untrack median ratio0.29–0.43, с шумными AA-контролями. JS1295/99, types,
  package с Edge GC и actions4/4 прошли. Это не performance gate приложений;
  повтор без конкурирующей нагрузки и multi-copy замеры остаются открытыми.
  [Квалификация](benchmarks/core-stack-fastpath-qualification-2026-11-02.json).

- [x] Освободить ErrorBoundary closure при сохранённом retry после teardown.
  GC before4/4 cores retained → after0/4, controls4/0; package gate требует
  этот сценарий. JS1295/99, types, Edge/package и actions4/4 прошли.
- [x] Исправить удержание disposed Router: before4/4 cores/memory → after0/4
  при сохранённых router objects и guard-removers. Terminal readers/methods
  сохраняют snapshots, поздний navigate=false; package gate держит12 ссылок
  и собирает8 cores+8 Memory. JS1296/99, types, package/Edge GC, actions4/4
  прошли. Предполагаемый1 node оказался inactive effect pool; public telemetry
  подтверждаетgraph0 до и после исправления. Chrome156 installed runtime:
  history17/17 (paced), natural GC под bounded pressure собирает16 cores+
  16 Memory при40 сохранённых ссылках, controls4→0. Edge/другие браузеры
  и длительные bounds ещё открыты.
  [Browser квалификация](benchmarks/retired-ui-browser-qualification-2026-11-02.json).
  Local Edge stable и beta/November:16 requests,40 сохранённых ссылок,
  before32/32 → after0/32 core/Memory targets; controls4→0, inspector ACK.
  Installed-package gate требует этот сценарий. Natural cadence, deployed
  providers и Edge History API этим не квалифицированы.
  [Edge квалификация](benchmarks/edge-retained-ui-qualification-2026-11-02.json).
  [Квалификация](benchmarks/retained-router-qualification-2026-11-02.json).
  [GC evidence](benchmarks/retained-ui-callbacks-qualification-2026-11-02.json).

- [x] Восстановить замену options.routes у живого Router после выделения
  чистого matcher. После dispose используется последняя прочитанная таблица,
  options больше не читается/не удерживается. Installed4/4, JS1297/99
  (one worker, лимит benchmark5s сохранён), types/actions/package+Edge GC
  прошли. Chrome156 installed bundles: before17 pass/2 fail → after19/19
  (paced), включая foreign RouterView и terminal matcher в обоих направлениях.
  Длительный soak этой редакции ещё открыт.
  [Browser квалификация](benchmarks/router-table-browser-qualification-2026-11-02.json).
  [Квалификация](benchmarks/router-route-table-qualification-2026-11-02.json).

- [x] Проверить Rust → embedded → installed WASM локально:53 native tests,
  две независимые release/simd128 сборки одинаковы,54320 bytes; source embed,
  exports и установленный asset/runtime совпадают. Валидный byte-different
  control отвергается. CI guard добавлен до перегенерации embed и в installed
  gate через SVET_REBUILT_WASM; local package+Edge gate прошёл. Remote Linux
  CI/cross-host воспроизводимость ещё не проверены; общий P4 остаётся открытым.
  [Квалификация](benchmarks/rust-wasm-provenance-qualification-2026-11-02.json).

- [x] Исправить AOT .ui module registry: production больше не удерживает
  созданные/не mounted instances; HMR регистрирует только успешные mounts,
  сохраняет entry до последнего unmount и удаляет при dispose. Before3 fail/
  8 targets retained → after3/3,0/8; controls4→0. JS1297/99, types, actions4/4,
  package с Rust provenance и Edge GC прошли. Реальный Vite HMR/browser ещё открыт.
  [Квалификация](benchmarks/forma-aot-lifecycle-qualification-2026-11-02.json).

- [x] Убрать общий unsynchronized native C-ABI graph: native calls теперь
  используют thread-local RefCell; owned ReactiveGraph предназначен для
  отдельных окон на одном потоке. Before child видел parent node; after55 Rust
  tests, включая8×256 concurrent flush cycles, прошли. Новый WASM re-embedded,
  две сборки одинаковы; JS1297/99, types/actions/package+Rust provenance+Edge GC
  прошли. Реальный native Studio adapter остаётся открытым.
  [Квалификация](benchmarks/native-abi-thread-qualification-2026-11-02.json).

- [x] Добавить native owned graph context для будущего window adapter:
  identity/lifetime-safe handles, stale jobs/reset/reuse refusal, dynamic
  tracking, nested computed, cycle guard и batch/unwind scopes. Rust66/66,
  native binding projection example и wasm32 example compile прошли.
  WASM/runtime20 файлов byte-equal прежней1297/99-qualified редакции.
  CI запускает native example; подключение к Forma native model/renderer,
  callbacks/values и component ownership остаётся открытым.
  [Квалификация](benchmarks/native-owned-context-qualification-2026-11-02.json).

### 2. Завершить длительную проверку памяти и транспорта

- [x] Получить final report frozen двухчасового natural-GC HTTP
  soak, проверить неизменность runtime/harness и анализировать все четыре окна.
  Progress-файл не считать результатом. Бюджеты роста: heap 16 MiB, RSS 64 MiB,
  external 64 MiB, ArrayBuffer 16 MiB; сохранить raw и вывод анализатора.
  [Метаданные запуска](benchmarks/http-natural-frozen-run-2026-11-02.json).
  Final: 1 232 608 requests /7200032ms, без request/cleanup failures,
  forcedGC0, graph0, hashes unchanged. Все4 memory budgets прошли по
  post-warmup window envelopes. Это ранняя baseline, не текущий runtime.
  [Квалификация](benchmarks/http-natural-frozen-2h-qualification-2026-11-02.json).
- [ ] Установить исходную причину отказа предыдущего прогона после ~24 минут;
  исправленная primary/cleanup диагностика сама по себе причину не устраняет.
- [ ] Повторить длительную квалификацию на окончательном новом runtime:
  текущий frozen запуск относится к более раннему installed artifact и не
  проверяет новые shared-domain/owner/core-scope изменения.
  Frozen LPJBKt завершён и проанализирован:918440 requests, все4 бюджета прошли.
  Актуальная редакция отличается4 runtime files; отдельный latest2h прогон09qhPH запущен.
  [Метаданные](benchmarks/http-natural-current-run-2026-11-02.json).
- [ ] Расширить нагрузку: оба codecs, gzip, медленные клиенты, крупные patches,
  disconnect/abort, насыщение сокетов; проверить bounded queues и восстановление.
  Затем проверить выбранный production proxy/CDN, TLS и HTTP2/3.

### 3. Расширить матрицу сред и renderer

- [ ] Другие браузеры и реальные async-context carriers: nested/concurrent await,
  rejection, abort, streaming, capture и teardown. Проверить native forms APIs.
- [ ] Развёрнутые Edge providers, natural GC, stable Miniflare после beta;
  отдельно cancel/FIN propagation через proxy и mixed-version deployments.
- [ ] Физические GPU/backends: queue cancellation, повторные device loss/recovery,
  resize/DPR, resident buffers и renderer lifetime. Сопоставить projection,
  picking, clipping/depth и перекрывающуюся/невыпуклую геометрию с эталонами.

### 4. Проверить пользовательские приложения и подготовить доставку

- [ ] Реализовать native Svet adapter: текущий --svet Studio ведёт в общий
  vector paint_native; owned core теперь подключён к typed Form и retained
  Document scheduling. Ещё нужны полный
  graph/renderer/lifetime bridge и доказанный end-to-end на native backend.
  [Аудит](benchmarks/native-studio-svet-integration-audit-2026-11-02.json).
- [ ] Пройти production end-to-end Forma/compiler и native studio: запуск,
  взаимодействие, обновление и teardown.
- [ ] Выполнить A/A и A/B на реальных приложениях: startup, p95, frame time,
  long tasks, transfer bytes, очередь/readback и память; повторить на другом GPU.
- [ ] Свести supported/partial/unsupported по функциям и средам с актуальными
  артефактами; закрепить CI, чистую установку, exports/types и соответствие
  Rust → embedded WASM → installed archive. Обновить документацию.
- [ ] Commit/push/release — отдельный запрошенный этап после проверки состава
  изменений и сохранения посторонней работы.

Для каждого пункта: fixture и критерий приёмки → исправление и регрессия →
проверка установленного пакета → raw evidence и ограничения. Новые дефекты
корректности и утечки имеют приоритет перед измерениями производительности.

## Подробная очередь и история на 2026-11-01

Эта историческая очередь дополняет актуальный рабочий план выше. Здесь перечислена
оставшаяся работа, а реализованные части указаны как исходная база. Узкая проверка
не закрывает весь пункт. Проверки привязаны к конкретным логам и артефактам.

Базовая завершённая квалификация request/render tokens: **1286 тестов / 97 файлов**, TypeScript,
installed-package gate с workerd (8 concurrent events без keepalive) и HTTP
actions4/4 прошли. [Qualification](benchmarks/edge-gc-qualification-2026-11-01.json)
связывает source/runtime hashes, snapshots, installed и GC evidence.
[JS лог](benchmarks/edge-gc-final-suite-2026-11-01.txt),
[package лог](benchmarks/edge-gc-qualified-package-2026-11-01.txt),
[actions лог](benchmarks/edge-gc-final-actions-2026-11-01.txt).
Retained-error GC: 32 конкурентные операции, 96 объектов, 0 удержанных при
сохранённых errors/handles ([отчёт](benchmarks/ssr-timeout-retention-final-2026-11-01.json)).
Это короткая Node-проверка, не доказательство длительного плато или browser/Edge GC.

### P0 — завершить текущую правку и проверки времени жизни

- [x] Исправить зависание timeout Web reader.closed при8 workerd fetch events.
  Adapter удерживает native stream до завершения producer teardown и затем
  снимает ссылку. Контролируемый откат возвращает отказ на той же fixture.
  Без keepalive прошли60с:1464 events,23424 DOM cycles,11712 streams и столько
  же string renders,1464 timeouts. Direct HTTP5с:416 events. Installed gate
  требует обычный readable transport/8 concurrent requests и отвергает diagnostics.
  GC432 objects collected, включая16 discarded unstarted streams.
  [Qualification](benchmarks/edge-stream-lease-qualification-2026-11-01.json).
  Точный native heap-retainer path не снят; Edge GC и многочасовые границы
  не следуют из этого результата и остаются открытыми.

- [x] Добавить регрессию: пользовательский `Error.prepareStackTrace` бросает
  исключение при фактическом timeout; запрос всё равно завершается типизированным
  `SsrTimeoutError`, таймер и request graph освобождаются.
- [x] После последней правки последовательно выполнить build:package, TypeScript,
  полный JS suite, installed-package gate с optional Edge, HTTP actions и
  `git diff --check`; сохранить логи и привязать их к исходникам.
- [x] Провести конкурентный deadline churn: success/timeout/caller abort/child
  timeout, отмена части запросов при живых соседях, сохранённые errors/handles.
  Проверять исходные abort reasons, cleanup ровно один раз, отсутствие поздних
  записей, остаточных timers/listeners и удержанных cores/WASM memory.
  Выполнена Node Web-stream часть: 60с, 16 live requests, оба codecs,
  8336 операций (по 2084 complete/cancel/abort/timeout), 50016 объектов собраны
  при сохранённых terminal errors/handles; cleanup1, graph0 и caller listeners0.
  [Raw](benchmarks/ssr-deadline-churn-60s-2026-11-01.json).
  Child scopes: 60с, 21832 parents / 87328 children, 43664 late refusals,
  152824 объектов собраны при сохранённых errors; активных таймеров0 после
  каждой группы. [Raw](benchmarks/child-deadline-churn-60s-2026-11-01.json).
  Web-stream timer accounting отдельно прошёл160 операций:
  [raw](benchmarks/ssr-deadline-timers-2026-11-01.json).
  Оба коротких варианта включены в package gate. Длительные HTTP/Edge нагрузки
  и многочасовое плато остаются в P1.
- [ ] Расширить GC-проверки на браузерные receiver/DOM и Edge lifecycle.
  Сделано: локальный workerd /24 requests /6 paths /2 codecs,140 целевых
  объектов собраны при сохранённых terminal pairs/errors; controls4 до release,
  0 после. Inspector GC команды подтверждены. Retained host request/render tokens
  больше не держат core/publisher; зависший view также закрывает render session.
  [Qualification](benchmarks/edge-gc-qualification-2026-11-01.json).
  Owner-channel tokens также исправлены: установленный Node-пакет собирает
  17 cores при удержанных 17 request / 13 owner / 11 render tokens;
  [package log](benchmarks/owner-token-package-2026-11-01.txt).
  Полный JS suite: 1286 тестов / 97 файлов;
  [лог](benchmarks/owner-token-qualified-suite-2026-11-01.txt).
  Осталось: другие providers/браузеры, естественная GC cadence и длительные
  RSS bounds. Этот общий пункт пока открыт.
  Chrome156 browser GC: 32 terminal receiver containers и 16 select signals
  собраны при сохранённых handles/detached select DOM. Live controls4 до release,
  0 после; graph0. Два прогона, natural GC под bounded allocation pressure,
  не CDP forced GC. [Raw](benchmarks/browser-retention-gc-2026-11-01.json).
  Открыты SSR request-core/browser carrier, Edge GC и другие browsers.
  Node probes iterator/receiver/readable уже есть; они не доказывают browser GC.
  Edge DOM lifecycle квалифицирован в локальном workerd+LinkeDOM: два
  concurrent events / 32 cycles, keyed identity/index updates, removed scopes,
  Show, SVG, escaped text и event teardown. Graph0/cleanup подтверждены;
  это не GC-доказательство и не deployed-provider matrix.
  [Raw](benchmarks/edge-dom-lifecycle-qualified-2026-11-01.json).
  Forms выявили getter-only select.value в LinkeDOM; model исправлен через
  option.selected. Text/checkbox/textarea/single-select SSR прошли в workerd;
  [raw](benchmarks/edge-forms-after-2026-11-01.json). Multiple select, reset,
  form events и остальные DOM APIs требуют отдельной проверки.
  Исправлен select value без model: initial static/reactive значение применяется
  после options, getter-only host поддержан. Suite1281/96, package с Edge,
  actions4/4 прошли; [raw](benchmarks/select-value-edge-2026-11-01.json).
  Поздние options исправлены: mutation checkpoint обновляет selection при
  вставке/удалении/перестановке и text/value changes; cached value не вызывает
  accessor повторно. Unmount/reset защищены. Suite1285/96 и package с Edge
  прошли; [raw](benchmarks/select-options-edge-2026-11-01.json).
  Multiple-select host path исправлен и квалифицирован: missing multiple/
  selectedOptions reflection, initial/reactive выбор, change roundtrip и HTML
  с несколькими selected options. Suite1286/97, package с Edge прошли.
  [Raw](benchmarks/edge-multiple-after-2026-11-01.json).
  Native form reset/validation в Edge и естественная GC cadence остаются открытыми.

Критерий закрытия: регрессии входят в package gate, полные проверки проходят
на последней редакции, raw результаты и ограничения записаны в DEVELOPMENT.md.

### Ближайшая последовательность работ

1. [x] Завершить owner-token этап: финальные hashes и raw Node/Edge отчёты
   сведены в [qualification receipt](benchmarks/owner-token-qualification-2026-11-01.json),
   HTTP actions4/4 и TypeScript прошли; DEVELOPMENT/матрица обновлены.
2. [x] Закрыть transport error matrix бинарника workerd2026-11-01: producer
   failure до Response, EOF refusal, cleanup и fresh-request recovery проверены.
   [Raw](benchmarks/edge-transport-new-qualified-2026-11-01.json).
   Controller/compatibility date закреплены на July; новая Miniflare/compat date открыты.
3. Двухчасовой Node HTTP natural-GC soak завершился отказом после189144
   запросов/~24мин: timeout cleanup-запросаforget. Хэши не изменились;
   причина исходного отказа пока не установлена. Сохранённый final raw:
   benchmarks/http-natural-2h-2026-11-01.json. Primary/cleanup диагностика исправлена и проверена controlled503/503,
   natural/forced pilots. Повторить длительную квалификацию после установленного
   package gate новой редакции прошёл; исходная причина24-минутного отказа открыта.
   Повтор запущен на frozen harness + installed package, все20 JS/WASM files
   зафиксированы: http-natural-frozen-2h-2026-11-02.json.progress.json.
   [Harness receipt](benchmarks/http-frozen-harness-qualification-2026-11-02.json). Полный HTTP/Edge soak с обоими
   codecs/gzip, насыщением сокетов и длительными memory bounds остаётся открыт.
4. Native browser constraint-validation fixture прошла13/13 на текущих
   исходниках; reset regressions30/30. Installed production matrix21/21,
   189 assertions также прошла. Квалифицировать другие browsers и Edge native APIs; расширить browser/Edge lifecycle matrix; затем выполнить GPU/renderer и Forma/native studio проверки из P2–P3.
5. Собрать итоговую матрицу готовности и обязательные CI gates из P4.

Для каждого шага: сначала воспроизводимый сценарий и критерий приёмки, затем
реализация/регрессия, проверка установленного пакета и raw evidence. Найденный
дефект корректности или утечка получает приоритет перед оптимизациями.
Commit/push/release выполняются отдельным запрошенным шагом.

### P1 — длительная нагрузка и transport/SSR совместимость

- [x] Квалифицировать workerd2026-11-01 binary override на закреплённом July
  controller/compat date: fetch rejection до Response и truncated EOF различаются,
  producer failure подтверждён, cleanup1/graph0 и fresh-request recovery проходят
  для обоих codecs. [Raw](benchmarks/edge-transport-new-qualified-2026-11-01.json).
- [x] Проверить доступную Miniflare5 beta с matching October workerd/date и
  November binary/date override; официальный v4-options converter подключён.
  [Raw](benchmarks/edge-beta-november-2026-11-01.json). Stable default сохранён.
- [ ] Повторить beta matrix после stable-релиза; отдельно квалифицировать remote
  deployment, многочасовые границы и natural GC cadence.

- [ ] Провести длительный конкурентный HTTP soak с медленными клиентами,
  крупными patches, disconnect, gzip и обоими codecs. Наблюдать реальную
  backpressure/насыщение сокета, bounded queues и восстановление после abort.
  Добавлена concurrent16-case matrix: оба codecs, gzip on/off, complete, shell
  disconnect, pending-patch disconnect/host abort; 94 rounds/1504 requests прошли.
  [Raw](benchmarks/transport-matrix-qualified-2026-11-01.json).
  Существующие 60-секундные loopback-прогоны не доказывают многочасовое плато.
- [ ] Измерять отдельно JS heap, RSS, external/ArrayBuffer, WASM и GPU память;
  зафиксировать прогрев, нагрузку, интервалы и допустимый рост. Для роста снять
  heap/retainer evidence; оценить временные копии и стоимость ограничения арены.
- [ ] Проверить production proxy/CDN и TLS/HTTP2/3 на конкретном deployment target.
  Отдельно разобрать непередачу cancel/FIN через закреплённый Miniflare proxy:
  host deadline ограничивает lifetime, но propagation пока не исправлен.
- [ ] Квалифицировать mixed server/client deployments, несовместимые версии,
  capabilities, CORS и malformed frames через настоящий HTTP и браузер.
  Negotiation helpers и wire v1 уже реализованы; ошибка должна возникать до DOM mutation.
- [ ] Проверить progressive HTTP latency, peak memory и CPU для identity/ref codec
  с gzip и без него. Сохранить DOM/input identity и cross-boundary coherence;
  codec принимать по workload, а не по одному уменьшению bytes.
- [ ] Расширить Edge DOM/churn matrix, проверить развёрнутый host, определить
  поддерживаемые DOM API. Затем квалифицировать другие Edge providers и настоящий
  browser async-context carrier: nested/concurrent await, rejection, abort,
  streaming, teardown. Неподдерживаемые среды явно отклонять/документировать.

Критерий закрытия: воспроизводимые отчёты на названных средах, bounded resources,
изоляция запросов, подтверждённые terminal paths; непроверенные среды не объявлены поддержанными.

### P2 — браузеры, GPU, async-контракт и renderer correctness

- [ ] Расширить [BROWSER_MATRIX.md](BROWSER_MATRIX.md): дополнительные браузеры,
  физические GPU/backends, resize/DPR, queue cancellation, повторные device loss/recovery.
  Разделять software и hardware результаты.
- [ ] Проверить capture после await и
  tracking между несколькими копиями движка; закрепить контракт тестами.
  Cross-copy untrack исправлен на исходниках и проверен двумя независимыми
  сборками: suppression/new observers/throw/reset. Installed package gate с Edge/GC прошёл; длительный soak завершился отказом; foreign signal dependency остаётся
  failing/open. [Installed partial receipt](benchmarks/cross-copy-weak-installed-qualification-2026-11-02.json).
  Протоколv2: локальный hot path + слабый discovery; performance gate пока открыт.
  GPU fallback thenables проверены; sync double-getter defect исправлен,
  JS1293/98 + TypeScript/package с Edge прошли.
  [GPU receipt](benchmarks/gpu-thenable-qualification-2026-11-01.json).
  Resource PromiseLike контракт и hostile-thenable lifecycle проверены:4 новых
  регрессии, JS1290/98, TypeScript и package с Edge прошли.
  [Receipt](benchmarks/resource-thenable-qualification-2026-11-01.json).
- [ ] Сопоставить 2D/GPU projection, picking, clipping/depth на невыпуклых,
  перекрывающихся объектах и граничных координатах; сохранить визуальные эталоны.

Критерий закрытия: явная матрица supported/partial/unsupported с артефактами,
регрессиями и проверенным освобождением GPU/DOM ресурсов.

### P3 — производительность приложений и интеграция Forma

- [ ] Выполнить воспроизводимые A/B и A/A на пользовательских приложениях:
  startup, p95/frame time, long tasks, bytes и память. Повторить в отдельном
  запуске и на другом оборудовании; отдельно измерить shallow-untrack regression
  и цену lifecycle guards.
- [ ] Проверить compiler/Forma и native studio end-to-end на актуальных production
  артефактах; отдельно обычный compiler mode, запуск, взаимодействие и teardown.

Критерий закрытия: наблюдаемое поведение совпадает с эталоном, выигрыш выдерживает
контроли и повторные запуски; отсутствие регрессий подтверждено, а не выведено из FPS.

### P4 — готовность и доставка

- [ ] Собрать итоговую матрицу функций и сред с implemented/validated/partial/
  unsupported, ссылками на raw evidence и оставшимися ограничениями.
- [ ] Закрепить обязательные CI gates, чистую установку, exports/types,
  Rust → embedded WASM → installed archive и воспроизводимость артефактов.
- [ ] Актуализировать README/DEVELOPMENT и инструкции запуска/диагностики.
- [ ] По отдельному запросу подготовить commit/push/release; перед доставкой
  проверить branch/remote, состав изменений и доставленный артефакт.

Критерий закрытия: проверенная чистая установка и согласованная матрица готовности.
Публикация сама по себе не закрывает незавершённые P0–P3.

### Уже сделано — база для оставшейся работы

Ниже сохранены результаты отдельных этапов. Старые числа и формулировки
«ещё открыто» читаются вместе с более поздними результатами и очередью выше.

Stream receiver retention исправлен: публичный handle хранит только очищаемую
ссылку на внутренний receiver. Teardown разрывает её до очистки resumed bindings.
GC probe с сохранёнными handles прошёл для dispose/reset/replacement/protocol error:
0/32 удержанных detached containers; контроль также 0/32. Исходный отказ 32/32
сохранён. Raw: [stream-receiver-retention-after](benchmarks/stream-receiver-retention-after-2026-11-01.json).
Probe добавлен в installed-package gate. Полный JS suite: 1243/88 passed.
Iterator, complete без dispose, request-core retention и browser GC ещё открыты.

Terminal stream iterator retention исправлен: сохранённые handles после
полного чтения/return/error не удерживают view/captured objects/core/memory/node.
До исправления: 24 view/captures и 8 core/memory; после — 0 всех типов.
Raw: [stream-iterator-retention-after](benchmarks/stream-iterator-retention-after-2026-11-01.json).
Полный suite 1244/88 прошёл; concurrent next regression включена.

External abort без terminal чтения квалифицирован: отмена до запуска и после
shell освобождает view, поздний next один раз возвращает исходную ошибку.
GC probe40 handles: 0 удержанных объектов, JS1245/88 passed.
Raw: [stream-abort-retention-after](benchmarks/stream-abort-retention-after-2026-11-01.json).
Незапущенный неотменённый iterator сохраняет view для будущего вызова.

Concurrent HTTP streaming soak60с прошёл: 8 clients, 11952 requests,
7968 complete /3984 disconnect,15936 writer drains; после каждого запроса
nodes0/cleanup1. Heap/RSS/external/arrayBuffers записаны, sample ring32.
Raw: [stream-http-soak](benchmarks/stream-http-soak-60s-2026-11-01.json).
Это delayed fetch reads на loopback, не подтверждение TCP saturation/proxy
buffering или многочасового плато.

Local HTTP proxy matrix4/4 прошла: throttled relay и full-buffering proxy,
complete/disconnect в обоих режимах. Streaming даёт ранний shell, buffering
его скрывает; оба disconnect закрывают origin с nodes0/cleanup1.
Raw: [stream-proxy](benchmarks/stream-proxy-qualified-2026-11-01.json).
Production proxy/CDN, TLS/HTTP2/3 и длительное socket saturation открыты.

Opt-in HTTP version negotiation реализован публичными helpers:
server выбирает v1 до SSR, client проверяет headers до body/receiver.
Native HTTP/JSDOM7 checks прошли; JS1247/89. Raw:
[stream-http-negotiation](benchmarks/stream-http-negotiation-2026-11-01.json).
Включение policy в каждый production route остаётся задачей host; mixed real
versions в браузере, CORS и capabilities ещё открыты.

Перед оптимизацией исправлена topology validation boundaries: crossing ranges
и noncanonical IDs отклоняются; shell/patch fragments проверяются до замены
старого содержимого. Регрессия воспроизведена до fix; JS1250/89 passed.
Installed gate содержит crossing-shell regression. Final HTML duplication
этой правкой не уменьшено.

Optional HTML-reference transport codec реализован: decoded wirev1 не меняется,
повтор предыдущего HTML заменяется reference+prefix/suffix при меньшем JSON.
Fixture plain520757→encoded260752 bytes, complete260300→295; records exact,
input identity/resume сохранены; JS1255/90 passed. Raw:
[stream-codec](benchmarks/stream-codec-2026-11-01.json).
Это opt-in transport, не auto-negotiated формат. Final serialization CPU,
реальный HTTP/browser codec и paired latency/memory ещё открыты.

HTTP codec negotiation и installed browser integration квалифицированы:
explicit request/response codec header, identity default, unknown offer406 до SSR,
identity client отвергает encoded ответ до body. Installed HTTP4 checks и
production Chrome7/7 прошли, bytes248/132403/301; runtime18 JS hash match.
JS1256/90 и package9/9 passed. Evidence:
[stream-codec-browser](benchmarks/stream-codec-browser-qualification-2026-11-01.json).
CORS, malformed-frame browser paths и mixed deployments остаются открытыми.

Isolated paired transport benchmark выполнен:72 processes/864 measured
roundtrips,3 workloads,interleaved plain/codec/gzip/codec-gzip и A/A.
Varied codec+buffered-gzip processing−26…−31% к gzip (A/A<1%), repeated+19…25%
при102 bytes экономии. Без gzip codec добавляет CPU; universal win не заявляется.
Лишняя повторная сериализация chunk удалена; JS1256/90 passed.
Raw: [codec-paired](benchmarks/stream-codec-paired-qualified-2026-11-01.json).
Память только sampled, gzip buffered: progressive HTTP latency/peak/plateau
остаются открытыми.

Progressive gzip fixture квалифицирован через native HTTP и production browser:
identity-gzip4 checks/codec-gzip5 checks, package9/9, Chrome7/7. Shell приходит
до release,3 sync flushes,1 compressor drain,715 compressed body bytes, graph0.
Evidence: [progressive-gzip](benchmarks/stream-progressive-gzip-qualification-2026-11-01.json).
Это explicit test-host compression, не Accept-Encoding policy для любого host.

Active large gzip write cancellation проверен:4MiB varied patch, identity/ref
×disconnect/host abort. Во всех4 cases наблюдались pending drain, живой graph,
compressed output>4096; после abort pipeline settled/compressor closed/nodes0/
cleanup1. Fresh compressed stream восстановился. Raw:
[active-gzip-abort](benchmarks/stream-gzip-active-abort-qualified-2026-11-01.json).
Pending flush отдельно не подтверждён, socket saturation не заявляется.

Pending native gzip flush квалифицирован deterministic host-abort injection:
12 pending flush aborts +12 successful recoveries,6 cycles на каждый codec.
Во всех случаях graph0/cleanup1/pipeline settled/compressor closed; package9/9.
Raw: [pending-flush](benchmarks/stream-gzip-pending-flush-qualified-2026-11-01.json).
Это не remote timing race, socket saturation или long memory plateau.

Node Web ReadableStream adapter реализован: lazy pull/HWM0, UTF8 NDJSON,
identity/ref codec, cancel/pending next, external abort/owner disposal. JS1263/91,
TypeScript/build passed; native Response→Readable.fromWeb→HTTP4 checks passed.
Raw: [readable-SSR](benchmarks/readable-ssr-http-qualified-2026-11-01.json).
Это Node ALS entry, не portable Edge ownership implementation.

Readable terminal notification исправлена по2 failing regressions: background
producer error сообщает reader.closed без read; complete сам закрывает stream.
JS1265/91, TypeScript/build passed. Retained stream+reader GC64 requests/384
objects:0 alive, обе codecs и4 terminal modes. Raw:
[readable-retention](benchmarks/readable-ssr-retention-qualified-2026-11-01.json).
Long RSS/throughput/churn plateau этим не доказано.

Concurrent Readable soak60с квалифицирован:16 live cores/barrier,64720 requests,
по16180 complete/cancel/abort/error. Отмена половины не портит остальные cores;
каждый final response проверен по собственному ID. Все388320 WeakRef objects
collected при сохранённых terminal pairs; heap/RSS последних окон ниже ранних.
Raw: [readable-concurrent](benchmarks/readable-concurrent-soak-60s-2026-11-01.json).
Installed short probe включён в package gate,9/9 passed. Это не HTTP/browser/
Edge throughput, не многочасовой memory bound.

Host-neutral SSR runtime выделен: server.ts — Node ALS/UUID adapter, общий
runtime доступен через ./server-runtime. Carrier contract getStore/run/exit,
independent request/owner/render channels, bounded native-await/concurrency/
exit/error probe; sync stack rejected. JS1268/92, TypeScript/build и package9/9
passed; installed browser build напрямую импортирует runtime без Node builtins.
Injected Node host проверен отдельно; actual browser/Edge provider не готов.

Initial real Edge host qualified: local workerd native ALS +LinkeDOM,
2 concurrent fetch events/16 async renders, own values/isolated cores/cleanup.
Dynamic WASM compile rejection reproduced and fixed with public precompiled
asset +installSvetWasmModule before bootstrap. Export shape/lifetime guards
and same-build asset packaging verified. Rust53/build/JS1269/93 passed.
CARGO_TARGET_DIR embedding bug fixed;3 target-path regressions passed.
Raw: [workerd-installed](benchmarks/edge-ssr-installed-qualified-2026-11-01.json).
This is local workerd string SSR, not deployed Cloudflare/general Edge support.

Workerd streaming ownership qualified inside fetch events:16 concurrent cases,
4 terminal modes ×2 codecs; own values/cores/snapshots,8 late refusals. Direct
Response6-case matrix passes complete/error/TCP reset; Request.signal observed
before component completion with cleanup1/nodes0. Added receiver.finish() to
reject truncated EOF;3 regressions, JS1272/93 and optional Edge package9/9 passed.
Raw: [edge-stream](benchmarks/edge-stream-qualification-2026-11-01.json).
Miniflare proxy reader cancellation/normal FIN probes still fail to propagate
abort and are explicitly unqualified; failure evidence retained.

Explicit proxy request AbortController also fails to reach incoming Worker
signal in the pinned Miniflare path; negative evidence preserved. Optional
host deadline500ms qualified for both codecs: pending graph retired before
5000ms component completion, cleanup1/nodes0, deadline true/incoming abort false.
Raw: [proxy-deadline](benchmarks/edge-proxy-deadline-qualified-2026-11-01.json).
This bounds lifetime without claiming proxy cancellation fixed.

Shared runtime timeoutMs implemented for isolated/child/string/chunks/readable:
SsrTimeoutError, caller reason preservation, timer/listener release and child
isolation. JS1276/94, TypeScript/build and optional Edge package9/9 passed.
Real workerd Web stream timeout verified in2 fetch events: cleanup1/graph0.
Raw: [runtime-timeout](benchmarks/edge-runtime-timeout-2026-11-01.json).
Default remains no timeout; host timer cannot preempt synchronous CPU work.

Retained timeout error GC: исходный probe воспроизвёл удержание 8 child cores
и 8 WASM memories; перенос создания ошибки до таймера сам по себе не помог.
Преобразование stack в строку устранило удержание в Node probe. Последний вариант
создаёт ошибку только при истечении срока; 32 concurrent operations / 96 objects
collected при сохранённых errors/handles. Точный heap-retainer path отдельно не снят.
Raw: [expiry GC](benchmarks/ssr-timeout-retention-expiry-2026-11-01.json).
Полный suite после этой редакции прошёл: 1278/94; package с Edge и actions также прошли.

Следующий конкретный этап: owner-channel token GC, затем длительные HTTP/RSS bounds. Остальные задачи выполняются
по очереди P1–P4 выше; исходные отказы и результаты сохраняются.

## История этапов и подробные ограничения

Пункты ниже фиксируют ход работ. Их старые формулировки и числа тестов
не заменяют актуальную очередь выше.

### P0. Завершить текущий замер реального GPU-приложения

- [x] Проверить и запустить production fixture `tests/browser/resident-app-bench.html`
  по протоколу `interleaved-controls-v2`: предварительный прогрев, калибровка,
  чередование mapped/resident и A/A-контроли между раундами.
- [x] Сохранить новый raw JSON отдельно от пилота, параметры, версии, аппаратный
  адаптер, хеши исходников и production bundle. Независимо пересчитать p50/p95,
  проверить число кадров, порядок раундов, ошибки и освобождение ресурсов.
- [x] Подтвердить совпадение геометрии и пикселей; сравнить bytes upload/readback,
  latency, RAF intervals, long tasks и живые GPU-буферы. Отдельно указать
  постоянную память и пиковые allocations. RAF cadence не считать доказанным FPS.
- [x] Записать вывод и границы измерения в DEVELOPMENT.md. Пилот сохранён в
  `benchmarks/resident-app-pilot-metal-2026-11-01.json`: обнаружен временной дрейф,
  поэтому он пока не доказывает устойчивое ускорение. Наличие кода v2 ещё
  не означает, что повторный аппаратный прогон выполнен.
- [x] Проверить preparer, production fixture и package checks после последних
  изменений; сохранить итоговый отчёт и `git diff --check`.

Готово, когда результат воспроизводим, A/A показывает величину шума, а вывод
учитывает дрейф и компромисс памяти. Если шум перекрывает эффект, зафиксировать
неопределённость и причину следующего эксперимента.

Результат v2: 3240 измеренных кадров, 10 A/B и 3 A/A пары на Apple Metal 3.
На большой сетке p50 −39…−45%, A/A p50 −9…+4%; малая сетка без устойчивого
выигрыша. Постоянная GPU-память resident выше, пик logical buffers ниже.
Raw, независимый анализ, снимок и package checks сохранены; детали и ограничения
в DEVELOPMENT.md. Повтор на другом запуске и оборудовании остаётся в P3.

### P1. Закрыть память и длительный lifecycle

- [ ] Сравнить стоимость ограничения Rust-арены с зафиксированным старым
  алгоритмом при росте и повторном использовании блоков.
- [ ] Измерить JS heap, RSS, WASM memory и GPU allocations раздельно в
  контролируемом процессе; проверить плато после прогрева и пики реаллокации.
- [ ] Расширить churn на одновременно живые roots, DOM и GPU-сцены, несколько
  cores с общим device, отмену, reset и device loss. Проверить callbacks/finalizers
  и суммарный бюджет индексов/временных копий, а не только лимит одного массива.

Готово, когда измерения показывают ограниченный рост для явно заданной нагрузки
и teardown возвращает живые ресурсы к ожидаемой базе.

Промежуточное evidence P1: rolling 64 одновременно живых roots на двух cores,
160080 созданий / 640320 освобождённых WeakRef-объектов; graph capacity и WASM
стабильны, teardown возвращает nodes/edges/jobs к нулю. Raw:
`benchmarks/live-roots-default-isolated-frame-2026-11-01.json`.
RSS и heap выросли, поэтому пункт плато остаётся открытым. Журнал теперь ограничен 32 samples + baseline и четырьмя агрегатами.
Прогон без concurrent recompilation: 1.6 млн roots / 6.4 млн объектов за 73 с,
RSS по четвертям 45.93 / 47.19 / 45.53 / 44.10 МБ. Raw:
`benchmarks/live-roots-ring-no-concurrent-100k-2026-11-01.json`.
Обычный Node снова не освободил четыре объекта за десять GC; отказ сохранён.
Retainer path раннего отказа подтвердил module generator измерителя;
поздние snapshots показывают только WeakRef keep-during-job, после snapshot
и одного GC объекты освобождаются. Исходные отказы сохранены, gate не ослаблен.
Выполнен отдельный timed soak 120 с в обычном Node: 5.91 млн roots /
23.65 млн объектов, final backlog=0, максимум post-GC backlog=41 / возраст44 мс.
Heap по четвертям 8.121 / 8.168 / 8.172 / 8.172 МБ; RSS 53.71…54.58 МБ.
Raw: `benchmarks/live-roots-soak-120s-2026-11-01.json`.
Actions/сцены проверены отдельным 60-секундным soak: 743680 roots, столько
же успешных и отменённых actions, 5.95 млн WeakRef-объектов освобождены.
Graph/WASM/arena стабильны; RSS 87.80 / 91.41 / 92.37 / 89.97 МБ по четвертям.
Raw: `benchmarks/live-roots-actions-scenes-60s-2026-11-01.json`.
Это local RPC, HTTP и серверная конкурентность остаются открытыми.
Выполнен browser DOM/GPU soak на Metal 3: 60 с, 7194 ticks, 1031 сцена,
28776 updates/DOM events, 1796 snapshots; allocation counters сбалансированы,
финальные графы/буферы/текстуры/credits=0. Арена стабильна после фиксированного
warmup; ранний отказ сохранён. Raw: `benchmarks/dom-gpu-soak-metal-2026-11-01.json`.
Совместный recovery fixture прошёл 4/4 на Metal 3: pending pick + reset одного
core, сохранение соседа, explicit device.destroy(), новый adapter/device и
успешный render/pick обоих cores. Final allocations/графы/credits=0.
Raw: `benchmarks/shared-gpu-recovery-metal-2026-11-01.json`; package gates9/9.
Это явное destroy, не спонтанный hardware crash; повторяемые recovery cycles
остаётся открытым; installed-package hardware recovery ниже уже проверен.
Добавлены BROWSER_MATRIX.md и frozen runner: 11/11 DOM fixtures /
90 assertions прошли в production Chrome 156. Source hashes сверены с worktree.
Raw: `benchmarks/browser-matrix-chromium-2026-11-01.json`.
Router history прошёл 14/14 frozen production; Node SSR → browser resume
8/8 и browser HTTP actions7/7 прошли в dev, Node HTTP4/4 повторены.
Raw HTML/snapshot и server-observed replies сохранены, hashes в
`benchmarks/router-ssr-http-qualification-2026-11-01.json`.
Production browser SSR8/8 и HTTP7/7 установленного архива квалифицированы
следующим этапом ниже; нагрузка/async SSR остаются открытыми.
Независимые сборки текущего движка прошли9/9 в одном document:
shared dispatcher, closed Shadow DOM, teardown/reset isolation. Raw:
`benchmarks/independent-engines-production-2026-11-01.json`.
Это не cross-module tracking/untrack/async propagation.
Installed archive consumer прошёл matrix11/11 /90 assertions и hardware
recovery4/4 на Metal3. Все9 package gates прошли;15 runtime JS побайтно
совпали с dist/package,36 production assets проверены по hashes.
Raw: `benchmarks/installed-browser-qualification-2026-11-01.json`.
Production installed SSR→resume8/8 и HTTP actions7/7 прошли; Node HTTP4/4
и package9/9 повторены. Shared endpoints и installed Node renderer, archive/
runtime/asset hashes связаны в
`benchmarks/installed-production-ssr-http-qualification-2026-11-01.json`.
Остальные9 DOM lifecycle fixtures добавлены: installed production matrix
теперь20/20 /176 assertions, package9/9. Raw:
`benchmarks/expanded-installed-lifecycle-2026-11-01.json`;60 assets/15 runtime JS
проверены независимо. Это explicit captured continuation, не automatic async h().
Node async h/JSX component integration реализована: nested/provider owners
после await, immediate sync DOM, fail-fast, cancellation и concurrent requests.
Rust53, production build, JS1229/86, package9 и HTTP4 прошли; installed browser
async roundtrip5/5 + DOM20/176. Evidence:
`benchmarks/async-components-qualification-2026-11-01.json`.
Async request retention проверен15000 requests /120000 objects: exports и
WebAssembly.Memory освобождены без ручного cache advance. Исправлен hot-cache
retention retired cores, включая late cleanup; installed GC regression300
requests/2400 objects включена в package gate. Build,JS1229/86,package9,HTTP4
прошли. Raw: `benchmarks/async-component-retention-memory-2026-11-01.json`.
RSS вырос; длительное plateau и throughput improvement не доказаны.
Concurrent async soak60с прошёл:24 живых requests,2568 barriers,61632 requests /
493056 collected objects,41088 late refusals. Heap последних трёх окон~36.3МБ,
RSS к концу ниже первого окна; многочасовой bound не доказан.
Короткий installed concurrent GC probe включён в package gate;package9/9.
Raw: `benchmarks/async-concurrent-soak-60s-2026-11-01.json`.
Concurrent native HTTP async SSR/disconnect проверен:240 requests,peak12,
80 success/80 failure/80 disconnect; disconnected writes0, final graph/hooks0.
Installed regression48 requests включена в package gate;package9/9.
Evidence: `benchmarks/async-ssr-http-concurrent-2026-11-01.json`.
Это full HTML до headers, не streaming SSR.
Progressive Node stream protocol реализован: shell/patch/complete, one-slot
backpressure, limits, cancellation/pending next и owned browser receiver.
Build,JS1238/87,package9,HTTP4 прошли; native installed HTTP/browser6/6,
body disconnect/recovery2 checks. Raw:
`benchmarks/streaming-qualification-2026-11-01.json`.
Complete пока дублирует final HTML для coherence; versioning, retention/
throughput, long streams/proxies и Edge остаются открытыми.
Wire v1 реализован: explicit version/sequence/streamId + nested payload;
producer numbers0..N, receiver rejects unknown/legacy/replay/gaps before payload.
Исправлен pre-shell ownership: чужой resume state при protocol failure сохраняется.
Build,JS1243/88,package9,HTTP4,installed browser7/7 прошли. Actual legacy/new
receivers отвергают чужой формат в Node/JSDOM. Evidence:
`benchmarks/stream-v1-qualification-2026-11-01.json`.
Автоматический HTTP negotiation не реализован.
Следующий шаг — stream retention/throughput и portable browser/Edge adapters;
длительное плато и стабильность обычного compiler mode остаются открытыми.
DOM/GPU и actions/сцены требуют отдельных сценариев.

### P2. Закрыть браузерные и async-интеграции

- [ ] Собрать браузерную матрицу: события, перенос DOM между documents,
  models, router, SSR/resumption, настоящие HTTP actions и Shadow DOM.
- [ ] Проверить совместные GPU-сценарии: resize/DPR, очередь, отмена,
  readback, восстановление после device loss; отдельно hardware/software adapters.
- [ ] Определить поддержку async SSR components в `h()`, нескольких копий
  пакета, thenables resources/GPU fallback и capture после пользовательского await.
- [ ] Реализовать и квалифицировать Edge/browser SSR adapters в согласованном
  объёме. Node helper уже реализован; остальные среды остаются открытыми.

### P3. Расширить GPU и измерения приложений

- [ ] Проверить соответствие 2D/GPU projection, picking, clipping/depth,
  невыпуклых и перекрывающихся объектов на реальном renderer.
- [ ] Измерить throughput графов, startup и latency на дополнительных реальных
  сценариях; отделить очередь, вычисления, transfer и readback там, где это
  позволяет API. Не выдавать wall-clock latency за чистое время GPU kernel.
- [ ] Повторить untrack A/B и контроль без untrack: глубокий сценарий улучшился,
  но shallow control ранее регрессировал; стоимость bookkeeping ещё открыта.
- [ ] Измерить lifecycle guards, snapshots, cleanup и adoption относительно
  зафиксированной базы. Сравнения с другими движками проводить на настоящих
  реализациях и одинаковых нагрузках.

### P4. Матрица готовности и доставка

- [ ] Для reactive core, DOM, compiler/Forma, SSR, router, GPU, package и native
  studio записать пользовательские сценарии, критерии приёмки и ссылки на
  актуальные проверки; явно отметить частичное и неподдерживаемое.
- [ ] Исправить найденные дефекты с регрессиями и последовательно выполнить
  необходимые native/build/JS/package/HTTP проверки без конкуренции с замерами.
- [ ] Проверить CI, чистую установку, package archive, exports/types и соответствие
  Rust → embedded WASM → установленный пакет.
- [ ] Подготовить обзор изменений, результатов и ограничений. Commit, merge,
  push и публикация — отдельный запрошенный шаг с сохранением чужой работы.

## Ближайший этап: завершить текущие изменения reactive core

Обновлено 2026-11-01. Следующие пункты выполнять в указанном порядке.
Это незавершённая квалификация текущего кода, перед добавлением новых функций.

- [x] Проверить итоговые изменения lazy computed cache в Rust: отсутствие
  terminal effects должно сохранять инвалидирование вычислений; кеш должен
  учитывать только достижимые узлы своего источника и чужие pending effects.
  Пройдены 53 Rust-теста, включая четыре новые регрессии, и шесть JS-регрессий.
  Production build прошёл, embedded SIMD WASM — 54 320 байт.
- [x] Закрыть регрессию statesView: Rust Vec может переместиться внутри прежней
  WASM memory. Проверить сброс JS-view при выделении signal, computed, effect,
  повторном использовании effect и создании tensor. Все пять сценариев прошли
  в итоговом suite; installed-package consumer проверил перемещение отдельно.
- [x] После последних изменений последовательно выполнить native tests,
  production build, полный JS/WASM suite и девять installed-package проверок.
  Ожидаемый состав JS-suite — 1161 тест / 81 файл; это ориентир состава,
  а не уже полученный итоговый результат. Проверить фактические числа в отчёте.
  Итоговый прогон: 1160 passed, один timeout существующего benchmark-теста
  (6046 мс при лимите 5000 мс), 80 файлов passed из 81. Все новые функциональные
  регрессии прошли, девять installed-package checks прошли. Отдельный benchmark-тест
  затем прошёл без изменения timeout. Повтор полного suite с четырьмя workers:
  те же 1160 passed, только benchmark timeout 5994 мс.
  После исправления достоверности отчёта полный suite прошёл: 1161/1161,
  81/81 файлов, maxWorkers=4, 47.70 с; timeout и нагрузки не ослаблены.
  Артефакт: `benchmarks/core-suite-verified-2026-11-01.txt`. Ранние отказы сохранены;
  стабильность времени при разной нагрузке машины этим не доказана.
- [x] Проверить освобождение 64 отдельных cores после reset внутри активных
  observer frames скриптом `scripts/check-reset-core-gc.mjs`: все освобождены
  за один GC при отключённой concurrent recompilation. Артефакт:
  `benchmarks/reset-core-gc-2026-11-01.json`; RSS этим не проверяется.
- [x] Подготовить одинаковые baseline/candidate через
  `scripts/prepare-untrack-bench.mjs`: обе версии должны включать исправления
  корректности, различаться только регистрацией observer contexts.
- [x] Измерить `untrack` на глубинах 1/32/128/256 через
  `scripts/bench-untrack-cores.mjs`; отдельно выполнить контроль с
  `--untracks 0`. Сохранить raw results, хеши и параметры. Ранние замеры
  до исправления корректности не использовать для заявлений об ускорении.
  Первый парный прогон и контроль сохранены в `benchmarks/untrack-core-depth-2026-11-01.json`
  и `benchmarks/untrack-core-control-2026-11-01.json`. Глубокий repeated-untrack
  ускорился; shallow control показал регрессию. Повторяемость и стоимость
  bookkeeping без untrack остаются открытыми при фоновой нагрузке машины.
- [x] Обновить DEVELOPMENT.md итоговыми результатами, отказами и границами
  проверенного. Проверить `git diff --check`.

Тесты и CPU-бенчмарки запускать последовательно, без конкуренции за CPU.
Для полного suite явно фиксировать workers (проверены 1 и 4); существующий timeout
сравнительного benchmark-теста не увеличивать.

## Очерёдность дальнейшей работы

1. Завершить текущий замер GPU-приложения (P0); этап reactive core ниже уже проверен.
2. Доказать границы памяти и длительного lifecycle (разделы 1–2).
3. Собрать браузерную матрицу и закрыть async ownership (разделы 3 и 5).
4. Развить GPU-resident execution и проверить совместно с renderer (раздел 4).
5. Измерить реальные приложения, собрать матрицу готовности и подготовить
   доставку (разделы 6–8).

Каждый пункт закрывать конкретным сценарием, воспроизводимой проверкой
и сохранённым результатом. Результаты отдельных микробенчмарков не закрывают
производительность приложений; bounded memory checks не закрывают RSS.

## 1. Закончить проверку ограниченного роста Rust-арены

- [x] Проверить ограничение резервируемой ёмкости, амортизированный рост,
  сохранение данных и отказ без изменения старого хранилища: 48 Rust-тестов.
- [x] Пересобрать SIMD WASM и production-пакет с текущим кодом.
- [x] Прогнать полный JS/WASM suite, установку пакета и настоящий HTTP transport:
  1101 тест / 78 файлов, 9 проверок пакета и 4 HTTP-теста прошли.
- [ ] Измерить обычный рост и повторное использование свободных блоков;
  отдельно оценить стоимость ограничения ёмкости.
  Регрессия повторного использования пройдена: 2000 циклов по 128 блоков
  разного размера, случайный детерминированный порядок освобождения,
  стабильные адрес/ёмкость арены после прогрева. Сравнительный замер ещё открыт.
  Добавлен локальный WASM-замер: 4096 выделений при росте и 448 000 пар
  выделения/освобождения, семь замеров времени и памяти, проверка reset.
  Результаты: `benchmarks/arena-churn-2026-11-01.json`. Сравнение стоимости
  ограничения со старым алгоритмом остаётся открытым.
- [ ] Проверить давление памяти в контролируемом процессе. Ограничение Vec
  не доказывает ограничение RSS, пиков реаллокации или WASM linear memory.

## 2. Проверить длительную работу и освобождение памяти

- [ ] Циклически создавать и уничтожать roots, effects, resources, actions,
  captured contexts и сцены; проверить выход на плато после прогрева.
  Пройдены 20 000 циклов на двух чередующихся WASM cores без reset:
  живые nodes/edges/pending jobs равны нулю после каждого teardown,
  ёмкость графа и WASM memory стабильны. JS/RSS-плато ещё не доказано;
  записи находятся в `benchmarks/lifecycle-churn-20k-2026-11-01.json`.
  Повторный прогон ограничивает журнал 32 текущими и двумя базовыми
  замерами; WeakRef подтвердил освобождение 180 000 объектов за 20 000 циклов.
  Артефакт: `benchmarks/lifecycle-retention-20k-2026-11-01.json`.
  Этот сценарий не закрывает память DOM, GPU и одновременно живых roots.
  После исправления multi-core tracking повторены 20 000 циклов / 180 000
  объектов: обычный Node потребовал до десяти GC, без concurrent recompilation
  — один. Два более ранних отказа и путь удержания через VM-root сохранены
  в `benchmarks/multicore-gc-diagnostic-2026-11-01.json` и связанных файлах.
  Стабильное время сборки и общее RSS-плато остаются открытыми.
  Новый async-owner сценарий прошёл 1000 циклов / 3000 поддеревьев внутри одного
  живого запроса без reset. Все 15 000 объектов из WeakRef-проб освобождены,
  scopes и внешние abort hooks отсоединены, nodes/edges/jobs возвращаются к нулю.
  После прогрева capacity=6 и WASM memory=1 179 648 байт стабильны. Обычный Node:
  до четырёх GC-раундов; без concurrent recompilation: один. Raw results:
  `benchmarks/async-owner-churn-default-1000-2026-11-01.json` и
  `benchmarks/async-owner-churn-1000-2026-11-01.json`. RSS вырос в обоих прогонах;
  JS/RSS-плато, параллельная нагрузка и память DOM/GPU этим не доказаны.
- [ ] Раздельно измерять живые узлы, ёмкость графа и free lists, JS heap,
  WASM memory и GPU allocations.
- [ ] Повторить для нескольких cores, общего GPU device, отмены и reset.
- [ ] Проверить удержание finalizer/callback ссылок после завершения операций.
- [ ] Проверить большие входы индексов и координат без опасных выделений памяти.
  Добавлен `maxIndexBytes`: фиксированный лимит одной приватной копии индексов
  (по умолчанию 256 МиБ), общий для конструктора и updateIndices. Guarded tests
  отвергают обе ширины индексов выше лимита и почти MAX_SAFE_INTEGER без
  выделения памяти и чтения элементов. Суммарная/пиковая память ещё не закрыта.

## 3. Расширить проверки в настоящем браузере

- [x] Составить воспроизводимую матрицу событий, переноса DOM между documents,
  models, router, SSR/resumption и HTTP actions: BROWSER_MATRIX.md. DOM runner
  прошёл 11 fixtures / 90 assertions; router/SSR/HTTP помечены для отдельных
  свежих запусков, их полная квалификация остаётся открытой.
- [ ] Проверить WebGPU uploads, readback, rendering, device loss, отмену очереди,
  resize/DPR и Shadow DOM в совместных сценариях.
- [ ] Отдельно записывать результаты software adapter и физических GPU;
  CPU fallback и mocks не закрывают аппаратную квалификацию.

## 4. Развить GPU execution

- [x] Добавить кеширование compute pipelines с корректным device lifecycle:
  общий кеш устройства, ключ WGSL/entry point, LRU 32 записи / 1 МиБ ключей,
  вытеснение при ошибках и очистка при потере/уничтожении устройства.
  Браузерный fixture прошёл 10/10 на Apple Metal 3 (аппаратный адаптер):
  повторное использование, новые данные/workgroups, разные entry points,
  восстановление после ошибок и потери устройства. Замеры приложений,
  другие GPU/backends и аппаратная проверка вытеснения ещё открыты.
  Локальное аппаратное A/B выполнено дважды: задержка пакета из 40 dispatch
  с readback снизилась на 35–38% для 7 floats и примерно 22% для 65 536 floats.
  Это измерение конкретной нагрузки; время кадра приложения ещё не проверено.
- [x] Добавить GPU-resident результаты и цепочки без обязательного readback.
  Реализованы цепочки внутри одного dispatch: до 32 kernels, два чередующихся
  выходных GPU-буфера, один submit/readback и одна публикация. На Metal проверены
  реальные вызовы upload/map/submit, частичная запись и максимальная цепочка.
  Добавлены immutable GPU-handles между вызовами и ветвящиеся графы через
  `dispatchGpuComputeResident`; явный `readGpuResidentTensor` публикует итог.
  Отдельный device budget ограничивает persistent storage (256 МиБ / 1024
  reservations по умолчанию). Renderer interop закрыт отдельным пунктом ниже.
  Аппаратный fixture расширен до 15/15: поздняя ошибка bind group и отмена
  владельцем при ожидании map не публикуют результат, вызывают destroy всех
  временных буферов, закрывают scopes и возвращают кредиты очереди. Следующая
  корректная цепочка успешно выполняется после каждого отказа.
  Измерено сравнение с тремя отдельными dispatch на том же Metal: два прогона
  показали снижение задержки запроса на 56–61%. Входные загрузки 28→12 байт
  на элемент, readback 12→4, submit/map 3→1. Конечный observer вызывается один
  раз в обоих вариантах; замеры времени кадра и постоянных GPU-handles открыты.
  Добавлен outputLength для промежуточных стадий: GPU-only размеры независимы
  от WASM-арены, ограничены безопасным числом байт и лимитами устройства.
  На Metal проверена цепочка 129→1→7 с редукцией и обнулением выросшего хвоста.
  Новый resident fixture прошёл 7/7 на физическом Apple Metal 3: ветвление,
  одно host upload / явный readback, отказ input-writing WGSL, admission,
  ошибка shader, отмена map, reset и device loss. Явный read-only layout
  обеспечивает immutability; pipelines разделены по layout/input count.
  Тест границы 1024 результатов проверяет отказ до выделения и массовый reset.
  Shared lifetime hubs держат один постоянный listener на lifetime-сигнал.
  Пройдены production build, полный suite 1207/1207 (84 файла), девять
  installed-package checks и четыре HTTP-теста. Native/source evidence:
  `benchmarks/resident-qualification-2026-11-01.json`.
- [x] Подключить resident buffers к renderer без host readback на каждом кадре.
  Canvas3D/Viewport3D принимают `gpuVertices`: прямой vertex binding, без копии
  vertex buffer и без чтения host transformedVertices. Topology остаётся от mesh.
  Picking читает GPU snapshot только по клику и использует Rust occlusion picker;
  frame metadata содержит identity исходного GPU-handle. Замена кадра/teardown
  отменяют stale readback. Invalid resident source не подменяется host геометрией.
  Исправлены потеря последнего scheduled кадра при source retirement и скрытие
  первичной GPU-ошибки. Обе регрессии воспроизведены до исправления.
  Пройдены production build, 1221 тест / 85 файлов, девять package checks,
  четыре HTTP-теста, 7/7 аппаратных interop checks в dev и production bundle
  установленного пакета; прежний renderer прошёл 17/17 на Apple Metal 3.
- [ ] Измерить планирование графов/throughput на реальных приложениях.
- [ ] Измерить transfer bytes, время очереди, вычисления, readback и кадра.
- [ ] Проверить соответствие 2D/GPU projection, picking, clipping, depth,
  невыпуклой геометрии и перекрывающихся объектов.

## 5. Закрыть границы асинхронного контекста

- [ ] Определить и проверить ownership отдельных async SSR component subtrees.
  Добавлен Node-only `runWithOwnerScopeAsync`: независимые context/owner после
  await, вложенные и параллельные children, сохранение успешных scopes до teardown
  запроса, немедленная очистка при ошибке/отмене и отказ поздних продолжений.
  Синхронные scopes и captured re-entry имеют приоритет. Production build,
  полный suite 1172/1172 (82 файла), девять installed-package checks и четыре
  настоящих HTTP action теста прошли на Node 22.23.4.
  Node renderToStringAsync теперь автоматически ожидает async h/JSX components;
  qualification: JS1229/86, package9, HTTP4, installed browser roundtrip5/5.
  Browser async renderer, Edge, streaming и cross-copy propagation остаются открытыми.
- [ ] Проверить tracking при третьем приостановленном core и нескольких копиях
  пакета; сформулировать поддерживаемые границы.
  Исправлен третий синхронно приостановленный core: untrack и служебные
  callbacks приостанавливают все существующие observer frames в одной копии
  движка. Новые observers внутри callback продолжают tracking; исключения
  и reset восстанавливаются с учётом поколений. Две независимо собранные копии
  текущего кода прошли9/9 native dispatcher/graph isolation проверок; tracking
  и untrack через границу копий остаются открытыми.
- [ ] Проверить цепочки thenables у resources и GPU fallback.
- [ ] Проверить явный capture после пользовательского await в интеграциях.
- [ ] Отдельно реализовать и квалифицировать Edge/browser SSR adapters.

## 6. Измерить производительность приложений

- [x] Исправить достоверность существующего synthetic benchmark: явно назвать
  локальные имитации, убрать вычисленные из чужого времени SSR-результаты,
  измерить настоящие DOM-обходы и проверить их результат. Это не сравнение
  с настоящими React/Vue/Svelte и не аппаратный GPU-замер.

- [ ] Подготовить одинаковые воспроизводимые нагрузки и сохранить raw results,
  версии, оборудование и настройки сборки.
- [ ] Измерять startup, latency/p95, frame time, long tasks, память и bytes.
- [ ] Проверить стоимость lifecycle guards, snapshots, cleanup и adoption
  относительно зафиксированной базы.
- [ ] Делать сравнения с другими движками только на сопоставимых сценариях.

## 7. Собрать матрицу готовности

- [ ] Для reactive core, DOM, compiler/Forma, SSR, router, GPU, package и native
  studio перечислить конкретные пользовательские сценарии и критерии приёмки.
- [ ] Каждому критерию сопоставить актуальную проверку и артефакт результата.
- [ ] Отметить неподдерживаемое, частичное и непроверенное отдельно.
- [ ] Устранить обнаруженные дефекты с регрессиями; число тестов само по себе
  не является критерием готовности.
  Исправлена повторная проверка lifetime после чтения context-итераторов и tuple
  getters: reset/teardown не запускают callback и не портят свежий owner state.
  До исправления три регрессии падали; после него прошли четыре новые регрессии,
  весь suite 1176/1176 (83 файла), production build и девять package checks.


## 8. Подготовить доставку

- [ ] Проверить CI, чистую установку, архив пакета, exports и TypeScript types.
- [ ] Проверить отсутствие расхождений между Rust, embedded WASM и пакетом.
- [ ] Сохранить чужую работу в репозитории. Commit, merge, push и публикация
  выполняются отдельным запрошенным шагом.

## Команды базовой проверки

Запускать последовательно из корня репозитория; build должен предшествовать
JS-тестам после изменения Rust, чтобы они использовали новый embedded WASM.

```sh
CARGO_TARGET_DIR=/tmp/svet-native-validation cargo test --manifest-path packages/svet/crates/svet-wasm/Cargo.toml
npm --prefix packages/svet run build
npm --prefix packages/svet test -- --maxWorkers=1
npm --prefix packages/svet run test:package
npm --prefix packages/svet run test:actions
git diff --check
```
