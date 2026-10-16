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

/// Ядро инженерных принципов. Добавляется воркеру всегда, независимо от вида.
pub const CORE_PRINCIPLES: &str = "\
Принципы работы (обязательны для любой задачи):
- Prove It Works: не заявляй об успехе без проверяемого доказательства \
(тест, команда, конкретный файл:строка). «Должно работать» — не доказательство.
- Fix Root Causes: устраняй первопричину дефекта, а не симптом. Заглушка \
поверх проблемы считается невыполненной задачей.
- Laziness Protocol: делай минимальное изменение, решающее задачу. Не \
раздувай дифф и не рефактори то, что не просили.
- Subtract Before You Add: сначала удаляй мёртвый и дублирующийся код, \
только потом добавляй новые абстракции.
- Boundary Discipline: соблюдай границы модулей и контрактов. Не пробивай \
слои и не расширяй свои права.
- Handoff Discipline: если в контракте есть вход от родительских задач, \
используй его и заверши результат короткой секцией `## Handoff` — ключевые \
решения и выходные данные для следующих задач графа.";

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
        "root" => Some(PLAYBOOK_ROOT),
        _ => None,
    }
}

const PLAYBOOK_BUGFIX: &str = "\
Протокол: Bug Fix.
1. Воспроизведи проблему: определи падающий кейс (тест, команда, входные данные).
2. Локализуй первопричину: найди место, где поведение расходится с ожиданием. \
Не чини символ, пока не понял источник.
3. Внеси минимальный фикс, устраняющий причину.
4. Проверь отсутствие регрессий: исходный кейс зелёный, остальное не сломано.";

const PLAYBOOK_FEATURE: &str = "\
Протокол: Feature (Domain First, затем TDD).
1. Определи типы, структуры данных и границы домена до реализации.
2. Сформулируй проверку внешнего поведения (что считается успехом).
3. Реализуй минимум кода, проходящий эту проверку.
4. Докажи прохождение checks из контракта.";

const PLAYBOOK_REFACTOR: &str = "\
Протокол: Refactoring.
1. Зафиксируй текущее поведение (существующие проверки должны быть зелёными).
2. Удали лишнее и дублирующееся (subtract before add).
3. Измени структуру, не меняя внешнее поведение.
4. Подтверди, что поведение не изменилось.";

const PLAYBOOK_INVESTIGATE: &str = "\
Протокол: Investigation & Epistemics.
1. Работай только с проверяемыми фактами из кода, контракта и входных данных.
2. Каждое утверждение подкрепляй точным указанием (файл:строка, имя функции).
3. Не угадывай: то, что не проверено, помечай как непроверенное явно.";

const PLAYBOOK_PERF: &str = "\
Протокол: Performance & Hillclimb.
1. Зафиксируй baseline: чем измеряешь и сколько сейчас.
2. Сформулируй узкое место и объясни число (почему именно там).
3. Предложи оптимизацию и оцени ожидаемую дельту.
4. Измерь результат после изменения.";

const PLAYBOOK_ARCHITECT: &str = "\
Протокол: Architect.
1. Спроектируй структуры данных и интерфейсы до кода.
2. Оцени альтернативы (design it twice), выбери и объясни выбор.
3. Определи инварианты и границы обработки ошибок.";

const PLAYBOOK_VERIFY: &str = "\
Протокол: Verification.
1. Проверь граничные случаи и негативные пути, а не только happy path.
2. Тестируй внешнее поведение, а не внутреннюю реализацию.
3. Не пиши тест, дублирующий реализацию: он пройдёт при любом баге.";

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
    fn core_principles_cover_five_named_rules() {
        for name in [
            "Prove It Works",
            "Fix Root Causes",
            "Laziness Protocol",
            "Subtract Before You Add",
            "Boundary Discipline",
        ] {
            assert!(
                CORE_PRINCIPLES.contains(name),
                "принцип {name} отсутствует в CORE_PRINCIPLES"
            );
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
            "root",
        ] {
            assert!(playbook_for_kind(k).is_some(), "нет плейбука для {k}");
        }
    }

    #[test]
    fn investigation_forbids_guessing() {
        let pb = playbook_for_kind("code.investigate").unwrap();
        assert!(pb.contains("проверяемыми фактами"));
        assert!(pb.contains("Не угадывай"));
    }

    #[test]
    fn unknown_kind_has_no_playbook() {
        assert!(playbook_for_kind("nonsense").is_none());
        assert!(playbook_for_kind("").is_none());
    }
}
