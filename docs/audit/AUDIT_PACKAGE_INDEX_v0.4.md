# Аудит-пакет проекта `swarm-dispatcher` — индекс

**Сгенерировано:** 2026-10-14T09:14:54+07:00
**Генератор:** `scripts/build_project_bundle.py` + `scripts/build_audit_index.py`
**Схема:** `swarm-dispatcher.audit-bundle.v2`

Предназначено для передачи **внешнему аудитору (LLM)**: полный исходный код, инфраструктура и документация в машиночитаемых XML-томах. Каждый файл — с полным текстом и двумя хешами (`sha256`, `md5`). Секреты исключены.

## Состав пакета

| Том | Содержание | Файлов | Строк | Исходники, байт | XML, байт |
|-----|------------|-------:|------:|----------------:|----------:|
| `CODE` | Код проекта (Rust + тесты + манифесты) | 58 | 16246 | 573575 | 601381 |
| `INFRA` | Инфраструктура (SQL, init-скрипты, контейнеры, конфиг) | 60 | 9090 | 424921 | 453345 |
| `DOCS_CORE` | Ключевая документация (ТЗ, архитектура, ограничения, отчёты) | 29 | 6317 | 325032 | 339129 |
| `DOCS` | Документация и отчёты (без писем history/inbound) | 54 | 11536 | 730413 | 755925 |
| **Итого** | | **201** | | **2053941** | **2149780** |

## Хеши томов (для передачи аудитору)

### CODE — `PROJECT_CODE_BUNDLE_v0.4.xml`

- Размер: 601381 байт
- `sha256`: `cf2d3d113759f396d45fe2a1dadac879c32d6d419ae47c841b50ffcb39cde43b`
- `md5`: `e7390143a97becc14faedc69837f60ad`

### INFRA — `PROJECT_INFRA_BUNDLE_v0.4.xml`

- Размер: 453345 байт
- `sha256`: `411d46ae305ae6841f1fcab564e53ba6f6060b9b007c88751c035499978187da`
- `md5`: `a3775df50765fbe58f2ee4d12d25b9f8`

### DOCS_CORE — `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml`

- Размер: 339129 байт
- `sha256`: `e406f882e006bcf137771f28ea343fb6b6f7ca0f1a341c65814f569c05115dcf`
- `md5`: `2598ce32497007318fe452e5d8ceaee7`

### DOCS — `PROJECT_DOCS_BUNDLE_v0.4.xml`

- Размер: 755925 байт
- `sha256`: `0e78021d448f2f12a9abcb20d39dfee2cf681571fed0a32d1f2048a5f0454e69`
- `md5`: `f19fc25b659d45ecf8f26049108007c8`

## Исключено сознательно

- `.env`, `.env.bak.*` — **секреты** (пароли ролей, ключи LLM).
- `target/`, `.venv/`, `__pycache__/`, `.pytest_cache/` — сборка/кэш.
- `.docx`, `.png` и прочие бинарники.
- `docs/history/inbound/*` — переписка (не код; при необходимости — отдельно).

Вместо `.env` включён `.env.example` (структура без значений).

## Манифест по томам

### CODE (58 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `Cargo.lock` | toml | 90735 | 3711 | `fd8c4b2c5ee5a53b005c41ef7d3cf95a587c99b8f57118aa8969eee264dbcc49` |
| 2 | `Cargo.toml` | toml | 896 | 37 | `de0bc01c4830608a03d8668ddda8f3270e52dbabc0dd3873d4c0c17c6f13e4a8` |
| 3 | `crates/swarm-cli/Cargo.toml` | toml | 225 | 10 | `43cacce0e3960bb63925656b3ac96ccece391d295e041a7242844b134eb13f46` |
| 4 | `crates/swarm-cli/src/main.rs` | rust | 6676 | 205 | `7202eb0ae1412fa3686c1067e7281b1d1c3a06de5504b35e17825e2b3c608e2b` |
| 5 | `crates/swarm-cli/tests/cli_exit_codes.rs` | rust | 5221 | 124 | `ef30033ec8bfed4354db9481bee8be88766b834b616e65baeca5b046d1725075` |
| 6 | `crates/swarm-core/Cargo.toml` | toml | 345 | 17 | `aa491b9d7ab721880f0f86efaa5c02875e9d3ab1a1dba37e474f2c70d81c1f40` |
| 7 | `crates/swarm-core/src/dispatcher.rs` | rust | 19248 | 539 | `6be41c75a535fdb6d587644a8e0b61fa1dcc861e233d60f29c734c1f02c57ab7` |
| 8 | `crates/swarm-core/src/errors.rs` | rust | 6946 | 189 | `9fcf70110b95e28e44cf68de7a4edb1f770b7148ed06e055f4d14e635f854489` |
| 9 | `crates/swarm-core/src/lib.rs` | rust | 235 | 7 | `b9210371c22830287ec2681249d835ddc7e793d67a8078ddc5c228586ec277b0` |
| 10 | `crates/swarm-core/src/types.rs` | rust | 10140 | 302 | `b39f3811b77263cc289b9ca482b8af97df96f1d88f180f9b8ab5535625309023` |
| 11 | `crates/swarm-core/tests/alt/integration_alt.rs` | rust | 6600 | 198 | `eacfdbdbf6ec6f8bffffce35221e52c43481ff19caaaa477f256096c38a798bb` |
| 12 | `crates/swarm-core/tests/integration_verified.rs` | rust | 14237 | 427 | `412bfb378aa4ce381c1c7464cad7f01495e16a3599d689ee2ed239fcc839d9f3` |
| 13 | `crates/swarm-llm/Cargo.toml` | toml | 367 | 11 | `f3da8011038d37fc95332a7158f11c0ba44141d51e85842acd62d682276ab86b` |
| 14 | `crates/swarm-llm/src/error.rs` | rust | 1147 | 43 | `83ed168ea6bab9202c19d61f06106e2878fecd62b007a0b179cbdbfa8010445e` |
| 15 | `crates/swarm-llm/src/lib.rs` | rust | 20832 | 559 | `9ee7012beafcce1b960318e1e10d2d5479baf270edadafa3702b4f1f2de049a7` |
| 16 | `crates/swarm-planner/Cargo.toml` | toml | 557 | 24 | `abe2418a29d3ea9ec317ad9f5f82cbe947d6cc1ad708a25a04f1220f65171bdc` |
| 17 | `crates/swarm-planner/src/error.rs` | rust | 516 | 23 | `cec97ee8df913e6a866ea76742a0fb127aace1f46b2e3f16c0e5470b8e5f37f8` |
| 18 | `crates/swarm-planner/src/lib.rs` | rust | 14192 | 367 | `bbfebbee4a08b980cfe48d63ce66ce1db8ee0d20a350c3d37563ab0b6d7ab68c` |
| 19 | `crates/swarm-planner/src/llm.rs` | rust | 4449 | 118 | `3da4666d4c4beef83db225c4ecf6c87b251aae34c8560f045a7f2338ca5b3953` |
| 20 | `crates/swarm-planner/src/main.rs` | rust | 2440 | 78 | `4ce3896b0f96c48037e560f7b05172e51be10a6e6a5e3bdd9d4d07d9c06802cf` |
| 21 | `crates/swarm-planner/src/parse.rs` | rust | 20373 | 567 | `caa725810eabdda0bef1e96ae6c9faca698e3932fd9c243288d07e2de02dcf9c` |
| 22 | `crates/swarm-planner/src/prompt.rs` | rust | 5240 | 130 | `472e1f2deec5b7a88724527c71a95ffe359b8c252d569fd80335035e0b870c40` |
| 23 | `crates/swarm-planner/tests/fixtures/planner_dup_dep.json` | json | 745 | 31 | `a2853ef4c1f243da155cba4436f9e7b6fc733094331903ec3df7252497cc2eb1` |
| 24 | `crates/swarm-planner/tests/fixtures/planner_ok.json` | json | 935 | 31 | `2044fcfa6c1dc51ffb12bc6b959f4609e88b22f62964eae8a94351d52ca88d09` |
| 25 | `crates/swarm-planner/tests/fixtures/planner_too_many.json` | json | 340 | 18 | `6660c81d7e32c0b292251d83d8c673218a4c5ceae865b285574b615d33b0f22c` |
| 26 | `crates/swarm-planner/tests/fixtures/retry_seq/001_broken.json` | json | 52 | 1 | `9fd86e8d5c15b0e265113b8d55be59b4b6f2660045007a2c3ab32bce969d86bf` |
| 27 | `crates/swarm-planner/tests/fixtures/retry_seq/002_ok.json` | json | 935 | 31 | `2044fcfa6c1dc51ffb12bc6b959f4609e88b22f62964eae8a94351d52ca88d09` |
| 28 | `crates/swarm-planner/tests/planner_integration.rs` | rust | 12293 | 313 | `6fd60cf4e010e4576bcc6fc0aa81d9a39eedb4fce9be020557a6eab4a13030b2` |
| 29 | `crates/swarm-reviewer/Cargo.toml` | toml | 261 | 11 | `349e7c383e0e4e19a7de889e7d57104dd062846efecbf72b37d3806906d976c5` |
| 30 | `crates/swarm-reviewer/src/main.rs` | rust | 13595 | 355 | `8ac7eb0f1442f5545b84514ec15b7fe8d69ddf74152be3ffdef218e1e793b92b` |
| 31 | `crates/swarm-watchdog/Cargo.toml` | toml | 227 | 10 | `214a53d97e923842faf74e5a0f30ae8ea4277b65b75d74554f22fc20a45e6fab` |
| 32 | `crates/swarm-watchdog/src/main.rs` | rust | 3207 | 88 | `545410e432cafa038d7ef1955342acd0ccf8cf18d6c13e9cd476f23d63f2d70f` |
| 33 | `crates/swarm-watchdog/tests/watchdog_lifecycle.rs` | rust | 3939 | 95 | `5ac681783521c255ebe0575906f7764ca60c47c49408ce9880f2183effddc29d` |
| 34 | `crates/swarm-worker/Cargo.toml` | toml | 1545 | 49 | `686c4886c032b82ef0eb71eeedf5472d251840e2cd426f11f9abb7cac970222b` |
| 35 | `crates/swarm-worker/examples/agent_loop_demo.rs` | rust | 4840 | 123 | `1380b6ba0651b30a89116828b27fcd1256d9cbeada1623ee6674f404a4ec937f` |
| 36 | `crates/swarm-worker/src/agent_loop.rs` | rust | 15437 | 457 | `8e52bfdb8a7ecdd427dbcdf302df536b27268412f841ee79bf448e2a6880343c` |
| 37 | `crates/swarm-worker/src/allowlist.rs` | rust | 32276 | 761 | `eeb2885c2e0d99f26e6c4ca049b15f72c401b13f6d8d55664b9f3273bb9d5acb` |
| 38 | `crates/swarm-worker/src/allowlist/allowlist_layers_tests.rs` | rust | 5257 | 117 | `9ab02ba566d12be0656c492aed703ee1c676d59c7816aaf3e4dab908606c6394` |
| 39 | `crates/swarm-worker/src/allowlist/mutants_deep_tests.rs` | rust | 3974 | 83 | `2af4db29eca2a7768ffd916a06eda2d86d07b942dbedb8887b92bbf5b441a318` |
| 40 | `crates/swarm-worker/src/artifacts.rs` | rust | 8019 | 197 | `840be123cb3d9e61f75c2d8c06540161c2cba489e1bfc82c5b51aa8167d8e0f3` |
| 41 | `crates/swarm-worker/src/audit.rs` | rust | 3178 | 83 | `7638c4d31e447c63d470342995658f23b345a10e2f8304fa82e54209e25dc844` |
| 42 | `crates/swarm-worker/src/budget.rs` | rust | 15612 | 386 | `b41f3bc4eaffa7de21b397e81ccd467450a2b275b19fc16de5b5926d4031346e` |
| 43 | `crates/swarm-worker/src/error.rs` | rust | 1492 | 47 | `af1c2bcd7d1e7e196ede8332b2f2f2c497666eec5f96a1e470fc32d2e5c6f60e` |
| 44 | `crates/swarm-worker/src/lib.rs` | rust | 1448 | 38 | `a1cced52f7986703e66e012bec021ead7b8fd4220f42e53eb7c58d6d20ace5f6` |
| 45 | `crates/swarm-worker/src/llm_cost.rs` | rust | 4965 | 106 | `860bcc8d8941b0125f80cea355e0c65c1ac1ff478b902eb73d506e1fd36544c7` |
| 46 | `crates/swarm-worker/src/main.rs` | rust | 13768 | 300 | `05ad39e35206c82e1e1033e28a709005ccfd0119c1b56661646e023cc8132d57` |
| 47 | `crates/swarm-worker/src/model_selector.rs` | rust | 17077 | 439 | `87cffd7ddd791446a37d71e523860f294f9a77818ef6fd45421d4bef2fe7d7cc` |
| 48 | `crates/swarm-worker/src/parse_step.rs` | rust | 16165 | 369 | `74f8e69be76842679fdc4f61caedda7471f1c32fb1087f784e4f9d0592315246` |
| 49 | `crates/swarm-worker/src/poll.rs` | rust | 4689 | 108 | `afa9a01909806541f9685cc03fc8b1916290896fc693c08ec51e1151d7da1b30` |
| 50 | `crates/swarm-worker/src/prompt.rs` | rust | 10918 | 223 | `545d0f976c299606ac894c951623e4949ee970c23ebcb687cc1b6604b3d2d9e0` |
| 51 | `crates/swarm-worker/src/retry.rs` | rust | 3238 | 103 | `f44b5d1792135f71da38191f257ecf54d4bf2facfa32f7ffb9d67a3d8bea35b9` |
| 52 | `crates/swarm-worker/src/sandbox.rs` | rust | 43984 | 1089 | `85239ad81c7be5642576b6fcfe2492bbad2b3162dfd372020b7642fff8ce86f0` |
| 53 | `crates/swarm-worker/src/storage.rs` | rust | 5657 | 165 | `73e82baa6fea1d489c9778a4578f251dc902f351683012d2e391b864e60be4c3` |
| 54 | `crates/swarm-worker/src/worker.rs` | rust | 31484 | 707 | `6e67d8cd785ad1c71755f274124dc4f9db27c58965c1de2be2201c88e996382c` |
| 55 | `crates/swarm-worker/tests/runtime_gate.rs` | rust | 4101 | 95 | `4f692d18ab4c0e12e09b363cb22abc72c4dcbbebeece1dd68a4e942b10d1c236` |
| 56 | `crates/swarm-worker/tests/worker_heartbeat_f5.rs` | rust | 7997 | 232 | `27fe55b56a6315c19d2c68266e5df766e7c68735bbb56812435a462784700611` |
| 57 | `crates/swarm-worker/tests/worker_integration.rs` | rust | 30268 | 791 | `7d3302b0293a59d79d12f5fda09572fadc5a11e452e321366c38264e097a4d96` |
| 58 | `crates/swarm-worker/tests/worker_sandbox.rs` | rust | 18040 | 508 | `a31490f7bd007d2a78a79dacc600f1c052f584a8f8cdd2b552eb001dacbd3c60` |

### INFRA (60 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `.dockerignore` | text | 59 | 6 | `09ba81543a788f3a573bf9461060e75cc513cc3323650524a0f70a8e12e39ee5` |
| 2 | `.env.example` | dotenv | 4145 | 71 | `772186b0dfec1ce0722f90752ddb211841a5046b6e1b5b3306d42d3f11eb06c6` |
| 3 | `.github/workflows/release-gate.yml` | yaml | 7928 | 188 | `52149273cea518a5bed1a64c8b38aa8c2d00fb88ca66c665c265e15420ec2090` |
| 4 | `.gitignore` | text | 520 | 29 | `5cee98afb214552d957676a9b0baba5cee9602cf5ce863318751df319291fed8` |
| 5 | `Dockerfile` | dockerfile | 2376 | 46 | `651c370de48a0325bb380ec08e75e71b5589ef4b3261ff5bd17add94f6663c76` |
| 6 | `LICENSE` | text | 1104 | 21 | `6a0f51d5703ca8719a56f87b7a5fb84a8c773f5e361487a1b28fe6580044dff9` |
| 7 | `docker-compose.yml` | yaml | 8016 | 189 | `5e783f8fc5f90bf23b47ace069219a22523cfb8387ddeac1720185400fbe820a` |
| 8 | `schema/migrations/001_llm_calls.sql` | sql | 3012 | 81 | `6293f4d6d774aebf761d3dc494162f4fba1b8dce7015f2161782f8478431079a` |
| 9 | `schema/migrations/002_llm_calls_grants.sql` | sql | 2014 | 33 | `8e0ef68148bf2e5297bb5ca51a473193d7cbb830b157aa94155facd1799a7a2a` |
| 10 | `schema/migrations/003_llm_calls_owner.sql` | sql | 2359 | 39 | `0d097b9907b1e1d2361a633a318234bf22e34fa6d08ba6d28a9688eea12207cc` |
| 11 | `schema/migrations/004_llm_calls_precision.sql` | sql | 3307 | 64 | `8c3d7bf94064329d067bc991929b4ac899c4df9177927c45f904a496e63b13fe` |
| 12 | `schema/migrations/005_budget_exhausted_state.sql` | sql | 5003 | 83 | `f9ba479e953f09bc9ef914a9b63ba0350237e900644c51904eabacacb9da2b12` |
| 13 | `schema/migrations/006_review_candidates.sql` | sql | 2065 | 36 | `cb4fe0ea85546ce149a3099fcf30cae1212a5a79ce40ed7bdae83d97609f8209` |
| 14 | `schema/migrations/007_llm_calls_http_status.sql` | sql | 2058 | 40 | `c9834ed46117ded9c69b408799ee0db3e3f4c2c7b4f685b0c96c53bfac249d2a` |
| 15 | `schema/migrations/008_llm_cost_reconciliation.sql` | sql | 4151 | 68 | `f41e18694998eb042b63125f19afefb75ddc3b34f3629d267e0947c484cfde6c` |
| 16 | `schema/migrations/009_llm_cost_baseline.sql` | sql | 4369 | 67 | `ca91dafa9fbeae4ddd42606a1cb9441446f185fa069248aba5314873354b28b4` |
| 17 | `schema/migrations/010_llm_model_quality.sql` | sql | 3823 | 66 | `555254ce6cc207a73ff29ba417fd9bbc8c872cf9653defc69b636c7f18f8ed86` |
| 18 | `schema/migrations/011_budget_sync_and_root_cap.sql` | sql | 16238 | 303 | `5fa446ab8d097bd00294f78299a25a7ba1dd730f567e4269376c661b51dd4d77` |
| 19 | `schema/schema_v0.sql` | sql | 39164 | 727 | `efab9dc5875037e92caa4d97f31f8fdf30537150d89bab5e0f211b1d7828470b` |
| 20 | `scripts/02-roles.sh` | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` |
| 21 | `scripts/03-role-patches.sh` | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` |
| 22 | `scripts/03-roles-2b.sh` | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` |
| 23 | `scripts/05-migrations.sh` | bash | 1059 | 22 | `3e4f8bc621181fc73284a099777cdf49cdceb3a0d6d1cd2e8f9f2714f822c359` |
| 24 | `scripts/acceptance_allowlist.txt` | text | 7161 | 156 | `ff6679e13c11035fe3541d1c964e6cbf7112824a6627aa4eefbb1006b7928b5f` |
| 25 | `scripts/acceptance_harness.sh` | bash | 37365 | 767 | `05bcb0f07de4fa34db60923ec85ab827d6cb964e23226520946562f6837967e3` |
| 26 | `scripts/build_audit_bundle.py` | python | 7014 | 183 | `ec6147bb3c3feb8a8f54313032217b276661530e552acad44690e96fabd8ad98` |
| 27 | `scripts/build_audit_index.py` | python | 5563 | 128 | `0d36cb3bdcbd645d495f5165d3b44ae12dcf9fadee76fecb9d09a00f774eeb51` |
| 28 | `scripts/build_audit_report.py` | python | 7385 | 162 | `0ce08c1494145a8d8d746b28b3cdc474ec218a14b2d3650481bb894ff4b3738f` |
| 29 | `scripts/build_audit_zip.sh` | bash | 2988 | 62 | `b0b75a1454d4ccff108e023ca761414198a1a585070937b469ac08d9241ae63a` |
| 30 | `scripts/build_project_bundle.py` | python | 9127 | 205 | `873589b0b692724faf2b419674ce28fe5584d9516f3ccd88c6b6eb1b2bf09ff8` |
| 31 | `scripts/compose_roles_arbiter.sh` | bash | 13668 | 247 | `4d16017c3e374dd57883121bcd75e481fd5366641d6ea606063330943d60fdf3` |
| 32 | `scripts/fetch_model_catalog.py` | python | 7241 | 150 | `51ef91b1e53cabf0e7162a8f9222ff8e9b28baeaf4547992149d44609221515c` |
| 33 | `scripts/model_quality_report.py` | python | 5195 | 127 | `c8865ee4221bfea9bf931730ca532a578e6d10dcfb4b8597ced0be739616bb1b` |
| 34 | `scripts/reconcile_llm_cost.py` | python | 11193 | 229 | `850801343d9e05c59be5516f9f7b78fe03b17d8ad8ddc8921e37f1dce912621c` |
| 35 | `scripts/run_mutations_sql.sh` | bash | 11836 | 224 | `bc87ed42b002310c398289090b5d28f76c64b974af879f4b868fd0016aab707a` |
| 36 | `scripts/run_mutations_v04.sh` | bash | 7217 | 160 | `fd1545b233d1935dd0b1a4f838c12713c99145b164a1a5f328d63841cfa6155d` |
| 37 | `scripts/scratch_pg.sh` | bash | 4762 | 87 | `0c6849cb6f3a84d95f90a04f078bc253a34f05a760f55c0c688bbe70818d1141` |
| 38 | `scripts/secret_scan.sh` | bash | 4625 | 101 | `b2b7fcd3728eaf97d29d62929471da94061dd664e83b0b4d89c07f5962a24c0b` |
| 39 | `scripts/start_demo.sh` | bash | 3769 | 84 | `484fc96b5df851d1d9c9f3d93925f09b6345fdc223e171b643e4c6f59500db1f` |
| 40 | `scripts/upload_release_gdrive.sh` | bash | 4125 | 89 | `e959b61e21afaf8958512e1e1e4f79c0fa092d64c09a607aee0a69b1b99af531` |
| 41 | `step2/roles.sql` | sql | 6698 | 94 | `56c551d6633521b438d7aa062eaa0e07aa65058f5a88cdf62a78ec6cef52da29` |
| 42 | `step2/roles_patch_planner_status.sql` | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` |
| 43 | `step2/roles_patch_tool_call_audit.sql` | sql | 2908 | 41 | `78f6bc1c62aa788699af3984084d84edf12d344dd6938d8157ffdcf9b65899a8` |
| 44 | `step2/test_review_gate.py` | python | 5367 | 150 | `153cbf184b1b48b4521635af9bc59472fcd6c82276020b9d8340f76bd27bc56f` |
| 45 | `step2/test_roles.py` | python | 11382 | 242 | `0b6a84cd4e2a4c74151bb0a604409669b1c02457c18def49fa84dc15330a66df` |
| 46 | `step2b/roles_2b.sql` | sql | 15188 | 268 | `7f124cfd1306b5d93f4254e423e57cc77dcfa5ba98170a13676c6fef51be8817` |
| 47 | `step2b/test_roles_2b.py` | python | 13409 | 319 | `9266b853e06c0fb6084f8b02174016f697cc67a28d3582136482645a15465645` |
| 48 | `tests/conftest.py` | python | 5222 | 119 | `48ee8a80a659e18327a82a59922ea0e6535072286f706feadc109b592ffd59f8` |
| 49 | `tests/test_contract_form_f15.py` | python | 3252 | 84 | `587691172a6e41fefb6d1a09efc02be0bf736be03ef41b2404030aada6366399` |
| 50 | `tests/test_cost_reconciliation.py` | python | 8406 | 213 | `89abf3781055cec3f878e7d43028ad157b6205eb056f9128107e79158e75f237` |
| 51 | `tests/test_f8_orphan_llm_spend.py` | python | 15492 | 327 | `1c7594135f0de2b30933eec398a3eca40b54c6d69b13f8f8c43e5f6602e79a78` |
| 52 | `tests/test_fail_unblock_f4.py` | python | 7200 | 162 | `14d7be23675f8dcdb30b1f12a27102a7a903a5b1fb564f138fcea9ec7f98d227` |
| 53 | `tests/test_find04_record_tool_call_identity.py` | python | 8165 | 212 | `57b6034d19430a560349376a01a5affe4d00f21e6f1a79d84377a49380c77180` |
| 54 | `tests/test_model_catalog.py` | python | 3869 | 106 | `8b3eb7ec23ab8656814da9c6740ad501a88d73780113fdb2620c59ca4904704c` |
| 55 | `tests/test_model_quality.py` | python | 6058 | 149 | `7ad7d911efa02764c86367123e483119530556f961c01392ded93198d766d0bd` |
| 56 | `tests/test_no_duplicate_function_bodies.py` | python | 7554 | 157 | `0cc45015a07adc30710c7f4982499bea4a7611d1eabd916a76fa2f6d9e839594` |
| 57 | `tests/test_review_gate_f2.py` | python | 6018 | 172 | `327cde66762dc446a7ae1ff9aacce3c2a693804566878d4963d5f40e8e6e25b8` |
| 58 | `tests/test_review_gate_wave4.py` | python | 5016 | 143 | `0cdfa037bc1f2e48b452e46bebee668899062410cfd29060d938271130749e65` |
| 59 | `tests/test_v0.py` | python | 21741 | 568 | `132772e6cbd2a7d93af8662628742d4927773864bc6d6a795022fc67cd23be0f` |
| 60 | `tests/test_worker_identity_f3.py` | python | 5272 | 142 | `e804f7a843d907c4863d74db9d87bb422d5bf9cd69eee5ad63e06cdc9393ed4a` |

### DOCS_CORE (29 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 23174 | 350 | `2cba789fdf904c633e7f65b720956586b6f9990fe7661eb09ede07f761fa7119` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 11869 | 267 | `db4a54df6a9eff81a9d60a602bd72aa959c50c9700a5ec360be61d56998ff2ad` |
| 4 | `REPORT_docker_gap_closed.md` | markdown | 10675 | 143 | `8f6ba2f8366312e844ae755c939e28873ce941a66a78a2a74bdb05ab5de10760` |
| 5 | `REPORT_v0.2_step1.md` | markdown | 9901 | 163 | `75d5b685e7b18f45d606d7a1e66dbe3943d1aa774990915574ac4f406dd0376a` |
| 6 | `REPORT_v0.2_step2_native_verify.md` | markdown | 5669 | 112 | `cc098a99af0b02a2c6144d7d8b44da9c69238b6e3924834ccbee1dd0c90036bf` |
| 7 | `REPORT_v0.2_step2_planner.md` | markdown | 11777 | 182 | `f474c16005df9fb6eaff932b8244582e8ac90f7665452c5f67f35a62c0c7417c` |
| 8 | `REPORT_v0.2_step2b.md` | markdown | 11521 | 188 | `17c6c01f82a457d90320259db4a6489f00f61ac326a46ec531d955c7f5a7e7c5` |
| 9 | `REPORT_v0.2_step2b_v2.md` | markdown | 8962 | 167 | `ae86e556ea31c76098a2773f19de2640133a057f78be4d30dd174b2b28c76a8f` |
| 10 | `REPORT_v0.3.1_planner_description.md` | markdown | 12208 | 207 | `8f6f8bf743261a865833aadac8bb7070e8aab86b2c675397e6c785dadb4f7182` |
| 11 | `REPORT_v0.3_llm_worker.md` | markdown | 12201 | 204 | `6a3fef555bdb9ac33e934151f4805b52c2e7f102d74f3975462271c2cf45888b` |
| 12 | `REPORT_v0.4.1.md` | markdown | 5948 | 147 | `fcb4640010a5851ad3b9a464ee241a8486910bbf366dd1bbcf564b326bdbe7b4` |
| 13 | `REPORT_v0.4_repairs.md` | markdown | 9918 | 168 | `36092e95e649a1cf72e07c5198b0e43e8ce0880c6f0952c9848c2052bd5e13aa` |
| 14 | `REPORT_v0.5_load_test.md` | markdown | 6549 | 54 | `25f04d076b3889356552ade608248fd60d32d3a46639eb53ac6baeed6eed0ac3` |
| 15 | `RESUME.md` | markdown | 16782 | 355 | `097dac9b7b545570b2929b92fd047f3b98cc9187073ab06d5d4f04b04d273359` |
| 16 | `docs/ARCHITECTURE.md` | markdown | 12226 | 381 | `4c800238c5ce8398b72f182c8fbb6ca81310d30b3461886d55421ca0b63caa31` |
| 17 | `docs/KNOWN_LIMITATIONS.md` | markdown | 27566 | 391 | `61c8979df5b0e1c3203a536a35505dd3b74c5f3380a61b2fdf4e65303ba65029` |
| 18 | `docs/METHODOLOGY.md` | markdown | 17263 | 271 | `8828f0bc85bc961b2a1aed1157e93423ba2ce14f20e7b23dfd23eafd6d1d6357` |
| 19 | `docs/PROJECT_STATE.md` | markdown | 5600 | 89 | `b5075619dc0228415cd75910cd447770212ea93ba44d7417f235797fd44dc663` |
| 20 | `docs/TZ_v0.2_step1_fixes.md` | markdown | 13920 | 398 | `994f7dae8362bde94e86ffc8df3ce2668b17c6de5138fbcd4b0bc042302a721a` |
| 21 | `docs/TZ_v0.2_step2_planner.md` | markdown | 12895 | 325 | `793f2523b85ce94448d0a96c149c35a916c7180dc6e24ffa58f3642e4155aa91` |
| 22 | `docs/TZ_v0.2_step2b_worker_identity.md` | markdown | 9783 | 200 | `74cdd5f0d13baa450787e8838092579399fd95e52ed833aea5152d8b00e72b95` |
| 23 | `docs/TZ_v0.3.1_planner_description.md` | markdown | 8264 | 230 | `0ed36aebcb2a83a397e13d3432e874d9d0931439205b358b4026ea5b53f7420a` |
| 24 | `docs/TZ_v0.3_llm_worker.md` | markdown | 11499 | 290 | `b0e6cfe44bf81b7277ff6c6933705d40d265c058ee46c98132cd54cf09a01627` |
| 25 | `docs/TZ_v0.4_shell_tools_sandbox.md` | markdown | 11169 | 287 | `67eaa042042ecee383dc9e0ac8a6f9b4ccb131935f7123ef27ec565a0d7b036b` |
| 26 | `docs/VERIFICATION_LOG.md` | markdown | 8512 | 180 | `4479463c38933b94b3fd4a50d407f98ba8a52bda3b9b38620fb87b075cd32103` |
| 27 | `docs/audit/AUDITOR_BRIEF_v0.4.en.md` | markdown | 7364 | 169 | `319186b702ec4a26b6d3442b21cf1dda1cfacf43c91f1a40ba13e4fcd76e245e` |
| 28 | `docs/audit/AUDITOR_BRIEF_v0.4.md` | markdown | 10541 | 164 | `a50fedc370bbdebcdc991c5bd0219ff1063acdc41e409107eac53a1c623e4a8a` |
| 29 | `docs/audit/README_AUDIT_PACKAGE.md` | markdown | 4993 | 96 | `e822b5efd3e55a16029545708ee847232ebe7b415380a29b22b0d11042f2127d` |

### DOCS (54 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 23174 | 350 | `2cba789fdf904c633e7f65b720956586b6f9990fe7661eb09ede07f761fa7119` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 11869 | 267 | `db4a54df6a9eff81a9d60a602bd72aa959c50c9700a5ec360be61d56998ff2ad` |
| 4 | `REPORT_docker_gap_closed.md` | markdown | 10675 | 143 | `8f6ba2f8366312e844ae755c939e28873ce941a66a78a2a74bdb05ab5de10760` |
| 5 | `REPORT_v0.2_step1.md` | markdown | 9901 | 163 | `75d5b685e7b18f45d606d7a1e66dbe3943d1aa774990915574ac4f406dd0376a` |
| 6 | `REPORT_v0.2_step2_native_verify.md` | markdown | 5669 | 112 | `cc098a99af0b02a2c6144d7d8b44da9c69238b6e3924834ccbee1dd0c90036bf` |
| 7 | `REPORT_v0.2_step2_planner.md` | markdown | 11777 | 182 | `f474c16005df9fb6eaff932b8244582e8ac90f7665452c5f67f35a62c0c7417c` |
| 8 | `REPORT_v0.2_step2b.md` | markdown | 11521 | 188 | `17c6c01f82a457d90320259db4a6489f00f61ac326a46ec531d955c7f5a7e7c5` |
| 9 | `REPORT_v0.2_step2b_v2.md` | markdown | 8962 | 167 | `ae86e556ea31c76098a2773f19de2640133a057f78be4d30dd174b2b28c76a8f` |
| 10 | `REPORT_v0.3.1_planner_description.md` | markdown | 12208 | 207 | `8f6f8bf743261a865833aadac8bb7070e8aab86b2c675397e6c785dadb4f7182` |
| 11 | `REPORT_v0.3_llm_worker.md` | markdown | 12201 | 204 | `6a3fef555bdb9ac33e934151f4805b52c2e7f102d74f3975462271c2cf45888b` |
| 12 | `REPORT_v0.4.1.md` | markdown | 5948 | 147 | `fcb4640010a5851ad3b9a464ee241a8486910bbf366dd1bbcf564b326bdbe7b4` |
| 13 | `REPORT_v0.4_repairs.md` | markdown | 9918 | 168 | `36092e95e649a1cf72e07c5198b0e43e8ce0880c6f0952c9848c2052bd5e13aa` |
| 14 | `REPORT_v0.5_load_test.md` | markdown | 6549 | 54 | `25f04d076b3889356552ade608248fd60d32d3a46639eb53ac6baeed6eed0ac3` |
| 15 | `RESUME.md` | markdown | 16782 | 355 | `097dac9b7b545570b2929b92fd047f3b98cc9187073ab06d5d4f04b04d273359` |
| 16 | `docs/AGENTS.md` | markdown | 6056 | 85 | `c465c5a4d759f9181502e32e81a03ad79ef358055bb28ad1f765243ee9c4f6fc` |
| 17 | `docs/ARCHITECTURE.md` | markdown | 12226 | 381 | `4c800238c5ce8398b72f182c8fbb6ca81310d30b3461886d55421ca0b63caa31` |
| 18 | `docs/DISTRIBUTED_WORKER_MVP.md` | markdown | 9203 | 126 | `7e046a39fa49669ed7f20e59878b681825bec5d3361bca56b3978ccb0467006d` |
| 19 | `docs/KNOWN_LIMITATIONS.md` | markdown | 27566 | 391 | `61c8979df5b0e1c3203a536a35505dd3b74c5f3380a61b2fdf4e65303ba65029` |
| 20 | `docs/METHODOLOGY.md` | markdown | 17263 | 271 | `8828f0bc85bc961b2a1aed1157e93423ba2ce14f20e7b23dfd23eafd6d1d6357` |
| 21 | `docs/PROJECT_STATE.md` | markdown | 5600 | 89 | `b5075619dc0228415cd75910cd447770212ea93ba44d7417f235797fd44dc663` |
| 22 | `docs/TZ_v0.2_step1_fixes.md` | markdown | 13920 | 398 | `994f7dae8362bde94e86ffc8df3ce2668b17c6de5138fbcd4b0bc042302a721a` |
| 23 | `docs/TZ_v0.2_step2_planner.md` | markdown | 12895 | 325 | `793f2523b85ce94448d0a96c149c35a916c7180dc6e24ffa58f3642e4155aa91` |
| 24 | `docs/TZ_v0.2_step2b_worker_identity.md` | markdown | 9783 | 200 | `74cdd5f0d13baa450787e8838092579399fd95e52ed833aea5152d8b00e72b95` |
| 25 | `docs/TZ_v0.3.1_planner_description.md` | markdown | 8264 | 230 | `0ed36aebcb2a83a397e13d3432e874d9d0931439205b358b4026ea5b53f7420a` |
| 26 | `docs/TZ_v0.3_llm_worker.md` | markdown | 11499 | 290 | `b0e6cfe44bf81b7277ff6c6933705d40d265c058ee46c98132cd54cf09a01627` |
| 27 | `docs/TZ_v0.4_shell_tools_sandbox.md` | markdown | 11169 | 287 | `67eaa042042ecee383dc9e0ac8a6f9b4ccb131935f7123ef27ec565a0d7b036b` |
| 28 | `docs/VERIFICATION_LOG.md` | markdown | 8512 | 180 | `4479463c38933b94b3fd4a50d407f98ba8a52bda3b9b38620fb87b075cd32103` |
| 29 | `docs/ci/CI_COST.md` | markdown | 7641 | 118 | `34fec006cb96c1ea8dd79664b9d73d20e01fc019cbf31eff837f5e4ab4995ff5` |
| 30 | `docs/history/ANONYMIZED_ARCHITECTURE_DISCUSSION_v0.6.md` | markdown | 4387 | 37 | `b39f1a731c711a7d05fbefe36f57ac8a2aa05a0f8da086356a88b6fd30d0842f` |
| 31 | `docs/history/AUDITED_DOC_v15.md` | markdown | 135883 | 684 | `cc702322793060d986f9420e1178951d2c5ced8c9232637cbfc1a0e316c1d6ed` |
| 32 | `docs/history/README.md` | markdown | 4795 | 109 | `e1690bc6c81a77243e2e1c522362630d56b99fb474ced4d8b1030eba4c977e2e` |
| 33 | `docs/history/TZ_v0.2_step1_fixes_v1.md` | markdown | 16123 | 386 | `e38ac008e8ac952f911aa3360c1064ac84fba0f6a3316168e0d7a5e9768de309` |
| 34 | `docs/history/TZ_v0.2_step2_planner_v1.md` | markdown | 9648 | 257 | `f23f444e5e3c276601aa48d8b0a9d3ae3ebd8ba4cb5d7b53688cd1dc4d8ca90b` |
| 35 | `docs/history/obsolete/README.md` | markdown | 1270 | 11 | `dc1c7ca8df710f8fa244b568458461a13ab3656680cccff4c123b0fceb96c696` |
| 36 | `docs/optional/ARCHITECTURE_DECISIONS.md` | markdown | 16395 | 265 | `d5791d18ee7da97b4181ee70d0c6d77654cede38137345251f52fa6ae85ad98e` |
| 37 | `docs/optional/AUDIT_HISTORY.md` | markdown | 22591 | 396 | `5d8ce99ea426b8bad568b368c38c08a468463fb67bc33c43924632286d89dd78` |
| 38 | `docs/optional/BRIEF_FOR_AGENTS.md` | markdown | 10812 | 227 | `8ce3070c8141cab0377068d68caa41871a1aba6a86e3b561ed92ffd6c1f7e3a7` |
| 39 | `docs/optional/CLAUDE_DIALOGUE.md` | markdown | 11641 | 190 | `f337b70f3578ceb7ff4991b7570c967294beb172e62dca1e03897dd76df09dce` |
| 40 | `docs/optional/DOC_STRATEGY.md` | markdown | 10857 | 235 | `ca4b24d93476b01f6545ddfee373def01dd2cf2fb589e963bf35c7aa9c7fa22a` |
| 41 | `docs/optional/ERRORS.md` | markdown | 9837 | 214 | `eb7c8892ee7a0086e1f02ef44e008e51ce9a7f3f9484f13d9e6bce8b4690d7f1` |
| 42 | `docs/optional/NATIVE_SETUP.md` | markdown | 10822 | 331 | `a89679682d4fc39afe7ec75e10cac3d2336a5ccb7d4ca63437cd48adc6277ec9` |
| 43 | `docs/optional/NEXT_STEPS.md` | markdown | 8042 | 158 | `7b44843489ab61b314fb29e54dbd07a0bc69fde1076cedc441c78fd4adde0a3f` |
| 44 | `docs/optional/PLATFORM_COMPARISON.md` | markdown | 13790 | 200 | `27d89af9aedf6e2b3249da72b6d25f0d0de3c9ac6b44dd04e01cf021a8ea941a` |
| 45 | `docs/optional/REPORT_TEMPLATE.md` | markdown | 7475 | 202 | `cfb6796f9a34322ae2d4a2202562bb4e6a5fcbac1e9f6628dfbed13514e6603b` |
| 46 | `docs/optional/ROADMAP.md` | markdown | 10374 | 247 | `ef98362758829f76c526614b179ef2c4b6325923f0c85e5834eb759a6abaaffa` |
| 47 | `docs/optional/RUST_FINDINGS.md` | markdown | 3668 | 30 | `538039fed69765820f6e9c9e508afbeb34eddf0f3f3a227001b49deafce2ddaa` |
| 48 | `docs/optional/THREAT_MODEL.md` | markdown | 19399 | 297 | `088a4e79b375a3b9e09997bcd784b95f1b6521bc11b81671c802ff5b003c33db` |
| 49 | `docs/optional/TZ_v0.2_step2_roles.md` | markdown | 10459 | 93 | `b22b3428747c656e2673d8b33691e0527b0fe285eee885d7b7e55b2a40cf5b1f` |
| 50 | `docs/research/POSITIONING_2026.md` | markdown | 13046 | 100 | `514e13e19756c1983cc038a27b0fc26a1b84f48a69fecab9c7c91ef29ff88651` |
| 51 | `docs/research/agent-orchestrator.md` | markdown | 11642 | 158 | `541d25b3c5365928c184d293ed7c9fd776696a487d6fb24dfcaaebfa20ed3488` |
| 52 | `docs/research/global-market.md` | markdown | 10087 | 137 | `21ac814d9ba862099cbf8902a9e04ddbfc0199cc06216d9446a48cc2f11a1acc` |
| 53 | `docs/research/tee-applicability.md` | markdown | 9931 | 114 | `36b151e98a65f02a45084af69ff462803b855c0ad074f7866cccb205d43a5f28` |
| 54 | `docs/research/zkml-optimistic-verification.md` | markdown | 22405 | 241 | `9b5bf469524ccf1004c1eefd32a7413e63ba7e3473c042fa0b879470c3356bfe` |

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
