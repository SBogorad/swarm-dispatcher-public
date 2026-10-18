//! Пересчёт счётчиков хунков в unified diff.
//!
//! Модель объявляет заголовок `@@ -old_start,old_count +new_start,new_count @@`,
//! но счётчики берёт «на глаз»: в теле хунка оказывается другое число строк.
//! Ревьюер (`validate_unified_diff`) сверяет объявленное с фактическим и
//! отклоняет артефакт целиком — задача уходит в реворк, хотя сам патч верный.
//!
//! Здесь счётчики пересчитываются по телу каждого хунка, а объявленные
//! `old_start`/`new_start` остаются как есть: модель знает, куда вставлять,
//! а арифметику строк делает код. Хвост заголовка (имя функции-якоря) тоже
//! сохраняется.

/// Пересчитать счётчики всех хунков в тексте артефакта.
///
/// Текст, не содержащий хунков, возвращается без изменений. Строки за
/// пределами хунка (заборы ```, заголовки файлов) не трогаются.
pub fn recount_hunks(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    // `split` даёт пустой хвост, если текст кончается переводом строки: это не
    // строка тела, а признак завершающего `\n`. Иначе она попала бы в контекст
    // и раздула счётчики последнего хунка.
    let trailing_newline = text.ends_with('\n');
    let last = lines.len().saturating_sub(1);

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut hunk: Option<Hunk> = None;

    for (idx, line) in lines.iter().enumerate() {
        let line = *line;
        if trailing_newline && idx == last {
            break;
        }
        if let Some((old_start, new_start, tail)) = parse_header(line) {
            flush(&mut hunk, &mut out);
            hunk = Some(Hunk {
                old_start,
                new_start,
                tail: tail.to_string(),
                body: Vec::new(),
                old_count: 0,
                new_count: 0,
            });
            continue;
        }
        if is_hunk_boundary(line) {
            flush(&mut hunk, &mut out);
            out.push(line.to_string());
            continue;
        }
        match hunk.as_mut() {
            Some(h) => match line.chars().next() {
                Some(' ') => {
                    h.old_count += 1;
                    h.new_count += 1;
                    h.body.push(line.to_string());
                }
                Some('-') => {
                    h.old_count += 1;
                    h.body.push(line.to_string());
                }
                Some('+') => {
                    h.new_count += 1;
                    h.body.push(line.to_string());
                }
                // `\ No newline at end of file` — маркер, а не строка тела.
                Some('\\') => h.body.push(line.to_string()),
                // Строка вне формата хунка: закрываем хунк, чтобы не приписать
                // ей чужие строки, и отдаём как есть.
                _ => {
                    flush(&mut hunk, &mut out);
                    out.push(line.to_string());
                }
            },
            None => out.push(line.to_string()),
        }
    }
    flush(&mut hunk, &mut out);

    let mut result = out.join("\n");
    if trailing_newline {
        result.push('\n');
    }
    result
}

struct Hunk {
    old_start: usize,
    new_start: usize,
    tail: String,
    body: Vec<String>,
    old_count: usize,
    new_count: usize,
}

fn flush(hunk: &mut Option<Hunk>, out: &mut Vec<String>) {
    if let Some(h) = hunk.take() {
        out.push(format!(
            "@@ -{},{} +{},{} @@{}",
            h.old_start, h.old_count, h.new_start, h.new_count, h.tail
        ));
        out.extend(h.body);
    }
}

/// Разобрать `@@ -l[,s] +l[,s] @@ <хвост>`.
///
/// Возвращает стартовые номера и хвост после закрывающих `@@`. Счётчики не
/// возвращаются: они и есть то, что мы пересчитываем.
fn parse_header(line: &str) -> Option<(usize, usize, &str)> {
    let rest = line.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let spec = &rest[..end];
    let tail = &rest[end + 3..];
    let mut parts = spec.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    if parts.next().is_some() {
        return None;
    }
    Some((start_number(old)?, start_number(new)?, tail))
}

fn start_number(spec: &str) -> Option<usize> {
    let start = match spec.split_once(',') {
        Some((start, _)) => start,
        None => spec,
    };
    if start.is_empty() {
        return None;
    }
    start.parse().ok()
}

/// Строка, которая не может быть телом хунка.
fn is_hunk_boundary(line: &str) -> bool {
    line.starts_with("```") || line.starts_with("--- ") || line.starts_with("+++ ")
}

#[cfg(test)]
mod tests {
    use super::recount_hunks;

    #[test]
    fn recounts_each_hunk_by_its_own_body() {
        let src = "```diff\n--- a/x.rs\n+++ b/x.rs\n\
@@ -10,2 +10,3 @@ fn f()\n a\n-b\n+c\n+d\n\
@@ -50,6 +50,6 @@\n e\n```\n";
        let got = recount_hunks(src);
        assert!(got.contains("@@ -10,2 +10,3 @@ fn f()"), "{got}");
        assert!(got.contains("@@ -50,1 +50,1 @@"), "{got}");
        assert!(got.contains("\n a\n-b\n+c\n+d\n"), "тело потеряно: {got}");
    }

    #[test]
    fn keeps_text_without_hunks_untouched() {
        let src = "просто текст\nбез патча\n";
        assert_eq!(recount_hunks(src), src);
    }

    #[test]
    fn counts_context_and_ignores_no_newline_marker() {
        let src = "@@ -1,1 +1,1 @@\n a\n b\n\\ No newline at end of file\n";
        assert_eq!(
            recount_hunks(src),
            "@@ -1,2 +1,2 @@\n a\n b\n\\ No newline at end of file\n"
        );
    }

    #[test]
    fn keeps_text_without_trailing_newline() {
        let src = "@@ -1,1 +1,1 @@\n+a";
        assert_eq!(recount_hunks(src), "@@ -1,0 +1,1 @@\n+a");
    }
}
