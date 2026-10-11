# Аудит-бандл скриптов — отчёт

**Проект:** `swarm-dispatcher`
**Версия бандла:** v0.4
**Сгенерировано:** 2026-10-11T09:15:47+07:00
**Генератор:** `scripts/build_audit_bundle.py`
**Артефакт:** `docs/audit/SCRIPTS_BUNDLE_v0.4.xml`

---

## 1. Назначение

Единый машиночитаемый снимок **всех скриптов** репозитория (инициализация БД, схема, роли, контейнеризация, тесты, конфиг без секретов) для независимого аудита. Каждый файл включён с полным текстом, размером, числом строк и двумя хешами (`sha256`, `md5`), что позволяет проверить целостность и воспроизводимость.

## 2. Сводка

- Файлов в бандле: **17**
- Суммарный объём исходников: **117198 байт** (~114.5 КиБ)
- Суммарно строк: **2527**
- Размер XML-бандла: **125967 байт**
- `sha256` бандла: `1a875d44f7fde6480d352689d6b308e729645796529c64e6bc47034a559bb32e`
- `md5` бандла: `bfac32672c6b627d065c7e7bcae56367`

## 3. Состав по ролям

| Роль | Файлов | Байт |
|------|-------:|-----:|
| Тесты | 3 | 43243 |
| Схема БД | 1 | 27083 |
| Роли и гранты | 2 | 19126 |
| Контейнеризация | 3 | 11354 |
| Вспомогательные патчи | 1 | 7030 |
| Патчи грантов | 2 | 3674 |
| Инициализация БД (bash) | 3 | 2934 |
| roles-patch-obsolete | 1 | 1579 |
| Конфиг (без секретов) | 1 | 1175 |

## 4. Манифест файлов

| # | Путь | Роль | Язык | Байт | Строк | sha256 | md5 |
|--:|------|------|------|-----:|------:|--------|-----|
| 1 | `scripts/02-roles.sh` | init | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` | `222f2961a86f5beb870842a67892823f` |
| 2 | `scripts/03-roles-2b.sh` | init | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` | `7641d32f6d09c4e07ae237a90945a7a5` |
| 3 | `scripts/03-role-patches.sh` | init | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` | `fb95baf99f0113e70b2be398967fad25` |
| 4 | `schema/schema_v0.sql` | schema | sql | 27083 | 533 | `0349a8e214da802e6f4ecff26e0686865ea9b93a518ac49d232ec4ba3797a087` | `62111662feb99c19d69b21428c674e0a` |
| 5 | `step2/roles.sql` | roles | sql | 5678 | 84 | `aedd115b03b5c5dcb754bb5a8025c6b063c7f7484066234aea9f11ef5048d01f` | `bb765c87a97b51a88488848d60055d9e` |
| 6 | `step2/roles_patch_planner_status.sql` | roles-patch | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` | `7ceb6fae8e556e3ee8d80d08ff3f7b41` |
| 7 | `step2/roles_patch_tool_call_audit.sql` | roles-patch | sql | 2908 | 41 | `78f6bc1c62aa788699af3984084d84edf12d344dd6938d8157ffdcf9b65899a8` | `b6fd69f62ed097ed5cc6c29c1f8bf1fa` |
| 8 | `step2b/roles_2b.sql` | roles | sql | 13448 | 242 | `37216bd7987df7be1a2875ffd21a709ed6f2fe5a532cc3e83ec919c9cd2e7082` | `97b4c9fa0e7d2458bad7fa706c4c6af0` |
| 9 | `docs/history/obsolete/roles_2b_patch_section3.sql` | roles-patch-obsolete | sql | 1579 | 26 | `6dff73f212efba97a30d36c7fec7ee0bf2e46be8bff3dd714201d10ac8d6c0b7` | `163c97a1368fe023c59ff914b48a2a34` |
| 10 | `Dockerfile` | container | dockerfile | 2303 | 45 | `52ab907ac0ec615b9daa2de24ac838d9c96beaf27b7508ee618953c9cebc1c3e` | `466b8862c94cac583e18d7c1cd59ee42` |
| 11 | `sandbox/Dockerfile` | container | dockerfile | 2171 | 45 | `ba734449566e19a9e925843690fe2ad2bd81c3c360ba45e446679fae5a78f85b` | `67172e86a9f7b19137d345d847da54d6` |
| 12 | `docker-compose.yml` | container | yaml | 6880 | 163 | `5d06db44f4dfe8ab7d5a30116a1c6d2f141d436602f5ee89ddb78267a4d9294d` | `c13969907289a8dc7f0125fecbafffee` |
| 13 | `tests/test_v0.py` | test | python | 18537 | 519 | `88a3ffdf56dc62f967ce516a7aa5bc650dbf7cd2eafbe67de093c4abd94ffa95` | `e8fd8c4af59d65b97f1da340209d46ff` |
| 14 | `step2/test_roles.py` | test | python | 11297 | 241 | `3dcd56ff42d4c400f7d5267358525f9ab36cbbeee5ca8c71d1bb40c6a22d544d` | `29a8dc8db9673dfd867ca6e31d870726` |
| 15 | `step2b/test_roles_2b.py` | test | python | 13409 | 319 | `9266b853e06c0fb6084f8b02174016f697cc67a28d3582136482645a15465645` | `d8b6a10a9be420a1dd6c90343dfdba51` |
| 16 | `.env.example` | config | dotenv | 1175 | 26 | `2fec247f468cf38696499cb9e683f245a990f0f9b4e82264e9fea6eec354c416` | `99cc1efa120aa2ef52209a9e3b90f077` |
| 17 | `rust_fixes/dispatcher.rs` | patch | rust | 7030 | 161 | `672cb96dde154bdefd7576a84d8a0ffdd4fe849e76985c02c9e5a3684f05d9e2` | `0e095b8e92951a36facaafbd49025a04` |

## 5. Исключения (сознательно не включены)

- `.env` — **секреты** (пароли ролей, API-ключи LLM). В бандл не попадает.
- `.env.bak.*` — резервные копии с секретами.
- `target/` — артефакты сборки Rust.
- `.venv/`, `__pycache__/`, `.pytest_cache/` — окружение и кэш тестов.

Вместо `.env` включён `.env.example` (структура без значений).

## 6. Как проверить

1. Целостность бандла:

   ```bash
   sha256sum docs/audit/SCRIPTS_BUNDLE_v0.4.xml
   # ожидается: 1a875d44f7fde6480d352689d6b308e729645796529c64e6bc47034a559bb32e
   ```

2. Извлечь файлы и сверить хеши (пример для одного файла):

   ```bash
   python3 - <<'PY'
   import xml.etree.ElementTree as ET, hashlib
   r = ET.parse('docs/audit/SCRIPTS_BUNDLE_v0.4.xml').getroot()
   for f in r.find('files'):
       body = (f.find('content').text or '')
       h = hashlib.sha256(body.rstrip('\n').encode()).hexdigest()
       print(f.get('path'), h == f.get('sha256'))
   PY
   ```

3. Пересобрать с нуля и сравнить:

   ```bash
   python3 scripts/build_audit_bundle.py
   md5sum docs/audit/SCRIPTS_BUNDLE_v0.4.xml
   ```

## 7. Оговорка о хешах содержимого

Хеши в манифесте считаны с **исходных файлов на диске**. В `<content>` текст записан без завершающего перевода строки, поэтому `sha256` из манифеста не обязан совпасть с хешем извлечённого текста — для побайтовой сверки используйте сами файлы репозитория. Манифест и есть эталон.
