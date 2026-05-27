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

## Open

- [ ]

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

