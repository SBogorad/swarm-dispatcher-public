# Аудит-бандл скриптов — отчёт

**Проект:** `swarm-dispatcher`
**Версия бандла:** v0.4
**Сгенерировано:** 2026-10-10T21:40:42+07:00
**Генератор:** `scripts/build_audit_bundle.py`
**Артефакт:** `docs/audit/SCRIPTS_BUNDLE_v0.4.xml`

---

## 1. Назначение

Единый машиночитаемый снимок **всех скриптов** репозитория (инициализация БД, схема, роли, контейнеризация, тесты, конфиг без секретов) для независимого аудита. Каждый файл включён с полным текстом, размером, числом строк и двумя хешами (`sha256`, `md5`), что позволяет проверить целостность и воспроизводимость.

## 2. Сводка

- Файлов в бандле: **16**
- Суммарный объём исходников: **106443 байт** (~103.9 КиБ)
- Суммарно строк: **2382**
- Размер XML-бандла: **114668 байт**
- `sha256` бандла: `56973013a9ade7b354e8495f0dbcd858e3b3591d1b34143a45e84b2d1f81d302`
- `md5` бандла: `f0e2dd94d66553d6527bd25cc1bf9298`

## 3. Состав по ролям

| Роль | Файлов | Байт |
|------|-------:|-----:|
| Тесты | 3 | 41990 |
| Схема БД | 1 | 23179 |
| Роли и гранты | 2 | 17613 |
| Контейнеризация | 3 | 9126 |
| Вспомогательные патчи | 1 | 7030 |
| Патчи грантов | 2 | 3396 |
| Инициализация БД (bash) | 3 | 2934 |
| Конфиг (без секретов) | 1 | 1175 |

## 4. Манифест файлов

| # | Путь | Роль | Язык | Байт | Строк | sha256 | md5 |
|--:|------|------|------|-----:|------:|--------|-----|
| 1 | `scripts/02-roles.sh` | init | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` | `222f2961a86f5beb870842a67892823f` |
| 2 | `scripts/03-roles-2b.sh` | init | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` | `7641d32f6d09c4e07ae237a90945a7a5` |
| 3 | `scripts/03-role-patches.sh` | init | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` | `fb95baf99f0113e70b2be398967fad25` |
| 4 | `schema/schema_v0.sql` | schema | sql | 23179 | 479 | `4f9ea6398fbd7f9671a3b3d3fd280252c12615304ba9cd4247e3f64b2bcd7482` | `01813aa838cdb12cc459ffd369cf51a7` |
| 5 | `step2/roles.sql` | roles | sql | 5275 | 80 | `bfdabc1b44d7fe136eb1e551fb631c45fc8c65e5f29ffd9eba4080efa2a312a6` | `eddd4e48a250d11e6ad1aea8e948c720` |
| 6 | `step2/roles_patch_planner_status.sql` | roles-patch | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` | `7ceb6fae8e556e3ee8d80d08ff3f7b41` |
| 7 | `step2/roles_patch_tool_call_audit.sql` | roles-patch | sql | 2630 | 38 | `c75ef22471ea33a9731eaae95b73424b0f0926437324f8f3a3bda5811fabbf9a` | `bb9915c1806053d510028c5b0aedc5a7` |
| 8 | `step2b/roles_2b.sql` | roles | sql | 12338 | 232 | `177fb6183080898d52159e1fb506b0334ad2e0dd3e12477ae739687a23567afd` | `0dfe745c65efd0a62298c8843e6f70b1` |
| 9 | `Dockerfile` | container | dockerfile | 1477 | 35 | `add45b22cb1a278bdce041848d4ff43fe12e30f6e5f6b0d63f1df8968e71ce0c` | `6f42122a37fa891b6b5d18bca645c3b8` |
| 10 | `sandbox/Dockerfile` | container | dockerfile | 2171 | 45 | `ba734449566e19a9e925843690fe2ad2bd81c3c360ba45e446679fae5a78f85b` | `67172e86a9f7b19137d345d847da54d6` |
| 11 | `docker-compose.yml` | container | yaml | 5478 | 141 | `73f6f467afdffbcd96c67023827b101aff2355b3dfccc45c9338f5e9b59f5ce5` | `a834e621209f0a2917dc2989d21577a6` |
| 12 | `tests/test_v0.py` | test | python | 18537 | 519 | `88a3ffdf56dc62f967ce516a7aa5bc650dbf7cd2eafbe67de093c4abd94ffa95` | `e8fd8c4af59d65b97f1da340209d46ff` |
| 13 | `step2/test_roles.py` | test | python | 11297 | 241 | `3dcd56ff42d4c400f7d5267358525f9ab36cbbeee5ca8c71d1bb40c6a22d544d` | `29a8dc8db9673dfd867ca6e31d870726` |
| 14 | `step2b/test_roles_2b.py` | test | python | 12156 | 303 | `1d4887b4544cde1fbd7547791a120e752c8181e3c3413102f0d938cf479f8bc7` | `e09f23f333ada151fcea185128884d41` |
| 15 | `.env.example` | config | dotenv | 1175 | 26 | `2fec247f468cf38696499cb9e683f245a990f0f9b4e82264e9fea6eec354c416` | `99cc1efa120aa2ef52209a9e3b90f077` |
| 16 | `rust_fixes/dispatcher.rs` | patch | rust | 7030 | 161 | `672cb96dde154bdefd7576a84d8a0ffdd4fe849e76985c02c9e5a3684f05d9e2` | `0e095b8e92951a36facaafbd49025a04` |

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
   # ожидается: 56973013a9ade7b354e8495f0dbcd858e3b3591d1b34143a45e84b2d1f81d302
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
