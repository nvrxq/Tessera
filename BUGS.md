# Tessera — bug log

Build: `release` (см. `target/release/tessera`)
Started testing: 2026-05-25

---

## Формат записи

```
- [ ] <severity> — <короткое описание>
      Шаги: ...
      Ожидал: ...
      Получил: ...
      Заметки: ...
```

`severity`: `crit` (краш / data loss) · `high` (workflow blocked) · `med` (раздражает) · `low` (косметика)

Поставь `[x]` когда я починю.

---

## Fixed — batch `fix/bug-hunt-batch` (2026-05-29)

<!-- Найдено многоагентным аудитом 2026-05-29 (46 кандидатов → 27 подтверждённых
     user-facing, 17 отсеяно). S1/S2/S3 = три симптома из исходного репорта.
     Все пофикшено на ветке fix/bug-hunt-batch; cargo test --workspace зелёный,
     tsc + vite build зелёные, прошло состязательное ревью (Rust + frontend).
     vitest не гонялся (нет в частичном node_modules; npm install заблокирован
     guard'ом) — 5 новых rename-тестов Sidebar покрыты tsc+build. -->

> **Caveat S3 (`--continue`):** фикс привязывает идентичность хуков/статуса/
> activity/уведомлений к процессу через `TESSERA_WORKSPACE_ID` — это
> детерминированно лечит кросс-атрибуцию при общей папке. Часть про
> *резюм чужой беседы* (`--continue` в общей папке) уже закрыта в **v0.1.10**
> (per-workspace `claude_session_id` + `--resume <id>`), на которую эта ветка
> отребейзена — две части S3 (атрибуция хуков + резюм беседы) теперь покрыты
> вместе.
>
> **Caveat config-толерантность:** один плохой *тип* у строкового поля
> (`font_family`/`density`/`cursor_shape` как число) или у `schema_version`
> всё ещё каскадит в дефолт секции. Покрыты распространённые случаи: плохой
> hex, размер-строкой/вне диапазона, неизвестный enum-вариант, плохой bool,
> не-массив palette. Остаточный edge — низкий приоритет.

### high

- [x] high — Shift+Tab отправляет обычный Tab (0x09) вместо CSI Z (back-tab)
      Получил: нельзя циклить режимы Claude Code (plan / auto-accept) — Shift+Tab == Tab
      Корень: `encodeKey` `case "Tab"` не смотрит `ev.shiftKey` — ui/src/Terminal.tsx:1437
      Фикс: `return ev.shiftKey ? [0x1b,0x5b,0x5a] : [0x09]`

- [x] high — wide-символы (CJK/emoji) сдвигают всю строку влево  (S1)
      Получил: всё правее emoji/CJK съезжает на колонку, глиф наезжает на соседа, курсор не там
      Корень: grid.rs строит строку push-ом по порядку, игнорируя `cell_index()`/`width()`
        wezterm (visible_cells() пропускает spacer-клетку wide-символа)
        — crates/term/src/grid.rs:57-122, cell.rs:34-72; фронт рисует по фикс. c*cellW
      Фикс: класть клетки по физической колонке (pad до cell_index, +width-1 spacer)

- [x] high — один broadcast `Lagged` навсегда убивает PTY-pump → весь вывод замерзает
      Шаги: флуд stdout (`cat большой_файл`, alt-screen storm) → >1024 чанков в очереди
      Получил: грид замирает во ВСЕХ workspace, ввод принимается но не виден, лечит только рестарт
      Корень: `while let Ok(evt) = rx.recv().await` выходит на recoverable Lagged — src-tauri/src/lib.rs:129
      Фикс: `match` с веткой `Err(Lagged(_)) => continue`, `break` только на `Closed`
      Заметки: severity high (блок workflow), вероятность триггера спорная (нужно >~4MB буфера)

- [x] high — keydown терминала висит на document без проверки target
      Шаги: открыть workspace (живой PTY) → печатать в поле "Add task"/Link URL/Settings hex
      Получил: символы не попадают в поле, а уходят в PTY claude; Backspace/Enter/стрелки тоже
      Корень: onKey гард только `if(!sid)return`, нет проверки INPUT/TEXTAREA — Terminal.tsx:966,1058
      Фикс: bail если `ev.target` — editable (как уже сделано в App.onGlobalKey:273-277)

- [x] high — идентичность workspace привязана к ПАПКЕ, не к процессу  (S3)
      Шаги: создать 2 workspace на одной папке (UI это позволяет, нет уникальности repo_path)
      Получил: статус-дот/Activity/OS-уведомление/worktree одного приписываются другому
        (last-created выигрывает запись `.claude/settings.local.json`); + `--continue`
        резюмит чужую беседу claude из той же папки
      Корень: install_hooks пишет UUID в файл папки безусловно, spawn env пуст
        — crates/workspace/src/service.rs:334-365,177; dispatch_hook доверяет evt.workspace_id — lib.rs:530
      Фикс: передавать workspace UUID процессу через env и читать из хука; либо запрет дублей папок

### med

- [x] med — одно невалидное значение в settings.json молча сбрасывает ВСЕ настройки
      Шаги: вписать `"background":"#GG0000"` (или font_size строкой) → рестарт
      Получил: все кастомизации "пропали" (только скрытый tracing::warn); следующий Save затирает их на диске
      Корень: serde all-or-nothing → load_or_default ловит ошибку поля и отдаёт default — crates/core/src/config.rs:274-296
      Фикс: пер-полевая толерантность (deserialize_with/нормализация) + не затирать файл на ошибке

- [x] med — инвентарь "All workspaces (globals only)" всегда пуст
      Шаги: открыть инвентарь без выбранного workspace
      Получил: 0 skills / 0 MCP, хотя ~/.claude настроен
      Корень: createResource с source `()=>props.workspaceId`; null трактуется как "skip fetcher" — ui/src/ClaudeInventoryModal.tsx:44
      Фикс: завернуть source в объект `()=>({id})` чтобы fetcher всегда вызывался

- [x] med — Pomodoro pause→resume теряет Break, и paused Break засчитывается как цикл
      Получил: пауза Break → resume как Work; paused Break ≥150s при reset инкрементит "cycles today"
      Корень: Paused не хранит прежний mode; resume хардкодит Work; cycle_credit не проверяет mode — src-tauri/src/commands.rs:1017-1027,901
      Фикс: хранить `paused_from`, восстанавливать его; кредит только если прежний mode == Work

- [x] med — сбой spawn (нет `claude` в PATH) → вечно "starting claude" без ошибки
      Получил: пейн навсегда висит на лоадере, ни ошибки ни ретрая, unhandled rejection в консоли
      Корень: spawn-IIFE без catch, phase не имеет "error" — ui/src/Terminal.tsx:1158-1185,351
      Фикс: добавить phase "error" + caption + retry; показать e.to_string() (claude not found)

- [x] med — font_size_px без границ на бэке и в модалке
      Шаги: вписать font_size_px 0 или 50000 в settings.json → рестарт
      Получил: 0 → невидимый текст + сотни фантомных колонок; 50000 → гигантский грид 20x5; PTY ресайзится в мусор
      Корень: bare u16 + только `#[serde(default)]`; границы 8..32 живут лишь во фронте — config.rs:133, SettingsModal.tsx:402
      Фикс: clamp 8..=32 в deserialize_with/normalize (и в settings_save)

- [x] med — Settings Cancel/Esc откатывает зум терминала (Ctrl +/-), сделанный при открытой модалке
      Получил: после зума и закрытия без Save шрифт прыгает обратно; на диске значение осталось → рассинхрон, вернётся после рестарта
      Корень: onCleanup пишет `initialSnapshot` в общий signal, не учитывая внешние записи (zoom/settings_changed) — ui/src/SettingsModal.tsx:118-124
      Фикс: на cancel перечитывать loadSettings() вместо setSettings(snapshot)

- [x] med — inline-rename уничтожается апдейтом статуса агента
      Шаги: переименовать workspace у которого агент активно работает (статус мигает) → печатать
      Получил: input пересоздаётся из старого имени на лету, текст теряется
      Корень: optimistic mutate меняет ref объекта на каждый status-event → <For> пересоздаёт строку; input uncontrolled — App.tsx:229-235, Sidebar.tsx:228
      Фикс: guard mutate (не менять ref если статус тот же) + controlled draft-signal для имени

- [x] med — клик по версии / 30-мин поллинг во время загрузки апдейта прячет прогресс
      Получил: индикатор загрузки заменяется на "Checking…/Update vX", закачка/relaunch идут скрыто; можно запустить ВТОРУЮ закачку
      Корень: runUpdateCheck/poll сбрасывают state без проверки "downloading"; pill не disabled — ui/src/App.tsx:136-177,476
      Фикс: early-return если state.kind==="downloading"; disabled на pill во время загрузки

- [x] med — смена палитры/фона перерисовывает локальный грид по устаревшим RGB  (S1)
      Получил: при смене Background в Settings тело терминала держит старый цвет (live-preview) / вспышка старого фона на 1 кадр при Save
      Корень: грид-зеркало хранит уже-resolved RGB; bg-skip сравнивает с НОВЫМ bgInt; live-preview не зовёт set_palette — Terminal.tsx:544,1126, terminal.rs:93
      Фикс: на смене bg переписать клетки old→new bg перед paintFull; либо хот-свап палитры на live-preview

- [x] med — delta-снапшот только что выбранной сессии ложится на старый грид  (S1)
      Шаги: 2 workspace с живыми сессиями одинакового размера, быстрое переключение
      Получил: мазки чужих клеток на ~1 кадр пока не придёт full
      Корень: setActiveSession не форсит следующий снапшот как full — Terminal.tsx:128-141,692
      Фикс: на switch сбросить gridCols/gridRows=0 (или forceFull) чтобы первый снапшот ре-базлайнил

### low

- [x] low — раздел Appearance (UI font family + Density) сохраняется, но НИГДЕ не применяется
      Корень: --font-ui захардкожен в index.css:78; density не реализована вовсе — SettingsModal.tsx:342, App.tsx:105
      Фикс: прокинуть в CSS-переменные/data-density, либо убрать мёртвые контролы

- [x] low — F1–F9 печатают литералы "F1".."F9" в PTY; F10–F12 молча проглатываются
      Корень: encodeKey без кейсов F-клавиш; "F1".."F9" проходят fallback length<=2 — Terminal.tsx:1450
      Фикс: добавить xterm-последовательности F1-F12 (или match /^F\d+$/ → [])

- [x] low — многокодпойнтовые кластеры (ZWJ-emoji, флаги, скин-тон) рендерятся первым кодпойнтом  (S1)
      Корень: GridCell.ch/WireCell.c — один `char`; from_wez берёт `.nfkc().next()` — crates/term/src/cell.rs:45-53
      Фикс: хранить грейфему строкой (SmolStr/Box<str>), зеркалить в WireCell.c

- [x] low — курсор рисуется поверх scrollback при прокрутке в историю  (S1)
      Корень: alwaysShowCursor перебивает scroll-aware visibility=false (бэк не шлёт shape=hidden) — terminal.rs:241,277, Terminal.tsx:740
      Фикс: добавить флаг `scrolled` в Snapshot и гасить курсор когда scrolled

- [x] low — extras-панель держит вкладку прошлого workspace после переключения
      Корень: `tab` signal инициализируется один раз; панель не keyed, prop меняется без resync — ui/src/WorkspaceExtrasPanel.tsx:48
      Фикс: `createEffect(on(()=>props.workspaceId, id=>setTab(readStoredTab(id))))`

- [x] low — onSpawned читает selected() через await → session id может уйти не тому workspace  (S3, транзиентно)
      Корень: нет ре-проверки `props.workspaceId===ws` после `await terminal_resize` — Terminal.tsx:1176-1181, App.tsx:634
      Фикс: повторить guard после await; пробрасывать ws id в onSpawned

- [x] low — старый грид прошлого workspace остаётся нарисован до первого full при переключении  (S2)
      Корень: setActiveSession не чистит грид-зеркало/канвас; resizeCanvasBacking пропускает clear при равных dims — Terminal.tsx:128-141,800
      Фикс: на switch залить канвас bg + сбросить грид-зеркало

- [x] low — терминальный шрифт не перемеряется после загрузки Geist Mono  (S2, cold-start)
      Корень: measureCell кэширует метрики фолбэк-шрифта на старте, нет document.fonts.ready хука как в SettingsPreview — Terminal.tsx:902
      Фикс: на fonts.ready инвалидировать кэш + measureCell + syncGrid

- [x] low — quick-switch Cmd/Ctrl+1..9 и [ ] завязаны на e.key → ломаются на AZERTY и пр.
      Корень: матч по e.key (символ, зависит от раскладки) вместо e.code — ui/src/App.tsx:281,289
      Фикс: матчить по e.code (Digit1..9 / BracketLeft/Right)

- [x] low — Alt/Option+клавиша теряет модификатор (нет ESC-Meta префикса)
      Корень: encodeKey не префиксит ESC для Alt; fallback шлёт сырой ev.key — Terminal.tsx:1428,1450
      Фикс: при bare Alt+ASCII вернуть `[0x1b, code]`; на macOS брать ev.code

- [x] low — drag-reorder пишет дублирующиеся sort_order между секциями active/passive
      Корень: индекс считается по одной секции ((idx+1)*10), пишется в глобальный столбец без уникальности — App.tsx:372, workspaces.rs:65
      Получил: при смене секции (старт/стоп сессии, рестарт) позиция прыгает по created_at-тайбрейку
      Фикс: ресеквенс по глобальному списку (или на бэке после батча)

- [x] low — inline-rename: blur отменяет вместо коммита
      Получил: набрал имя, кликнул мимо → имя молча потеряно
      Корень: onBlur чистит renamingId без props.onRename — ui/src/Sidebar.tsx:256
      Фикс: коммитить на blur как на Enter (или явный визуальный cue что отменено)

---

## Fixed

- [x] high — в терминале нет курсора
      Корень: Claude Code TUI постоянно шлёт DECTCEM (`\e[?25l`), бекенд
      честно отдаёт `cursor_visible: false`, фронт его не рисует. Плюс
      вторая дыра: delta-снапшот, попадающий в клетку курсора, затирал
      его через `paintCell`, а блок отрисовки курсора в `applySnapshot`
      перерисовывал курсор только при смене позиции.
      Фикс (`ui/src/Terminal.tsx`):
        1. `tessera.term.alwaysShowCursor` (default `true`) — игнорируем
           DECTCEM и всегда показываем курсор.
        2. Курсор перештамповывается на каждом снапшоте, когда
           `shouldShowCursor`, так что delta-overpaint больше не уносит
           его.
        3. Bar/underline thickness floor поднят с 1 до 2 device-px —
           1-px bar терялся на `#0F0F10` при низком DPI.

---

## Fixed — batch `fix/session-identity-render` (2026-05-29, v0.1.12)

Повтор багов после v0.1.10/0.1.11: воркспейсы в **одной папке** всё ещё
берут чужую сессию, артефакты, терминал (или половина) пропадает. Прошлый
«фикс» (пиннинг сессии по mtime) был неверным в корне. Многоагентный аудит
+ адверсариальная верификация подтвердили корни и проверили сами фиксы.

- [x] crit — воркспейсы в одной папке путают claude-сессии (S3)
      Корень: пиннинг по `newest_session_id(cwd)` — берётся jsonl с самым
      свежим mtime в общей папке, т.е. **соседский**; плюс `--continue`
      резюмит самый свежий разговор в папке независимо от воркспейса. Ещё:
      `encode_cwd` мапил только `/`→`-`, а claude 2.x мапит и `_`, и `.`→`-`
      (проверено: `/tmp/tessera_sidtest` → `-tmp-tessera-sidtest`), так что
      детекция вообще смотрела не туда.
      Фикс: Tessera сама генерит uuid сессии и передаёт claude через
      `--session-id <uuid>` (свежая) / `--resume <uuid>` (когда jsonl уже
      есть, поиск по uuid во всех project-dir — без зависимости от encode).
      Детекция по mtime и `--continue` удалены. Миграция 0011 чистит старые
      (битые) пины, чтобы существующие воркспейсы получили чистую identity.
      `claude --session-id`/`--resume` поведение проверено эмпирически.

- [x] crit — артефакты / «полтерминала» при переключении (S1)
      Корень: `applySnapshot` перестраивал сетку при `snap.full || sizeChanged`;
      после `setActiveSession` (gridCols=0) delta-снапшот новой сессии имел
      `sizeChanged===true` и его **частичные** клетки трактовались как полная
      сетка → пол-экрана пусто/мусор.
      Фикс: ребейзим только из `snap.full`; delta при сброшенном зеркале
      дропается (resize всё равно форсит full).

- [x] high — терминал просто пропадает
      Корень: бэкенд шлёт `pty_event{exit}`, но фронт его не слушал →
      мёртвый `session_id` оставался, повторный заход биндился к мёртвой
      сессии → пустой холст без оверлея.
      Фикс: `Terminal` слушает `pty_event` (синхронно, чтобы не пропустить
      краш-на-старте), помнит вышедшие сессии, показывает «session ended →
      Restart» вместо пустоты и не биндится к мёртвой сессии при повторном
      заходе.

- [x] high — двойной spawn / осиротевший claude
      Фикс: `spawn_agent` идемпотентен (переиспользует живую сессию через
      `Supervisor::is_alive`); «Reset session» теперь убивает PTY и чистит
      пин, иначе идемпотентность вернула бы ту же живую сессию.

- [x] med — чёрная вспышка на старте (S2)
      `resizeCanvasBacking` теперь заливает фон, даже когда сетки ещё нет.

- [x] high — неверный размер сетки на первом чанке (S1)
      `workspace_spawn_agent` пред-регистрирует (cols,rows) в sizes-map, чтобы
      парсер не создавался в 80×24 до прихода `terminal_resize`.

- [x] high — потеря байтов PTY под нагрузкой (S1)
      broadcast capacity 1024 → 8192; `Lagged` дропал байты парсера → артефакты.

- [x] med — палитра <16 цветов из правленого settings.json
      `config.rs` отвергает палитру не из 16 записей → дефолт (раньше ANSI 8–15
      молча подменялись).

- [x] low — `setTerminalFontSize` без клампа; форма Links/Tasks не сбрасывалась
      при смене воркспейса; backfill `paused_from` для таймеров, поставленных
      на паузу до миграции 0010 (миграция 0012).

- [x] high — артефакты накапливаются «спустя время пользования» (S1, повтор)
      Корень (подтверждён 3 независимыми агентами + адверсариальная верификация):
      когда широкий глиф (CJK/emoji, width=2) заменяется узким символом, бэкенд
      шлёт дельту с клеткой i, но НЕ со spacer-колонкой i+1 (она blank→blank,
      «не изменилась»). Фронт перерисовывает только i (ширина cellW), а правая
      половина старого широкого глифа в колонке i+1 остаётся на канвасе. Полной
      перерисовки в обычной работе нет (только при resize), поэтому такие
      ostatki НАКАПЛИВАЮТСЯ.
      Фикс: бэкенд (`terminal.rs`) в diff-цикле при `old.w>1 && new.w<=1`
      force-эмитит колонку i+1, чтобы фронт её перерисовал (тест
      `wide_to_narrow_force_emits_spacer_column`). Плюс safety-net: keyframe —
      полный снапшот раз в `KEYFRAME_INTERVAL` дельт, чтобы ЛЮБОй дрейф
      самоисцелялся за пару секунд активности (тест `keyframe_forces_periodic_full`).
      Плюс курсор: erase теперь срабатывает и при смене формы курсора в той же
      клетке (`lastCursorShape !== snap.cursor_shape`).

