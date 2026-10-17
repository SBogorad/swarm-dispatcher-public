//! Плейбуки исполнения: протокол работы воркера по виду задачи.
//!
//! Источник — набор `pstack` (Lauren Tan / poteto): 23 плейбука и 24
//! инженерных принципа, из которых здесь оставлено ядро, применимое к
//! задачам в песочнице. IDE-специфика (Cursor, управление PR, субагенты)
//! сознательно отброшена: воркер диспетчера не имеет доступа к GitHub и не
//! командует другими воркерами.
//!
//! Зачем: раньше `prompt.rs` отдавал воркеру один и тот же плоский промпт
//! на любую задачу, и модель не знала, исследует она баг, пишет фичу по TDD
//! или делает рефакторинг. Теперь `contract.kind` выбирает протокол.
//!
//! Итерация 4 (2026-10-17): краткие выжимки заменены боевыми протоколами из
//! `skills/` (диагностика багов, архитектура, перформанс, верификация,
//! расследование, рефакторинг, arena) и добавлены принципы, которые раньше
//! не доезжали до модели: атака на предпосылку, дисциплина типов,
//! идемпотентность, тестирование поведения, «объясни число» и Decision Trail.
//!
//! `## Decision Trail` — не только текст промпта: его структуру проверяет
//! ревьюер в строгом режиме рубрики (`REVIEW_REQUIRE_DECISION_TRAIL=1`,
//! см. `crates/swarm-reviewer/src/main.rs`).

/// Ядро инженерных принципов. Добавляется воркеру всегда, независимо от вида.
pub const CORE_PRINCIPLES: &str = "\
Принципы работы (обязательны для любой задачи):
- Prove It Works: не заявляй об успехе без проверяемого доказательства \
(тест, команда, конкретный файл:строка). «Должно работать» — не доказательство.
- Fix Root Causes: устраняй первопричину дефекта, а не симптом. Заглушка \
поверх проблемы считается невыполненной задачей.
- Attack the Premise: если два исправления на одной предпосылке не прошли одну \
и ту же проверку — выпиши эту предпосылку и проверь её саму, прежде чем писать \
третье исправление. Провалы указывают на предпосылку, а не на исправления.
- Laziness Protocol: делай минимальное изменение, решающее задачу. Не \
раздувай дифф и не рефактори то, что не просили.
- Subtract Before You Add: сначала удаляй мёртвый и дублирующийся код, \
только потом добавляй новые абстракции.
- Type System Discipline: делай недопустимые состояния непредставимыми \
(суммарные типы/enum вместо набора Option-полей), разбирай внешние данные на \
границе и не обманывай компилятор приведением типов.
- Make Operations Idempotent: каждый шаг должен сходиться к одному результату \
при повторном запуске или падении посередине. Спроси себя: что будет, если это \
выполнится дважды?
- Test Behavior, Not Implementation: тестируй наблюдаемое поведение через \
публичный интерфейс. Не пересчитывай ожидаемое значение той же формулой, что и \
код: тавтологический тест проходит при любом баге.
- Explain the Number: любая цифра (перф, лимит, доля) требует названного \
ограничителя (CPU, lock, I/O) и числа прогонов. Не можешь объяснить число — \
ты не знаешь, что измерил.
- Boundary Discipline: соблюдай границы модулей и контрактов. Не пробивай \
слои и не расширяй свои права.
- Show Me Your Work: заверши артефакт секцией `## Handoff`, а перед ней — \
таблицей `## Decision Trail` со столбцами \
`phase | decision | why | evidence | result`. В `evidence` — указатель \
(файл:строка, sha256, номер PR, SHA коммита), а не проза.
- Encode Lessons in Structure: память в промпте снижает вероятность повторной \
ошибки, но урок, закодированный в структуру (линтер, гейт, тест, схема БД, \
тип), делает её невозможной. Нашёл класс ошибок — сначала сделай так, чтобы \
проверка краснела на нём, и только потом пиши правило в текст.
- Reflect After Correction: если тебя вернули с `changes_requested` \
(`rework > 0`) или ты синтезируешь результат в Arena, обязательно добавь \
секцию `## Reflection / Lesson Learned`: какая КОРНЕВАЯ ошибка была допущена \
и какое структурное правило предотвратит её в следующем графе. Урок без \
названного ограничителя («был невнимателен») не считается уроком.";

/// Плейбук по виду задачи (`contract.kind`).
///
/// Возвращает структурированный чеклист шагов. Сопоставление — по точному
/// значению и по префиксу (`code.bugfix` и `bugfix` дают один плейбук).
pub fn playbook_for_kind(kind: &str) -> Option<&'static str> {
    let k = kind.trim().to_ascii_lowercase();
    // Префикс `code.` — декоративный: `code.bugfix` ≡ `bugfix`.
    let base = k.strip_prefix("code.").unwrap_or(&k);
    match base {
        "bugfix" | "bug" | "fix" => Some(PLAYBOOK_BUGFIX),
        "feature" | "code" | "task" | "implementation" => Some(PLAYBOOK_FEATURE),
        "refactor" | "refactoring" | "cleanup" => Some(PLAYBOOK_REFACTOR),
        "investigate" | "analysis" | "audit" | "research" => Some(PLAYBOOK_INVESTIGATE),
        "perf" | "performance" | "hillclimb" => Some(PLAYBOOK_PERF),
        "architect" | "design" | "architecture" => Some(PLAYBOOK_ARCHITECT),
        "verify" | "test" | "verification" => Some(PLAYBOOK_VERIFY),
        "arena_candidate" => Some(PLAYBOOK_ARENA_CANDIDATE),
        "arena_synthesize" => Some(PLAYBOOK_ARENA_SYNTHESIZE),
        "root" => Some(PLAYBOOK_ROOT),
        _ => None,
    }
}

const PLAYBOOK_BUGFIX: &str = "\
Протокол: Bug Fix (диагностика в четыре фазы).
1. Фаза 1 — тесная петля обратной связи: назови ОДНУ детерминированную \
команду или тест, которая краснеет на этом баге и зеленеет после фикса. \
Никаких гипотез, пока нет способа воспроизведения.
2. Фаза 2 — минимизация: сузь сценарий до минимального набора входных данных, \
на котором баг ещё воспроизводится.
3. Фаза 3 — опровержимые гипотезы: сформулируй 3 ранжированные гипотезы в \
формате «Если причина в <X>, то изменение <Y> устранит симптом». Проверяй их \
наблюдением, а не догадкой.
4. Фаза 4 — фикс первопричины и регрессионный тест: устрани причину (не ставь \
guard-костыль на симптом) и зафиксируй регрессионным тестом на публичном seam.";

const PLAYBOOK_FEATURE: &str = "\
Протокол: Feature (Domain First, затем TDD).
1. Определи типы, структуры данных и границы домена до реализации.
2. Сформулируй проверку внешнего поведения (что считается успехом).
3. Реализуй минимум кода, проходящий эту проверку.
4. Докажи прохождение checks из контракта.";

const PLAYBOOK_REFACTOR: &str = "\
Протокол: Refactoring (сначала вычти, потом добавляй).
1. Зафиксируй текущий контракт поведения (characterization pin): существующие \
проверки зелёные до и после изменения.
2. Удали мёртвый код и одноразовые обёртки, прежде чем добавлять абстракции.
3. Переведи всех вызывающих на новый интерфейс и удали старый API в одной \
волне: не оставляй параллельных legacy-путей.
4. Докажи снижение нагрузки на читателя: меньше слоёв косвенности и меньше \
скрытого состояния.";

const PLAYBOOK_INVESTIGATE: &str = "\
Протокол: Investigation (механика отдельно от мотивации).
1. Разделяй механику (`how`: Overview, Key Concepts, How It Works, \
Where Things Live, Gotchas) и мотивацию (`why`: почему сделано именно так).
2. Каждое утверждение маркируй уровнем уверенности: [Direct] (точная ссылка \
файл:строка), [Supported], [Inferred], [Speculative], [Unknown].
3. Обязательна секция `What We Don't Know`: пробелы, которые нельзя \
заполнять догадками. Не угадывай.";

const PLAYBOOK_PERF: &str = "\
Протокол: Performance (сначала замер, потом оптимизация).
1. Проверь замер по 7 вопросам benchmark-checklist: (1) почему не вдвое — \
назови ограничитель; (2) прогнан ли на боевом профиле; (3) не нарушил ли \
лимиты ресурсов; (4) не было ли ошибок; (5) воспроизводится ли (медиана и \
разброс по >= 5 прогонам); (6) значима ли доля в end-to-end; (7) вообще ли \
этот код исполнялся.
2. Применяй 7 мантр строго от дешёвой к дорогой: 1. Don't do it -> 2. Do it, \
but don't do it again -> 3. Do it less -> 4. Do it later -> 5. Do it when \
they're not looking -> 6. Do it concurrently -> 7. Do it cheaper.";

const PLAYBOOK_ARCHITECT: &str = "\
Протокол: Architect (сначала интерфейс, потом код).
1. Usage First: покажи 2-3 примера вызова глазами потребителя, и только из них \
выводи сигнатуры и типы.
2. Data Structures First: определи структуры данных и владение состоянием; \
раздели общее состояние до его сериализации.
3. Design It Twice: сравни минимум 2 радикально разных варианта по глубине \
интерфейса (Deep vs Shallow module) и выбери, объяснив выбор.
4. Screen Against 8 Design Red Flags: Shallow module, Information leakage \
(wire-типы в публичном API), Temporal decomposition, Pass-through method, \
Split ownership, Two ways to do one task, Importable internals, Hand-synced list.";

const PLAYBOOK_VERIFY: &str = "\
Протокол: Verification (фальсифицируемый вердикт).
1. Переформулируй проверяемое утверждение в фальсифицируемый предикат: условие, \
метрика, порог.
2. Найди «The one fact it's safe because of» и укажи уровень доказательства по \
шкале blast-radius (от ссылки файл:строка до реального запуска).
3. Вынеси ровно один вердикт: VERIFIED, NOT VERIFIED или INCONCLUSIVE. \
INCONCLUSIVE не считается успехом.";

const PLAYBOOK_ARENA_CANDIDATE: &str = "\
Протокол: Arena — независимый кандидат.
1. Предложи СВОЙ целостный вариант решения задачи; не оглядывайся на других \
кандидатов и не усредняй.
2. Приведи короткое обоснование: какие альтернативы рассмотрел и почему отверг \
каждую.
3. Верни один самодостаточный артефакт-кандидат.";

const PLAYBOOK_ARENA_SYNTHESIZE: &str = "\
Протокол: Arena — синтез (сначала Pick a base, затем Graft).
1. Оцени кандидатов-родителей по критериям рубрики и выбери базу: ту, которую \
будущий сопровождающий сможет расширить легче всего.
2. Вживи лучшие части проигравших в базу вручную (обычно 1-2 на кандидата), \
чтобы результат остался связным под одной моделью.
3. Назови базу и отметь, что отвергнуто и почему.";

const PLAYBOOK_ROOT: &str = "\
Протокол: Root Synthesis.
1. Выполни сводный разбор цели задачи, изложенной в постановке.
2. Дай структурированный, проверяемый итоговый артефакт.
3. Заглушки и отписки недопустимы: если данных не хватает, верни \
{\"error\": \"missing_input\", \"details\": \"...\"}.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_principles_cover_named_rules() {
        for name in [
            "Prove It Works",
            "Fix Root Causes",
            "Attack the Premise",
            "Laziness Protocol",
            "Subtract Before You Add",
            "Type System Discipline",
            "Make Operations Idempotent",
            "Test Behavior, Not Implementation",
            "Explain the Number",
            "Boundary Discipline",
            "Show Me Your Work",
        ] {
            assert!(
                CORE_PRINCIPLES.contains(name),
                "принцип {name} отсутствует в CORE_PRINCIPLES"
            );
        }
    }

    #[test]
    fn show_me_your_work_names_the_trail_columns() {
        assert!(CORE_PRINCIPLES.contains("## Decision Trail"));
        for col in ["phase", "decision", "why", "evidence", "result"] {
            assert!(CORE_PRINCIPLES.contains(col), "нет колонки {col}");
        }
    }

    #[test]
    fn code_prefix_is_transparent() {
        assert_eq!(
            playbook_for_kind("code.bugfix"),
            playbook_for_kind("bugfix")
        );
        assert_eq!(
            playbook_for_kind("code.feature"),
            playbook_for_kind("feature")
        );
        assert_eq!(playbook_for_kind("code.perf"), playbook_for_kind("perf"));
        assert_eq!(
            playbook_for_kind("code.arena_candidate"),
            playbook_for_kind("arena_candidate")
        );
    }

    #[test]
    fn kind_is_trimmed_and_case_insensitive() {
        assert_eq!(playbook_for_kind("  ROOT  "), playbook_for_kind("root"));
    }

    #[test]
    fn catalog_covers_required_kinds() {
        for k in [
            "code.bugfix",
            "code.feature",
            "code.refactor",
            "code.investigate",
            "code.perf",
            "code.architect",
            "code.verify",
            "code.arena_candidate",
            "code.arena_synthesize",
            "root",
        ] {
            assert!(playbook_for_kind(k).is_some(), "нет плейбука для {k}");
        }
    }

    #[test]
    fn investigation_forbids_guessing() {
        let pb = playbook_for_kind("code.investigate").unwrap();
        assert!(pb.contains("What We Don't Know"));
        assert!(pb.contains("Не угадывай"));
        assert!(pb.contains("[Direct]"));
    }

    #[test]
    fn bugfix_playbook_demands_a_failing_repro_first() {
        let pb = playbook_for_kind("code.bugfix").unwrap();
        assert!(pb.contains("тесная петля обратной связи"));
        assert!(pb.contains("Никаких гипотез"));
    }

    #[test]
    fn architect_playbook_screens_red_flags() {
        let pb = playbook_for_kind("code.architect").unwrap();
        assert!(pb.contains("Usage First"));
        assert!(pb.contains("Design It Twice"));
        assert!(pb.contains("Shallow module"));
        assert!(pb.contains("Hand-synced list"));
    }

    #[test]
    fn unknown_kind_has_no_playbook() {
        assert!(playbook_for_kind("nonsense").is_none());
        assert!(playbook_for_kind("").is_none());
    }
}
