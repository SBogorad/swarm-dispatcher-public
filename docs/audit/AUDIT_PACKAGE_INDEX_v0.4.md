# Аудит-пакет проекта `swarm-dispatcher` — индекс

**Сгенерировано:** 2026-10-08T22:47:18+07:00  
**Генератор:** `scripts/build_project_bundle.py` + `scripts/build_audit_index.py`  
**Схема:** `swarm-dispatcher.audit-bundle.v2`

Предназначено для передачи **внешнему аудитору (LLM)**: полный исходный код, инфраструктура и документация в машиночитаемых XML-томах. Каждый файл — с полным текстом и двумя хешами (`sha256`, `md5`). Секреты исключены.

## Состав пакета

| Том | Содержание | Файлов | Строк | Исходники, байт | XML, байт |
|-----|------------|-------:|------:|----------------:|----------:|
| `CODE` | Код проекта (Rust + тесты + манифесты) | 37 | 7272 | 233113 | 250888 |
| `INFRA` | Инфраструктура (SQL, init-скрипты, контейнеры, конфиг) | 23 | 2774 | 120300 | 131655 |
| `DOCS_CORE` | Ключевая документация (ТЗ, архитектура, ограничения, отчёты) | 24 | 5110 | 244432 | 256178 |
| `DOCS` | Документация и отчёты (без писем history/inbound) | 41 | 9463 | 566219 | 585740 |
| **Итого** | | **125** | | **1164064** | **1224461** |

## Хеши томов (для передачи аудитору)

### CODE — `PROJECT_CODE_BUNDLE_v0.4.xml`

- Размер: 250888 байт
- `sha256`: `327bc9027202a123d92017133ef70556a398d54c6b3c33be666a31603038b3a4`
- `md5`: `efa238772a2987f45ea23cf02f62aed8`

### INFRA — `PROJECT_INFRA_BUNDLE_v0.4.xml`

- Размер: 131655 байт
- `sha256`: `e05678cf1d5f250e78a6ace39bd26eafe7e28eb9a057d4d06a919f528b9588b9`
- `md5`: `c00dc63fd2fe5c32f074ed84d9122e97`

### DOCS_CORE — `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml`

- Размер: 256178 байт
- `sha256`: `59fd75c0b0619e71dfd787f8d2d1a95fe4d8f2b5f0f1f19b35b41bb74cb26b01`
- `md5`: `ae0e8df0bbcea129a9b4f2057084c5aa`

### DOCS — `PROJECT_DOCS_BUNDLE_v0.4.xml`

- Размер: 585740 байт
- `sha256`: `af8b9835e858ab1b5b4a3ff2ef4eaf11fdc57e215f4f74f2cd04e51c230492bc`
- `md5`: `b956ffe78bbaef2deb606043a3af6946`

## Исключено сознательно

- `.env`, `.env.bak.*` — **секреты** (пароли ролей, ключи LLM).
- `target/`, `.venv/`, `__pycache__/`, `.pytest_cache/` — сборка/кэш.
- `.docx`, `.png` и прочие бинарники.
- `docs/history/inbound/*` — переписка (не код; при необходимости — отдельно).

Вместо `.env` включён `.env.example` (структура без значений).

## Манифест по томам

### CODE (37 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `Cargo.lock` | toml | 62674 | 2516 | `5efc1e67c0c1c4fb0a3971ca02fc3ecb05aaf31f9d4b84ddc49605b638c0f4fb` |
| 2 | `Cargo.toml` | toml | 730 | 31 | `f8ee2ee887c855f7da8158f2093368b84c3d67bbcbe0c220d5a0f50d3ca968b7` |
| 3 | `crates/swarm-cli/Cargo.toml` | toml | 225 | 10 | `43cacce0e3960bb63925656b3ac96ccece391d295e041a7242844b134eb13f46` |
| 4 | `crates/swarm-cli/src/main.rs` | rust | 4033 | 130 | `f1fba43c7d43330d4e10e997e47b82dd0095df8aaaf1eec94c001f9cacbbd0b7` |
| 5 | `crates/swarm-core/Cargo.toml` | toml | 345 | 17 | `aa491b9d7ab721880f0f86efaa5c02875e9d3ab1a1dba37e474f2c70d81c1f40` |
| 6 | `crates/swarm-core/src/dispatcher.rs` | rust | 11985 | 346 | `be2649c6e4d578ea61888c77fff90569dd615dc47442c86fe5cd0b96eddcebee` |
| 7 | `crates/swarm-core/src/errors.rs` | rust | 6902 | 186 | `8694d6033e58b37a73a483ae5d78db110197ec5303e4b7de69572a653c917172` |
| 8 | `crates/swarm-core/src/lib.rs` | rust | 242 | 9 | `164742edff97d3d190d7fe86e29a19ca2f764e152749114e52ebe53bf952eb1e` |
| 9 | `crates/swarm-core/src/types.rs` | rust | 8610 | 256 | `76d19cc69baa85b22c204697e09cac550b5eea905aff975a6f036c5f93c6173e` |
| 10 | `crates/swarm-core/tests/alt/integration_alt.rs` | rust | 4978 | 138 | `86386804ed5985fe27646e3f577ac915b7fd2438576fb83e204591bf14496e1f` |
| 11 | `crates/swarm-core/tests/integration.rs` | rust | 10883 | 322 | `3eeb2feabce848250718bef77e9276558f9b6ea58e59b524b01f228514e58ee6` |
| 12 | `crates/swarm-core/tests/integration_verified.rs` | rust | 7797 | 185 | `4ff525bfa48324644d719edeb1b4c6f4f1a7c23ada9a7c4761c2b78bbf6a8bc5` |
| 13 | `crates/swarm-llm/Cargo.toml` | toml | 367 | 11 | `f3da8011038d37fc95332a7158f11c0ba44141d51e85842acd62d682276ab86b` |
| 14 | `crates/swarm-llm/src/error.rs` | rust | 1147 | 43 | `83ed168ea6bab9202c19d61f06106e2878fecd62b007a0b179cbdbfa8010445e` |
| 15 | `crates/swarm-llm/src/lib.rs` | rust | 13118 | 396 | `384ae976022bdd2f06a3197cfdd285a2d16001eb646337e9684204308db34e3f` |
| 16 | `crates/swarm-planner/Cargo.toml` | toml | 557 | 24 | `abe2418a29d3ea9ec317ad9f5f82cbe947d6cc1ad708a25a04f1220f65171bdc` |
| 17 | `crates/swarm-planner/src/error.rs` | rust | 516 | 23 | `cec97ee8df913e6a866ea76742a0fb127aace1f46b2e3f16c0e5470b8e5f37f8` |
| 18 | `crates/swarm-planner/src/lib.rs` | rust | 14005 | 348 | `7d51ec96ec9eb778dd904b4c12515f30034929303259d25c1af7dc8c04222e47` |
| 19 | `crates/swarm-planner/src/llm.rs` | rust | 4168 | 117 | `7a43f04d3358fbfbb8c770a567beca8e486ef8b2fd9fc3efee02ef001f2c7424` |
| 20 | `crates/swarm-planner/src/main.rs` | rust | 2474 | 80 | `69d0a96d4ffb930817413f803a806e27cfbe7eceefb990e8f283a255cb193de4` |
| 21 | `crates/swarm-planner/src/parse.rs` | rust | 11215 | 341 | `2a5e186e8a9702d0a6bac7f4cf59710565aca94fee923872f13446bace491c28` |
| 22 | `crates/swarm-planner/src/prompt.rs` | rust | 2615 | 74 | `ddeeeb231391bba0730609200a326b54e169faa36ff713d1045648926289f8e7` |
| 23 | `crates/swarm-planner/tests/fixtures/planner_dup_dep.json` | json | 745 | 31 | `a2853ef4c1f243da155cba4436f9e7b6fc733094331903ec3df7252497cc2eb1` |
| 24 | `crates/swarm-planner/tests/fixtures/planner_ok.json` | json | 935 | 31 | `2044fcfa6c1dc51ffb12bc6b959f4609e88b22f62964eae8a94351d52ca88d09` |
| 25 | `crates/swarm-planner/tests/fixtures/planner_too_many.json` | json | 340 | 18 | `6660c81d7e32c0b292251d83d8c673218a4c5ceae865b285574b615d33b0f22c` |
| 26 | `crates/swarm-planner/tests/planner_integration.rs` | rust | 9534 | 247 | `397d9696a4e40c8328abd0c656b142abf1a4386cb57cf8e911ab4514d961517c` |
| 27 | `crates/swarm-watchdog/Cargo.toml` | toml | 227 | 10 | `214a53d97e923842faf74e5a0f30ae8ea4277b65b75d74554f22fc20a45e6fab` |
| 28 | `crates/swarm-watchdog/src/main.rs` | rust | 3101 | 83 | `bf5c0ec2ce0306af83a874ce768a409e796ad3471f926090fdce35c2074c500b` |
| 29 | `crates/swarm-worker/Cargo.toml` | toml | 574 | 24 | `ad0c587a97e00a6fe291ce16d12cb4b9bcb76f3641726afd88c95279c6987828` |
| 30 | `crates/swarm-worker/src/artifacts.rs` | rust | 7268 | 173 | `d336bd23bbfe215c2504ecde4b1d4d1eb7e876fbe9c662713b443e4f2398790e` |
| 31 | `crates/swarm-worker/src/error.rs` | rust | 565 | 26 | `00a0eefef095ac15c955894f9c675280bdc0f9376809d83a515dfb522830e091` |
| 32 | `crates/swarm-worker/src/lib.rs` | rust | 519 | 14 | `8383df34973fed351be2efa02705af5a991ba2649533b8d3c312d61f7cca1e04` |
| 33 | `crates/swarm-worker/src/main.rs` | rust | 4954 | 126 | `c9b6c2d3438609701fe757e802843c98371b7e82fac187e18367fb5a52ed3163` |
| 34 | `crates/swarm-worker/src/prompt.rs` | rust | 5343 | 120 | `f908519c1de7ee1ea752e941abd54ad6e6e24cd7c15a288191f9e70c85af9704` |
| 35 | `crates/swarm-worker/src/retry.rs` | rust | 3065 | 87 | `56768d8703991ecec0bf1ce3ff0e1b7f933ee94815780b538faba093b5c5476f` |
| 36 | `crates/swarm-worker/src/worker.rs` | rust | 9236 | 254 | `135d7e8dc378db4f3e6511610c0fe5d37e965834c9379f967c033e11f174b23f` |
| 37 | `crates/swarm-worker/tests/worker_integration.rs` | rust | 17121 | 425 | `05e45fc281bff481b08ca05863e9ed63d69bfc1c32c351158b83896928b11977` |

### INFRA (23 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `.dockerignore` | text | 59 | 6 | `09ba81543a788f3a573bf9461060e75cc513cc3323650524a0f70a8e12e39ee5` |
| 2 | `.env.example` | dotenv | 740 | 17 | `ae9b238982794808c38b9856e9268dfedb732555a7ffc6ba697f41176977ad14` |
| 3 | `.gitignore` | text | 136 | 13 | `02f25f52e2540397c7b1b05a59f4d28270ab2766b018e1067d07afe61b5a77a2` |
| 4 | `Dockerfile` | dockerfile | 1477 | 35 | `add45b22cb1a278bdce041848d4ff43fe12e30f6e5f6b0d63f1df8968e71ce0c` |
| 5 | `LICENSE` | text | 1104 | 21 | `6a0f51d5703ca8719a56f87b7a5fb84a8c773f5e361487a1b28fe6580044dff9` |
| 6 | `docker-compose.yml` | yaml | 4907 | 133 | `4d08704c506630428606fb3386965e698988d6e9cb3691b886cadcd49ba39162` |
| 7 | `schema/roles.sql` | sql | 151 | 1 | `24d154d1c1c3ec534b69cb7736dc454e944918630d2ca049f4bf05dc2761b979` |
| 8 | `schema/schema_v0.sql` | sql | 22375 | 458 | `04792f46abe134f216b1dff4ad07d36041559d787441d1b6a99eb3b8f4362ecf` |
| 9 | `scripts/02-roles.sh` | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` |
| 10 | `scripts/03-role-patches.sh` | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` |
| 11 | `scripts/03-roles-2b.sh` | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` |
| 12 | `scripts/build_audit_bundle.py` | python | 6718 | 180 | `5b2f10166caeba3fe3310c923ab1aaf536b8c3364278388a302ac1de35fc5906` |
| 13 | `scripts/build_audit_index.py` | python | 5567 | 128 | `d528b9dd103f1e2f03ea8ec5ff74090d190d9e198fd710acf7a351031d7ded7a` |
| 14 | `scripts/build_audit_report.py` | python | 7393 | 162 | `ba87780d987945f3fb45ad036f16bd37f06c0ab02f7cce152d65771823c26c8c` |
| 15 | `scripts/build_project_bundle.py` | python | 7882 | 189 | `7987d4a053cfd71f7766c1bb95e64d2bd65b5d518fd2b8d74bfda52fbfaff3aa` |
| 16 | `step2/roles.sql` | sql | 5122 | 77 | `65ed077ccee3eb515d3585c1e7a168c52d9798735b97f4d8f482fa606ebe5274` |
| 17 | `step2/roles_patch_planner_status.sql` | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` |
| 18 | `step2/test_roles.py` | python | 11162 | 238 | `48b4e2c6c0592aff876660936ea07a323e9a318d241f41380eb0d9fcaeb8f2f2` |
| 19 | `step2b/roles_2b.sql` | sql | 10409 | 199 | `2798101331f945e5e4dce66e881de1742c7d0799893fd51df0c8d6e3fce93297` |
| 20 | `step2b/roles_2b_patch_section3.sql` | sql | 1579 | 26 | `6dff73f212efba97a30d36c7fec7ee0bf2e46be8bff3dd714201d10ac8d6c0b7` |
| 21 | `step2b/test_roles_2b.DELIVERED_partial.py` | python | 3477 | 97 | `156146fb1bc1df3392c7b3bec215510db62dd7e4c910e8653c364a36a00d7a94` |
| 22 | `step2b/test_roles_2b.py` | python | 8859 | 224 | `49670ce179dbd56db352527cf9699ae08a706a20a468c49d9a600e7ae46d2356` |
| 23 | `tests/test_v0.py` | python | 17483 | 488 | `fa27fcb25b4ffa9d6ea37877f5e8441a2b00307ceba00ae569c5281e8952e53f` |

### DOCS_CORE (24 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 4524 | 95 | `799478b5e46cb28c06a3b8ca68e700a93ecd50701adac820d5487fa3ddc1b51a` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 8266 | 225 | `15aa902e5f8726b5943708a0cf85fd66e14ee49f5d166d2c293fe075372bfb6f` |
| 4 | `REPORT_docker_gap_closed.md` | markdown | 10675 | 143 | `8f6ba2f8366312e844ae755c939e28873ce941a66a78a2a74bdb05ab5de10760` |
| 5 | `REPORT_v0.2_step1.md` | markdown | 9901 | 163 | `75d5b685e7b18f45d606d7a1e66dbe3943d1aa774990915574ac4f406dd0376a` |
| 6 | `REPORT_v0.2_step2_native_verify.md` | markdown | 5669 | 112 | `cc098a99af0b02a2c6144d7d8b44da9c69238b6e3924834ccbee1dd0c90036bf` |
| 7 | `REPORT_v0.2_step2_planner.md` | markdown | 11777 | 182 | `f474c16005df9fb6eaff932b8244582e8ac90f7665452c5f67f35a62c0c7417c` |
| 8 | `REPORT_v0.2_step2b.md` | markdown | 11521 | 188 | `17c6c01f82a457d90320259db4a6489f00f61ac326a46ec531d955c7f5a7e7c5` |
| 9 | `REPORT_v0.2_step2b_v2.md` | markdown | 8962 | 167 | `ae86e556ea31c76098a2773f19de2640133a057f78be4d30dd174b2b28c76a8f` |
| 10 | `REPORT_v0.3.1_planner_description.md` | markdown | 12208 | 207 | `8f6f8bf743261a865833aadac8bb7070e8aab86b2c675397e6c785dadb4f7182` |
| 11 | `REPORT_v0.3_llm_worker.md` | markdown | 12201 | 204 | `6a3fef555bdb9ac33e934151f4805b52c2e7f102d74f3975462271c2cf45888b` |
| 12 | `RESUME.md` | markdown | 16782 | 355 | `097dac9b7b545570b2929b92fd047f3b98cc9187073ab06d5d4f04b04d273359` |
| 13 | `docs/ARCHITECTURE.md` | markdown | 12226 | 381 | `4c800238c5ce8398b72f182c8fbb6ca81310d30b3461886d55421ca0b63caa31` |
| 14 | `docs/KNOWN_LIMITATIONS.md` | markdown | 11215 | 220 | `32e1ff6ca43d5d65a8874499aea7da4567915b9817e12e6d21ccd91fe462fe5d` |
| 15 | `docs/METHODOLOGY.md` | markdown | 14539 | 242 | `d0fdfa915879e43aff622687afd730c0b6306cfd422c92ddab0f4455404a4dcf` |
| 16 | `docs/PROJECT_STATE.md` | markdown | 5027 | 84 | `760f5a56059e54cde0d16921435f04d9f4f7f4376147a8ab4aa038afe5df7f97` |
| 17 | `docs/TZ_v0.2_step1_fixes.md` | markdown | 13920 | 398 | `994f7dae8362bde94e86ffc8df3ce2668b17c6de5138fbcd4b0bc042302a721a` |
| 18 | `docs/TZ_v0.2_step2_planner.md` | markdown | 12895 | 325 | `793f2523b85ce94448d0a96c149c35a916c7180dc6e24ffa58f3642e4155aa91` |
| 19 | `docs/TZ_v0.2_step2b_worker_identity.md` | markdown | 9783 | 200 | `74cdd5f0d13baa450787e8838092579399fd95e52ed833aea5152d8b00e72b95` |
| 20 | `docs/TZ_v0.3.1_planner_description.md` | markdown | 8264 | 230 | `0ed36aebcb2a83a397e13d3432e874d9d0931439205b358b4026ea5b53f7420a` |
| 21 | `docs/TZ_v0.3_llm_worker.md` | markdown | 11499 | 290 | `b0e6cfe44bf81b7277ff6c6933705d40d265c058ee46c98132cd54cf09a01627` |
| 22 | `docs/TZ_v0.4_shell_tools_sandbox.md` | markdown | 11169 | 287 | `67eaa042042ecee383dc9e0ac8a6f9b4ccb131935f7123ef27ec565a0d7b036b` |
| 23 | `docs/VERIFICATION_LOG.md` | markdown | 4585 | 109 | `aff4b00dc511a98bc6e08ead1f33293113573538e87ab51684d83f0c7281fe57` |
| 24 | `docs/audit/AUDITOR_BRIEF_v0.4.md` | markdown | 10541 | 164 | `a50fedc370bbdebcdc991c5bd0219ff1063acdc41e409107eac53a1c623e4a8a` |

### DOCS (41 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 4524 | 95 | `799478b5e46cb28c06a3b8ca68e700a93ecd50701adac820d5487fa3ddc1b51a` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 8266 | 225 | `15aa902e5f8726b5943708a0cf85fd66e14ee49f5d166d2c293fe075372bfb6f` |
| 4 | `REPORT_docker_gap_closed.md` | markdown | 10675 | 143 | `8f6ba2f8366312e844ae755c939e28873ce941a66a78a2a74bdb05ab5de10760` |
| 5 | `REPORT_v0.2_step1.md` | markdown | 9901 | 163 | `75d5b685e7b18f45d606d7a1e66dbe3943d1aa774990915574ac4f406dd0376a` |
| 6 | `REPORT_v0.2_step2_native_verify.md` | markdown | 5669 | 112 | `cc098a99af0b02a2c6144d7d8b44da9c69238b6e3924834ccbee1dd0c90036bf` |
| 7 | `REPORT_v0.2_step2_planner.md` | markdown | 11777 | 182 | `f474c16005df9fb6eaff932b8244582e8ac90f7665452c5f67f35a62c0c7417c` |
| 8 | `REPORT_v0.2_step2b.md` | markdown | 11521 | 188 | `17c6c01f82a457d90320259db4a6489f00f61ac326a46ec531d955c7f5a7e7c5` |
| 9 | `REPORT_v0.2_step2b_v2.md` | markdown | 8962 | 167 | `ae86e556ea31c76098a2773f19de2640133a057f78be4d30dd174b2b28c76a8f` |
| 10 | `REPORT_v0.3.1_planner_description.md` | markdown | 12208 | 207 | `8f6f8bf743261a865833aadac8bb7070e8aab86b2c675397e6c785dadb4f7182` |
| 11 | `REPORT_v0.3_llm_worker.md` | markdown | 12201 | 204 | `6a3fef555bdb9ac33e934151f4805b52c2e7f102d74f3975462271c2cf45888b` |
| 12 | `RESUME.md` | markdown | 16782 | 355 | `097dac9b7b545570b2929b92fd047f3b98cc9187073ab06d5d4f04b04d273359` |
| 13 | `docs/ARCHITECTURE.md` | markdown | 12226 | 381 | `4c800238c5ce8398b72f182c8fbb6ca81310d30b3461886d55421ca0b63caa31` |
| 14 | `docs/KNOWN_LIMITATIONS.md` | markdown | 11215 | 220 | `32e1ff6ca43d5d65a8874499aea7da4567915b9817e12e6d21ccd91fe462fe5d` |
| 15 | `docs/METHODOLOGY.md` | markdown | 14539 | 242 | `d0fdfa915879e43aff622687afd730c0b6306cfd422c92ddab0f4455404a4dcf` |
| 16 | `docs/PROJECT_STATE.md` | markdown | 5027 | 84 | `760f5a56059e54cde0d16921435f04d9f4f7f4376147a8ab4aa038afe5df7f97` |
| 17 | `docs/TZ_v0.2_step1_fixes.md` | markdown | 13920 | 398 | `994f7dae8362bde94e86ffc8df3ce2668b17c6de5138fbcd4b0bc042302a721a` |
| 18 | `docs/TZ_v0.2_step2_planner.md` | markdown | 12895 | 325 | `793f2523b85ce94448d0a96c149c35a916c7180dc6e24ffa58f3642e4155aa91` |
| 19 | `docs/TZ_v0.2_step2b_worker_identity.md` | markdown | 9783 | 200 | `74cdd5f0d13baa450787e8838092579399fd95e52ed833aea5152d8b00e72b95` |
| 20 | `docs/TZ_v0.3.1_planner_description.md` | markdown | 8264 | 230 | `0ed36aebcb2a83a397e13d3432e874d9d0931439205b358b4026ea5b53f7420a` |
| 21 | `docs/TZ_v0.3_llm_worker.md` | markdown | 11499 | 290 | `b0e6cfe44bf81b7277ff6c6933705d40d265c058ee46c98132cd54cf09a01627` |
| 22 | `docs/TZ_v0.4_shell_tools_sandbox.md` | markdown | 11169 | 287 | `67eaa042042ecee383dc9e0ac8a6f9b4ccb131935f7123ef27ec565a0d7b036b` |
| 23 | `docs/VERIFICATION_LOG.md` | markdown | 4585 | 109 | `aff4b00dc511a98bc6e08ead1f33293113573538e87ab51684d83f0c7281fe57` |
| 24 | `docs/history/AUDITED_DOC_v15.md` | markdown | 135883 | 684 | `cc702322793060d986f9420e1178951d2c5ced8c9232637cbfc1a0e316c1d6ed` |
| 25 | `docs/history/README.md` | markdown | 4795 | 109 | `e1690bc6c81a77243e2e1c522362630d56b99fb474ced4d8b1030eba4c977e2e` |
| 26 | `docs/history/TZ_v0.2_step1_fixes_v1.md` | markdown | 16123 | 386 | `e38ac008e8ac952f911aa3360c1064ac84fba0f6a3316168e0d7a5e9768de309` |
| 27 | `docs/history/TZ_v0.2_step2_planner_v1.md` | markdown | 9648 | 257 | `f23f444e5e3c276601aa48d8b0a9d3ae3ebd8ba4cb5d7b53688cd1dc4d8ca90b` |
| 28 | `docs/optional/ARCHITECTURE_DECISIONS.md` | markdown | 16395 | 265 | `d5791d18ee7da97b4181ee70d0c6d77654cede38137345251f52fa6ae85ad98e` |
| 29 | `docs/optional/AUDIT_HISTORY.md` | markdown | 22591 | 396 | `5d8ce99ea426b8bad568b368c38c08a468463fb67bc33c43924632286d89dd78` |
| 30 | `docs/optional/BRIEF_FOR_AGENTS.md` | markdown | 10812 | 227 | `8ce3070c8141cab0377068d68caa41871a1aba6a86e3b561ed92ffd6c1f7e3a7` |
| 31 | `docs/optional/CLAUDE_DIALOGUE.md` | markdown | 11641 | 190 | `f337b70f3578ceb7ff4991b7570c967294beb172e62dca1e03897dd76df09dce` |
| 32 | `docs/optional/DOC_STRATEGY.md` | markdown | 10857 | 235 | `ca4b24d93476b01f6545ddfee373def01dd2cf2fb589e963bf35c7aa9c7fa22a` |
| 33 | `docs/optional/ERRORS.md` | markdown | 9837 | 214 | `eb7c8892ee7a0086e1f02ef44e008e51ce9a7f3f9484f13d9e6bce8b4690d7f1` |
| 34 | `docs/optional/NATIVE_SETUP.md` | markdown | 10539 | 327 | `7bfb08a7950dbec6871eb960a3f0ae0ce1608b8a9ca655ecf56dbd04ac2ba29d` |
| 35 | `docs/optional/NEXT_STEPS.md` | markdown | 8042 | 158 | `7b44843489ab61b314fb29e54dbd07a0bc69fde1076cedc441c78fd4adde0a3f` |
| 36 | `docs/optional/PLATFORM_COMPARISON.md` | markdown | 13790 | 200 | `27d89af9aedf6e2b3249da72b6d25f0d0de3c9ac6b44dd04e01cf021a8ea941a` |
| 37 | `docs/optional/REPORT_TEMPLATE.md` | markdown | 7475 | 202 | `cfb6796f9a34322ae2d4a2202562bb4e6a5fcbac1e9f6628dfbed13514e6603b` |
| 38 | `docs/optional/ROADMAP.md` | markdown | 10374 | 247 | `ef98362758829f76c526614b179ef2c4b6325923f0c85e5834eb759a6abaaffa` |
| 39 | `docs/optional/RUST_FINDINGS.md` | markdown | 3668 | 30 | `538039fed69765820f6e9c9e508afbeb34eddf0f3f3a227001b49deafce2ddaa` |
| 40 | `docs/optional/THREAT_MODEL.md` | markdown | 19399 | 297 | `088a4e79b375a3b9e09997bcd784b95f1b6521bc11b81671c802ff5b003c33db` |
| 41 | `docs/optional/TZ_v0.2_step2_roles.md` | markdown | 10459 | 93 | `b22b3428747c656e2673d8b33691e0527b0fe285eee885d7b7e55b2a40cf5b1f` |

## Как читать

```bash
# целостность
sha256sum PROJECT_CODE_BUNDLE_v0.4.xml PROJECT_INFRA_BUNDLE_v0.4.xml PROJECT_DOCS_BUNDLE_v0.4.xml

# список файлов в томе
python3 - <<'PY'
import xml.etree.ElementTree as ET
r = ET.parse('PROJECT_CODE_BUNDLE_v0.4.xml').getroot()
for e in r.find('manifest/index'):
    print(e.get('path'), e.get('bytes'), e.get('sha256')[:16])
PY
```
