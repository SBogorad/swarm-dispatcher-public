# Obsolete-файлы (не участвуют в сборке и не попадают в аудит-бандл)

Перенесены сюда по итогам внешнего аудита v0.4 (см. `docs/audit/AUDITOR_BRIEF_v0.4.md`).

| Файл | Почему убран |
|------|--------------|
| `schema_roles.sql.stub` (был `schema/roles.sql`) | Заглушка в одну строку. Актуальные роли — `step2/roles.sql`; этот файл никем не применялся (`docker-compose.yml` монтирует `step2/roles.sql` как `/sql/roles.sql`). |
| `test_roles_2b.DELIVERED_partial.py` (был `step2b/`) | Обрезанный остаток от прерванной доставки (3477 Б против 8859 Б у полного `step2b/test_roles_2b.py`). |
| `roles_2b_patch_section3.sql` (был `step2b/`) | Исторический дубль секции ролей для тестов из `roles_2b.sql`; никем не применялся, не монтировался compose и не входил в glob `roles_patch_*.sql`. SHA-256 сохранён в `REPORT_v0.4.1.md`. |

Оба файла сохранены как история — ничего не удалено безвозвратно.
