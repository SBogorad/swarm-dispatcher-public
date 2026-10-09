# Аудит-бандл скриптов — отчёт

**Проект:** `swarm-dispatcher`  
**Версия бандла:** v0.4  
**Сгенерировано:** 2026-10-08T21:41:58+07:00  
**Генератор:** `scripts/build_audit_bundle.py`  
**Артефакт:** `docs/audit/SCRIPTS_BUNDLE_v0.4.xml`

---

## 1. Назначение

Единый машиночитаемый снимок **всех скриптов** репозитория (инициализация БД, схема, роли, контейнеризация, тесты, конфиг без секретов) для независимого аудита. Каждый файл включён с полным текстом, размером, числом строк и двумя хешами (`sha256`, `md5`), что позволяет проверить целостность и воспроизводимость.

## 2. Сводка

- Файлов в бандле: **17**
- Суммарный объём исходников: **98471 байт** (~96.2 КиБ)
- Суммарно строк: **2236**
- Размер XML-бандла: **107077 байт**
- `sha256` бандла: `4cb29b82c82b8b83b8323a7a1b6b6ca5b8c2996971e1c8fa59b5d099e99de3c0`
- `md5` бандла: `f3e442b87ca4f5582ed63612cd39d781`

## 3. Состав по ролям

| Роль | Файлов | Байт |
|------|-------:|-----:|
| Тесты | 4 | 40981 |
| Схема БД | 1 | 22375 |
| Роли и гранты | 3 | 15682 |
| Вспомогательные патчи | 1 | 7030 |
| Контейнеризация | 2 | 6384 |
| Инициализация БД (bash) | 3 | 2934 |
| Патчи грантов | 2 | 2345 |
| Конфиг (без секретов) | 1 | 740 |

## 4. Манифест файлов

| # | Путь | Роль | Язык | Байт | Строк | sha256 | md5 |
|--:|------|------|------|-----:|------:|--------|-----|
| 1 | `scripts/02-roles.sh` | init | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` | `222f2961a86f5beb870842a67892823f` |
| 2 | `scripts/03-roles-2b.sh` | init | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` | `7641d32f6d09c4e07ae237a90945a7a5` |
| 3 | `scripts/03-role-patches.sh` | init | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` | `fb95baf99f0113e70b2be398967fad25` |
| 4 | `schema/schema_v0.sql` | schema | sql | 22375 | 458 | `04792f46abe134f216b1dff4ad07d36041559d787441d1b6a99eb3b8f4362ecf` | `a45e28945b5b967289b88eeb834866d3` |
| 5 | `schema/roles.sql` | roles | sql | 151 | 1 | `24d154d1c1c3ec534b69cb7736dc454e944918630d2ca049f4bf05dc2761b979` | `9e98b5ff604055990e6e68aac54129a3` |
| 6 | `step2/roles.sql` | roles | sql | 5122 | 77 | `65ed077ccee3eb515d3585c1e7a168c52d9798735b97f4d8f482fa606ebe5274` | `fba1f83b96f54880ea608f46b5aaf56b` |
| 7 | `step2/roles_patch_planner_status.sql` | roles-patch | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` | `7ceb6fae8e556e3ee8d80d08ff3f7b41` |
| 8 | `step2b/roles_2b.sql` | roles | sql | 10409 | 199 | `2798101331f945e5e4dce66e881de1742c7d0799893fd51df0c8d6e3fce93297` | `0cb5c0b4d176e3a13c2cc13689f25233` |
| 9 | `step2b/roles_2b_patch_section3.sql` | roles-patch | sql | 1579 | 26 | `6dff73f212efba97a30d36c7fec7ee0bf2e46be8bff3dd714201d10ac8d6c0b7` | `163c97a1368fe023c59ff914b48a2a34` |
| 10 | `Dockerfile` | container | dockerfile | 1477 | 35 | `add45b22cb1a278bdce041848d4ff43fe12e30f6e5f6b0d63f1df8968e71ce0c` | `6f42122a37fa891b6b5d18bca645c3b8` |
| 11 | `docker-compose.yml` | container | yaml | 4907 | 133 | `4d08704c506630428606fb3386965e698988d6e9cb3691b886cadcd49ba39162` | `229661245c945eb66e16a99fb115d487` |
| 12 | `tests/test_v0.py` | test | python | 17483 | 488 | `fa27fcb25b4ffa9d6ea37877f5e8441a2b00307ceba00ae569c5281e8952e53f` | `e920b59abbd37db1861908a20f7ffe89` |
| 13 | `step2/test_roles.py` | test | python | 11162 | 238 | `48b4e2c6c0592aff876660936ea07a323e9a318d241f41380eb0d9fcaeb8f2f2` | `18d576fe36f26af828bc66e4f8e1f04d` |
| 14 | `step2b/test_roles_2b.py` | test | python | 8859 | 224 | `49670ce179dbd56db352527cf9699ae08a706a20a468c49d9a600e7ae46d2356` | `fddca7ccf11bb6ae2728c4331471013b` |
| 15 | `step2b/test_roles_2b.DELIVERED_partial.py` | test | python | 3477 | 97 | `156146fb1bc1df3392c7b3bec215510db62dd7e4c910e8653c364a36a00d7a94` | `6176c13bcba716db0b171c5ad4cbc5f6` |
| 16 | `.env.example` | config | dotenv | 740 | 17 | `ae9b238982794808c38b9856e9268dfedb732555a7ffc6ba697f41176977ad14` | `881eda95ee6ba793c4433a38767a7641` |
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
   # ожидается: 4cb29b82c82b8b83b8323a7a1b6b6ca5b8c2996971e1c8fa59b5d099e99de3c0
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
