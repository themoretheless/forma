# Покрытие Unified Style Guide

Эта карта сопоставляет текущие исходники Forma с **полным референсом**, а не
объявляет набор завершённым. Проверка исходников: 6 сентября 2026 года.
Источник — `Unified Style Guide.dc.html` из
`.forma/references/unified-style-guide`; номера строк ниже относятся к этому HTML.
Он содержит 17 разделов и глобальные Toast/Dialog. Screenshots отражают разные
редакции, поэтому точные состояния и метрики сверяются с HTML.

Для каждого пункта различаются три результата:

- **Визуальная часть:** `.ui`/SVG рисует конкретный снимок состояния.
- **Модель Rust:** отдельный тип меняет значение или выбор; наличие модели
  ещё не подключает её к конкретному экземпляру `.ui`.
- **Поведение хоста:** события окна, клавиатура, указатель, focus и модель
  соединены, результат отображается и одинаково работает в целевом хосте.

Файлы компонентов находятся в [components](components), композиции соседних
контролов — в [patterns.js](patterns.js). Галерея и native-окно проверяются
отдельно: наличие компонента в каталоге не доказывает готовность native-ввода,
модальности, IME или accessibility.

## Общая основа

| Референс | Что есть в исходниках | Что остаётся |
|---|---|---|
| Палитра light/dark, роли текста/границ/состояний, 848–928 | [tokens.js](tokens.js): две палитры, отдельные контрастные action/ink/wash роли, размеры и motion | Системное распространение смены темы на окна приложения; полная проверка контраста всех состояний после интеграции |
| Card/Panel, карточка цвета, 133–161 | `Surface`, `Card`, `ColorSwatch`, `DialogSurface`, `ToolbarSurface`; единая рамка и слоты | Swatch с автоматическими именем/HEX и копированием — отдельная композиция; тени ещё не часть Surface |
| Заголовки, UI text, mono/code text, 152–161 | `Label`, `StartLabel`, `BodyText`, `Paragraph`, `CodeLine` | Выбор IBM Plex Sans/Mono, начертания 400/500/600, измерение и рендер разных шрифтов; полноценные code/text blocks |
| Иконка, separator, статусная точка, клавиатурная подсказка | `Icon`, `CheckMark`, SVG, `Separator`, `StatusDot`, `Keycap` | Keycap показывает подпись, а не регистрирует shortcut; отдельная Link-модель и системные курсоры/shortcut policy |
| Badge пяти тонов, 239–242 | `Badge`: neutral/accent/success/warning/danger | Пассивная метка не заменяет selectable Chip |
| Avatar и AvatarGroup, 400–403 | `Avatar`; `patterns.avatarGroup()` размещает несколько аватаров с перекрытием | Изображения/загрузка, самостоятельный layout группы и avatar-hover transform |

`Surface` содержит поверхность, content, interaction и Reveal. Button добавляет
PointerArea; Label/Icon и составные визуальные части не получают собственные
обработчики. Верхние контролы остаются соседями одного Frame/Scroll; это не
произвольное дерево вложенных интерактивных элементов.

## Взаимодействие и выбор

[catalog-session.js](catalog-session.js) — работающий демонстрационный web-хост.
Он вызывает WASM-обёртки Rust-моделей, передаёт снимок в `sectionSource()` и
восстанавливает фокус после обновления сцены. Загрузка проекта из галереи
сохраняет текущий визуальный снимок через `catalogProject(..., { state })`.
CLI экспортирует исходные демонстрационные значения. Оба экспорта содержат
разметку без обработчиков демонстрационной сессии.

| Референс | Визуальная часть / композиция | Галерея | Контракт native-хоста и оставшееся |
|---|---|---|---|
| Primary/Secondary/Ghost/Danger, disabled, 165–181 | Четыре Button-наследника; `IconButton`, `LeadingIconButton`, `PrimaryIconButton` | Hover/pressed/focus/click, Tab/Shift+Tab/Space/Enter и журнал действий | Те же базовые состояния есть в Runtime; прикладные действия подключает приложение |
| LoadingButton/Spinner, 175 | `LoadingButton`, `Spinner`, `assets/spinner.svg` | Статическая иконка загрузки, disabled-кнопка | Вращение и жизненный цикл loading не реализованы самим компонентом |
| Checkbox, 272–279 | `Checkbox`, `ChoiceContent`, `ChoiceIndicator`, `CheckMark` | `CheckValue`: binary и tri-state, обновление checked/indeterminate; Reveal/focus только по квадратику | `CheckModel` доступна Rust-хосту; активацию и новый снимок связывает хост |
| Radio/RadioGroup, 283–290 | `RadioButton`; `patterns.choiceGroup()` | `SelectionValue`: один выбор, стрелки, Home/End; Reveal/focus только по кружку | `SingleSelection`/`RadioGroup`; хост связывает ввод, focus и disabled-policy группы |
| Switch, 244–248 | `Switch`, `SwitchThumb` | Переключение `ToggleValue` и checked | `ToggleModel`/`SwitchModel`; модель доступна, ввод и отображение связывает хост; анимации ручки нет |
| Выбираемые Chip, 293–297 | `Chip` | Независимые `ToggleValue` и selected | `ToggleModel`/`SelectableChipModel`; группу значений собирает владелец |
| SegmentedControl, 256–259 | `SegmentButton`; `patterns.buttonGroup()` | `SelectionValue`, активация, стрелки и Home/End | `SingleSelection`; действия группы, порядок фокуса и панель принадлежат хосту |
| UnderlineTabs, 356–363 | `TabButton`; `patterns.buttonGroup({ type: 'TabButton' })` | Выбор, стрелки/Home/End и подпись выбранной панели | `TabSelection`; реальные панели и focus policy подключаются отдельно |
| SideNavigation/ActivityBar, 364–370,608–614 | `NavigationItem`, `IconButton`, `StatusDot` | Выбор пункта боковой навигации через `SelectionValue` | Модель выбора доступна; ActivityBar, history и маршрутизация приложения требуют подключения |
| Pagination, 371–375 | `PageButton` и IconButton prev/next | Выбор, стрелки/Home/End, prev/next с границами | `SingleSelection`/`RangeModel`; страницы и данные приложения связывает хост |
| Закрываемые document-tabs и dirty-dot, 447–465 | `patterns.documentTabs()` резервирует место отдельной Close-кнопки | Выбор, закрытие, корректировка выбранного индекса, восстановление примера; список хранится в JS-сессии | `TabSelection` доступна; список документов, dirty-policy и подтверждение потери изменений принадлежат приложению |
| Tree, глубокая вложенность, 467–486 | `TreeItem`, `TreeContent`, file/folder/chevron SVG | `TreeValue`: раскрытие, видимые строки, выбор и четыре стрелки | `TreeModel` доступна; нужны native-связь с отображением, indentation guides, виртуализация и семантика пустой папки |
| Accordion/Disclosure, 430–441 | `DisclosureHeader`, `Paragraph`, `Card` | Сессия переключает expanded и наличие body | Boolean может храниться в `ToggleModel`; native-композиция и пересчёт layout на стороне хоста; высотной анимации нет |

В [control_state.rs](../src/control_state.rs) disabled/read-only policy намеренно
оставлена хосту. Поэтому наличие CheckModel/RangeModel не означает, что любой
экземпляр Checkbox/Slider в произвольном native-проекте уже меняется при вводе.

## Ввод значений

| Референс | Визуальная часть | Галерея | Контракт native-хоста и оставшееся |
|---|---|---|---|
| TextField с label/helper/suffix, error-field, 223–235 | TextField → FieldContent → FieldValue → TextInput | Набор, caret/selection, drag, Home/End, Delete/Backspace, undo/redo, clipboard | Тот же Runtime в native; label/helper/error association и прикладная валидация остаются у хоста |
| SearchField с иконкой и Kbd, 308–312 | SearchField и отдельные Icon/Label/TextInput | Полноценный обычный ввод и clipboard | Команда поиска и shortcut принадлежат приложению |
| Textarea, 328–330 | TextArea → MultilineValue → TextInput | Несколько строк, Enter, выделение, прокрутка за кареткой | Общий native Runtime; автоматического wrap ещё нет |
| Slider, 331–334 | `Slider`, `SliderTrack`; один процент value задаёт заливку и ручку | `RangeValue`: drag, capture, стрелки, Home/End, PageUp/PageDown, clamp/step | `RangeModel` доступна; native-хост переводит координаты и клавиши в модель, передаёт новый value |
| NumberStepper, 335–341 | `patterns.numberStepper()`: NumberValue и две IconButton | Plus/minus обновляют ту же модель диапазона, что и Slider | `RangeModel` доступна; native-связь с вводом, press-repeat и редактирование числа ещё нужны |
| Select, 313–327 | `SelectTrigger`, `MenuItem`, Surface | Открытие, выбор, стрелки/Home/End, Escape и возврат focus к trigger внутри секции | `SingleSelection` доступна; общий popup layer, anchor placement, outside-click и typeahead ещё нужны обоим хостам |
| Progress, 250–254 | `ProgressBar` и отдельная подпись | Статические проценты | Прогресс операции, indeterminate и shimmer — отдельная интеграция |
| Text subsystem, 521–563 | TextInput, TextEdit и общий renderer | Клавиатура, beforeinput, clipboard, preedit/commit | Winit keyboard/IME и положение candidate window, macOS clipboard; reconversion/surrounding-text, shaping и grapheme navigation ещё нужны |
| Rich text / RTF, 566–584 | `FormattingButton`, Paragraph | Независимое переключение B/I/U/S/H1 в панели без изменения текста | Нужны форматированный документ, выделение диапазонов, стили, headings/lists/clear formatting и связанные команды |

`Slider.value` и `ProgressBar.value` — процент (`60%`). У Slider ручка размещена
на границе процентной колонки через нулевую ячейку; отдельного `thumbOffset` нет.
`revealWidth: 1` обеспечивает Slider видимую рамку клавиатурного фокуса.

## Данные, сообщения и оболочки

| Референс | Реализованные части | Галерея | Контракт native-хоста и оставшееся |
|---|---|---|---|
| Table, 386–395 | `TableCells`, `TableHeader`, `TableRow`; `patterns.dataTable()` | Фиксированные данные трёх колонок, hover строки | Sorting/filtering, resize, selection, виртуализация, header semantics и данные приложения ещё не образуют Table-widget |
| Skeleton, 396–399 | `Skeleton` | Статические скруглённые полосы | Анимированного shimmer нет |
| Alert, 415–422 | `Alert`, `MessageContent`, Icon и два Label | Информационные сообщения | Wrap, actions и live announcements требуют отдельных возможностей |
| Toast, 674–676,1075–1077 | `Toast` | Показ по действию и скрытие через 2600 ms в секции | Общие positioning/queue, пауза/закрытие и accessibility announcements; native-таймер и lifecycle подключаются хостом |
| Tooltip, 426–428 | `Tooltip` | Toggle по кнопке, Escape; локальная поверхность | Hover/focus delay, anchor placement, overlay clipping и native-dismissal. В референсе только trigger-плейсхолдер |
| Dialog, 678–687 | `DialogSurface`, Paragraph, кнопки; `patterns.dialog()` | Заменяет содержимое одной секции; Close/Cancel/Confirm, локальный Tab-cycle, Escape и focus restore | Полноэкранная модальность, backdrop, блокирование других секций/окон и native overlay/window routing ещё не реализованы |
| AppShell, 488–518 | `WindowHeader`, `ToolbarSurface`, `NavigationItem`, `StatusBar`, Card, Splitter | Макет окна из соседних контролов; drag и ArrowLeft/ArrowRight/Home/End меняют ширину sidebar через RangeValue | Native-хост связывает модель диапазона с layout; маршрутизация действий и самостоятельный window manager ещё нужны |
| IDE, 587–661 | TreeItem, documentTabs, CodeLine/codeBlock, ToolbarSurface, FormattingButton, StatusBar, EmptyState, Splitter | Оболочка с работающим Splitter; код статичен, отдельная панель formatting переключает selected-кнопки | CodeEditor/Terminal/ProblemsPanel, RTF-документ, syntax model, gutter/current-line/caret, native splitter/layout routing, docking, редакторские команды и несколько окон |
| Rust theme code block, 664–669 | `CodeLine` и `patterns.codeBlock()` | Демонстрационные строки кода | Генерация Rust-констант из общей темы и copy action для такого блока отдельно не реализованы |

`CodeLine` рисует номер и одну строку. `patterns.codeBlock()` собирает строки,
но не добавляет syntax highlighting, editing или terminal emulation.
Галерея связывает `Splitter` с `RangeValue(70, 136, 1, 90)`: ширина sidebar
меняется от 70 до 136 px с шагом 1 px, исходное значение — 90 px. Native-хост
должен подключить изменение своего layout. `DialogSurface` в обычной сцене
не блокирует соседние контролы.

## Модели Rust: что именно уже есть

Исходники: [control_state.rs](../src/control_state.rs) и
[control_models_wasm.rs](../src/control_models_wasm.rs).

| Rust | WASM-обёртка | Реальная ответственность |
|---|---|---|
| CheckModel, CheckState, CheckMode | CheckValue | unchecked/checked/mixed, binary/tri-state циклы |
| ToggleModel | ToggleValue | Boolean для Switch или отдельного selectable Chip |
| RangeModel | RangeValue | Валидация min/max/step, clamp, snapping, fraction и шаг |
| SingleSelection; RadioGroup/TabSelection | SelectionValue | Выбор одного ID, очистка, перемещение с/без wrap |
| TreeModel | TreeValue | Preorder-дерево, раскрытие, visible IDs, выбор, навигация |
| TextEdit | TextValue (однострочный), Runtime TextInput (включая multiline) | caret/anchor, selection, insert/delete, home/end, undo/redo |

TextEdit работает по Unicode scalar boundaries, а смещения caret/anchor —
**UTF-8 bytes**, не JS UTF-16 и не grapheme clusters. История ограничена 100
редактированиями. Preedit/composition отображается адаптером TextInput; shaping
и форматирование модель не выполняет. TreeValue проверяет branch-флаг по наличию реальных детей;
пустая папка не является автоматически раскрываемой веткой.

Это самостоятельные модели, доступные хосту, а не общий движок bindings.
`.ui`-свойства checked/selected/expanded/value сейчас формируют снимок при
компиляции. Перевод OS/browser-события в вызов модели, перестроение снимка,
сохранение focus/capture и выполнение прикладного кода — слой хоста. Для части
контролов он уже показан в `catalog-session.js`; native-экспорт его не содержит.
Общее обсуждение свойств, зависимостей и прикладных bindings остаётся отдельным;
оно не препятствует доведению локального поведения контролов.

## Reveal: отдельное обязательное покрытие

Reveal в этом референсе означает **световое пятно на рамке рядом с указателем**,
которое при входе расширяется почти на весь контур. Это не entrance/fade-in.

В референсе rv2 (83–98,702–726): локальный центр пятна, radius 120 px / stop 60%,
при входе radius 1200 px / stop 99% с переходом 280 ms. Внешняя проверка близости 220 px
ограничивает набор DOM-элементов. Вариант rv1 с равномерным смешиванием рамки
по distance/130 объявлен отдельно; фактические образцы используют rv2.
Radial hover-блик по заливке (`hv-glow`,68–69) — ещё один самостоятельный эффект.

В Forma добавлены:

- [reveal.rs](../src/reveal.rs): параметры, координаты, переход, отключение
  движения, проверка дальнего указателя и disabled/focus policy;
- Reveal в [template.rs](../src/template.rs) и слоте
  [Surface.ui](components/Surface.ui);
- отдельная команда в [display_list.rs](../src/display_list.rs), CPU-отрисовка
  и GPU-ветка в [vector.wgsl](../src/vector.wgsl);
- `Runtime.reveal_pointer()` и обновление paint без пересборки геометрии;
- proximity-передача указателя web-хостом в
  [vector-preview.js](../../src/vector-preview.js), отдельно от hit-target.

Reveal реализован и проверен: тесты в `reveal.rs` покрывают proximity, дальний
указатель, focus/disabled/reduced-motion и согласованность CPU/GPU при разных
состояниях и DPI без повторной загрузки геометрии. Он включён в Surface и
отдельную секцию галереи. Фокус получает сплошную рамку, disabled отключает
Reveal; такой же примитив доступен native Runtime.

## Дополнительные motion-эффекты: открытый список

Эти эффекты референса перечислены отдельно. Они не считаются сделанными за счёт
наличия Reveal или обычного transition; выбор желательных эффектов не удаляет
их из карты покрытия.

| Эффект и строки референса | Статус |
|---|---|
| Цветовые состояния 180 ms, 52 | Brush-переходы есть |
| Reveal rv2, 83–98 | Реализован и проверен в общем Rust renderer, CPU/GPU |
| Diagonal shine 700 ms, 64–66 | Не реализован |
| Radial fill-light 250 ms, 68–69 | Не реализован; border-Reveal его не заменяет |
| Edge glow/blur 250 ms, 70–71 | Не реализован |
| Pulse 1 s и click-ripple 600 ms, 72–75 | Не реализованы |
| Press-scale .98, тени/hover-elevation, 52–59 | Не реализованы в Surface/Button |
| Стрелки/stepper/chevron press-transforms, 76–82 | Не реализованы |
| Spinner 700 ms, Skeleton shimmer 1600 ms, Progress shimmer 2400 ms | Есть статические части; ротация/shimmer не реализованы |
| Progress width 600 ms и демонстрационный шаг +7 каждые 2200 ms, 253,844 | Проценты отображаются снимком; такой transition и автоматический таймер не добавлены |
| Checkbox/Radio scale 220 ms, Switch knob 300 ms | Есть снимки состояний; transform-анимаций нет |
| Tab underline 250 ms, close/dirty 150 ms, sidebar indicator | Есть снимки выбранных состояний; соответствующих анимаций нет |
| Tree entrance 220 ms, Accordion height 350 ms / chevron 300 ms | Есть модели/локальное раскрытие; указанных анимаций нет |
| Popup opacity 200 ms / translate 280 ms; Toast opacity 300 ms / slide 350 ms; Dialog opacity 300 ms / scale 350 ms | Локальный lifecycle примера есть, в том числе toast timer 2600 ms; указанных анимаций и общего overlay manager нет |
| Theme crossfade 500 ms, entrance подъём 14 px/fade, background blobs | Переключение палитры есть; эти эффекты не реализованы |
| Символ 220 ms, каретка 120 ms / blink 1100 ms, программная подстановка 320 ms, placeholder 180 ms | Ввод немедленный; новые символы проявляются за 220 ms, каретка плавно мигает с циклом 1100 ms. Перемещение каретки, программная подстановка и placeholder пока без переходов |
| Тёплая selection-заливка 22% light / 38% dark | Выделение рисуется отдельными фигурами внутри clip TextInput; сейчас единая тёплая заливка |
| Avatar hover shift/scale, 402 | Не реализован |

Задержку отображения каждого набранного символа и движение каретки стоит
оставлять настраиваемыми: они меняют восприятие скорости ввода. Compact/touch
метрики адаптированы, а минимальные цели 18/22 px из референса не выбраны в
качестве базовых. Эти решения не означают отказ от остальных контролов.

## Полный список оставшегося поведения хостов

1. **Input routing:** перенос демонстрационных связей в публичный контракт
   контроллера приложения: экземпляр → Rust-модель → новый снимок, pointer
   capture, клавиатура, disabled/read-only и focus после перестройки/удаления.
   Галерея уже использует `onControlPointer`/`onControlKey` для перечисленных
   выше сценариев; это ещё не универсальный контроллер всех виджетов.
2. **Native window routing:** перевод winit-событий в контроллеры выбора,
   диапазона; несколько окон, active/focus/close lifecycle, popups
   внутри окна/отдельными окнами, DPI/resize и координаты anchor. Текущий
   `main.rs` обрабатывает кнопки/scroll и текстовый ввод/IME; модель
   отдельного native-приложения не подключается автоматически.
3. **Текст и IME:** автоматический wrap, grapheme/word navigation, surrounding-text
   и reconversion, clipboard для Windows/Linux, расширенная проверка IME на разных
   раскладках. Базовый ввод, каретка, выделение, clipboard web/macOS и Enter уже есть.
4. **RTF:** rich-text document model, ranges со стилями, bold/italic/underline/
   strike, headings, ordered/unordered lists, clear formatting, toolbar
   selection state, clipboard formats и история операций. `FormattingButton`
   является только кнопкой; TextEdit не хранит форматированный документ.
5. **Overlays:** positioning относительно anchor и viewport, отдельный
   overlay layer, clipping/z-order, outside-click, блокирование фонового ввода,
   modal semantics, tooltip hover/focus delay и toast queues. Локальные
   Escape/focus-cycle/restore и toast timer в галерее уже есть; нужны общий
   менеджер overlays и соответствующие native-адаптеры.
6. **Accessibility:** отдельные роли, имена, значения и состояния каждого
   контрола, отношения label/helper/error, keyboard/group semantics, live
   announcements и focus notifications. Web-подпись canvas не заменяет дерево
   элементов; native accessibility bridge ещё требуется.
7. **Текстовый renderer и оболочки:** разные шрифты/веса, автоматический wrap/ellipsis,
   shaping, code syntax model, terminal semantics, большой Table/Tree с
   виртуализацией, native-связь splitter с layout, docking, редакторские и
   window-команды. Drag и клавиатура Splitter уже подключены в галерее.
8. **Прикладные зависимости:** подписки на значения приложения, обновление
   данных, типы/стили/зависимости и вызов пользовательского кода. Это отдельный
   контракт библиотеки, который ещё предстоит обсудить.

## Границы самого референса

В HTML есть настоящие локальные переключения и демонстрационные заглушки.
Tooltip не имеет popup-body (426–428); pagination prev/next не меняют страницу
(1048); IDE splitter имеет resize-курсор без drag-handler (649); `⌘K` показан
как подсказка; Terminal/CodeView — демонстрационный текст. При реализации
виджетов эти места должны получить осмысленное поведение, а не копироваться
как якобы завершённые контролы.

Критерий завершения пункта — его визуальная часть, нужная модель и проверенное
поведение в заявленных хостах. Количество `.ui`-файлов или секций галереи таким
критерием не является.
