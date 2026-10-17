# Third-Party Notices

Этот репозиторий содержит материал, производный от сторонних проектов,
распространяемых под лицензией MIT. Ниже — атрибуция и статус использования.

## 1. pstack — используется

**Copyright (c) 2026 Lauren Tan**
Источник: <https://github.com/cursor/plugins/tree/main/pstack>
Лицензия: MIT — полный текст в [`LICENSES/pstack-MIT.txt`](LICENSES/pstack-MIT.txt)

**Что именно включено.** Плейбуки исполнения и набор инженерных принципов в
[`crates/swarm-worker/src/playbooks.rs`](crates/swarm-worker/src/playbooks.rs)
переписаны по текстам плейбуков и принципов pstack: `code.bugfix`,
`code.architect`, `code.perf`, `code.verify`, `code.investigate`, `code.refactor`
и принципы (Prove It Works, Fix Root Causes, Laziness Protocol,
Subtract Before You Add, Boundary Discipline, Attack the Premise,
Type System Discipline, Make Operations Idempotent,
Test Behavior Not Implementation, Explain the Number, Show Me Your Work).

Текст адаптирован под контекст песочницы без доступа к GitHub: IDE-специфика
(Cursor, управление PR, субагенты) исключена. Адаптация — производная работа
в смысле MIT; уведомление об авторских правах сохранено.

## 2. skills (Matt Pocock) — рассмотрен, текст не включён

**Copyright (c) 2026 Matt Pocock**
Источник: <https://github.com/mattpocock/skills>
Лицензия: MIT — полный текст в [`LICENSES/matt-skills-MIT.txt`](LICENSES/matt-skills-MIT.txt)

Набор изучался при проектировании плейбуков. Дословный текст в репозиторий
не переносился (`git grep -i "Matt Pocock"` → 0 совпадений). Атрибуция
приведена для полноты: если в будущем текст будет включён, уведомление уже
на месте.

## 3. cursor-team-kit — рассмотрен, текст не включён

**Copyright (c) 2026 Cursor**
Источник: <https://github.com/cursor/plugins/tree/main/cursor-team-kit>
Лицензия: MIT — полный текст в [`LICENSES/cursor-team-kit-MIT.txt`](LICENSES/cursor-team-kit-MIT.txt)

То же, что и п. 2: набор изучался, дословный текст не переносился
(`git grep -i "cursor-team-kit"` → 0 совпадений).

## Условие MIT

Все три лицензии требуют сохранять приведённое выше уведомление об авторских
правах и текст лицензии в копиях или существенных частях программного
обеспечения. Это выполнено: тексты лежат в `LICENSES/`, атрибуция — в этом
файле.

MIT не накладывает ограничений на лицензию собственного кода репозитория и не
требует его раскрытия.
