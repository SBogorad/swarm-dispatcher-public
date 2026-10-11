# Аудит-пакет проекта `swarm-dispatcher` — индекс

**Сгенерировано:** 2026-10-11T10:18:46+07:00
**Генератор:** `scripts/build_project_bundle.py` + `scripts/build_audit_index.py`
**Схема:** `swarm-dispatcher.audit-bundle.v2`

Предназначено для передачи **внешнему аудитору (LLM)**: полный исходный код, инфраструктура и документация в машиночитаемых XML-томах. Каждый файл — с полным текстом и двумя хешами (`sha256`, `md5`). Секреты исключены.

## Состав пакета

| Том | Содержание | Файлов | Строк | Исходники, байт | XML, байт |
|-----|------------|-------:|------:|----------------:|----------:|
| `CODE` | Код проекта (Rust + тесты + манифесты) | 46 | 12328 | 398650 | 420702 |
| `INFRA` | Инфраструктура (SQL, init-скрипты, контейнеры, конфиг) | 33 | 4887 | 208985 | 224936 |
| `DOCS_CORE` | Ключевая документация (ТЗ, архитектура, ограничения, отчёты) | 29 | 5972 | 295106 | 309211 |
| `DOCS` | Документация и отчёты (без писем history/inbound) | 49 | 10430 | 638684 | 661877 |
| **Итого** | | **157** | | **1541425** | **1616726** |

## Хеши томов (для передачи аудитору)

### CODE — `PROJECT_CODE_BUNDLE_v0.4.xml`

- Размер: 420702 байт
- `sha256`: `3c3584ce3b05c65281fb0cd54186796202286fcc0328137ad159e003c1248266`
- `md5`: `ae04eb513f99c2519402137ba6edaa53`

### INFRA — `PROJECT_INFRA_BUNDLE_v0.4.xml`

- Размер: 224936 байт
- `sha256`: `20635cae7c1bf9850f9e14769084ef928f4653f84b0d842f3efe9b6e08d9b29e`
- `md5`: `4b7483c836e2cad771005c649935f2de`

### DOCS_CORE — `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml`

- Размер: 309211 байт
- `sha256`: `d399c4f9ab1bd243dd65f838b8de92c19516e8aa83a7cc5ef2f648b9bf2c5d35`
- `md5`: `9b063e345f897eddafbf7e3767eb0727`

### DOCS — `PROJECT_DOCS_BUNDLE_v0.4.xml`

- Размер: 661877 байт
- `sha256`: `2761e794c25863efa1cfe23086eed908442c150c5f056e0f5f00c43d7e67d5fe`
- `md5`: `cdac769992422388bcc3a54aed11f60a`

## Исключено сознательно

- `.env`, `.env.bak.*` — **секреты** (пароли ролей, ключи LLM).
- `target/`, `.venv/`, `__pycache__/`, `.pytest_cache/` — сборка/кэш.
- `.docx`, `.png` и прочие бинарники.
- `docs/history/inbound/*` — переписка (не код; при необходимости — отдельно).

Вместо `.env` включён `.env.example` (структура без значений).

## Манифест по томам

### CODE (46 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `Cargo.lock` | toml | 88617 | 3617 | `522f721dee695dd2da851f9cb11bfcbed40a409e1554247a8fededbdb5b1f625` |
| 2 | `Cargo.toml` | toml | 867 | 36 | `b43634a10e13f2fd8317c4a0b2ed09c1a7ad87c55e22ee2542c9f233891be894` |
| 3 | `crates/swarm-cli/Cargo.toml` | toml | 225 | 10 | `43cacce0e3960bb63925656b3ac96ccece391d295e041a7242844b134eb13f46` |
| 4 | `crates/swarm-cli/src/main.rs` | rust | 6676 | 205 | `7202eb0ae1412fa3686c1067e7281b1d1c3a06de5504b35e17825e2b3c608e2b` |
| 5 | `crates/swarm-core/Cargo.toml` | toml | 345 | 17 | `aa491b9d7ab721880f0f86efaa5c02875e9d3ab1a1dba37e474f2c70d81c1f40` |
| 6 | `crates/swarm-core/src/dispatcher.rs` | rust | 13120 | 386 | `5cba1ed744924bfac25bfd44f07193f4dd69d69c59adc904d2db7f68dee5722c` |
| 7 | `crates/swarm-core/src/errors.rs` | rust | 6948 | 189 | `b8d8415d10ceb9010a171303247c749c8eaa3d8521611509d084830bb082b6ba` |
| 8 | `crates/swarm-core/src/lib.rs` | rust | 235 | 7 | `b9210371c22830287ec2681249d835ddc7e793d67a8078ddc5c228586ec277b0` |
| 9 | `crates/swarm-core/src/types.rs` | rust | 9084 | 272 | `d87f95640ed83607efed6ec875a75b30c407ef81a4b2e0782e3f49783e7ce4f2` |
| 10 | `crates/swarm-core/tests/alt/integration_alt.rs` | rust | 6587 | 200 | `23cc2ad52cc9ac2ad01d1f0d3c0a56d26101f1004acb98fb7236bdd4c9159988` |
| 11 | `crates/swarm-core/tests/integration.rs` | rust | 12783 | 364 | `85b571dc55a300c63e6ba55c548ac5b80863f94f92e755354fa7f2076a40026c` |
| 12 | `crates/swarm-core/tests/integration_verified.rs` | rust | 10177 | 319 | `792cc3ffff5abe4c97debe5da2c09f28539c5c6cc4e8fa2e8e809e84e785c4e9` |
| 13 | `crates/swarm-llm/Cargo.toml` | toml | 367 | 11 | `f3da8011038d37fc95332a7158f11c0ba44141d51e85842acd62d682276ab86b` |
| 14 | `crates/swarm-llm/src/error.rs` | rust | 1147 | 43 | `83ed168ea6bab9202c19d61f06106e2878fecd62b007a0b179cbdbfa8010445e` |
| 15 | `crates/swarm-llm/src/lib.rs` | rust | 13638 | 433 | `4da23c279bef521e3eacb8a0a88768e669bb8b4c84e9c28255b50b345ffd86d3` |
| 16 | `crates/swarm-planner/Cargo.toml` | toml | 557 | 24 | `abe2418a29d3ea9ec317ad9f5f82cbe947d6cc1ad708a25a04f1220f65171bdc` |
| 17 | `crates/swarm-planner/src/error.rs` | rust | 516 | 23 | `cec97ee8df913e6a866ea76742a0fb127aace1f46b2e3f16c0e5470b8e5f37f8` |
| 18 | `crates/swarm-planner/src/lib.rs` | rust | 14199 | 367 | `a2f1c473181d11125493e6e75118cf38efe5f21ca6e68ed39b32f3ed7354608d` |
| 19 | `crates/swarm-planner/src/llm.rs` | rust | 4128 | 114 | `9f5998ff3d569b1122f1fc748f013976e26d91623a2774f35f97e895c61b7c67` |
| 20 | `crates/swarm-planner/src/main.rs` | rust | 2440 | 78 | `4ce3896b0f96c48037e560f7b05172e51be10a6e6a5e3bdd9d4d07d9c06802cf` |
| 21 | `crates/swarm-planner/src/parse.rs` | rust | 11546 | 369 | `71738083b4146e3c76d5e7fd71a3ede5cd597c074bbc6e5461b49bb2a76f3b8c` |
| 22 | `crates/swarm-planner/src/prompt.rs` | rust | 2660 | 78 | `592075089361aa794fd86e9dc32377ebd0cb2bec1e15930d7bddcb155797a709` |
| 23 | `crates/swarm-planner/tests/fixtures/planner_dup_dep.json` | json | 745 | 31 | `a2853ef4c1f243da155cba4436f9e7b6fc733094331903ec3df7252497cc2eb1` |
| 24 | `crates/swarm-planner/tests/fixtures/planner_ok.json` | json | 935 | 31 | `2044fcfa6c1dc51ffb12bc6b959f4609e88b22f62964eae8a94351d52ca88d09` |
| 25 | `crates/swarm-planner/tests/fixtures/planner_too_many.json` | json | 340 | 18 | `6660c81d7e32c0b292251d83d8c673218a4c5ceae865b285574b615d33b0f22c` |
| 26 | `crates/swarm-planner/tests/planner_integration.rs` | rust | 10679 | 282 | `7785e61bc15435a238f321b94b4c54328f47ca752850be5975629a6efc89cb3e` |
| 27 | `crates/swarm-watchdog/Cargo.toml` | toml | 227 | 10 | `214a53d97e923842faf74e5a0f30ae8ea4277b65b75d74554f22fc20a45e6fab` |
| 28 | `crates/swarm-watchdog/src/main.rs` | rust | 3107 | 84 | `e439de2468b655e310a0b0d03f218883f986a7607722593f8176cb93e59dfb3f` |
| 29 | `crates/swarm-worker/Cargo.toml` | toml | 1745 | 51 | `85194c3fc2f6c2fb79e15644a94a91090915626d4aa3e2a6388b1c2f40f4bb7b` |
| 30 | `crates/swarm-worker/examples/agent_loop_demo.rs` | rust | 4840 | 123 | `1380b6ba0651b30a89116828b27fcd1256d9cbeada1623ee6674f404a4ec937f` |
| 31 | `crates/swarm-worker/src/agent_loop.rs` | rust | 15103 | 453 | `d2d4b89cfcedbab6f54cb699335c20928a41c20f51afa64eab3b03a8d061cc03` |
| 32 | `crates/swarm-worker/src/allowlist.rs` | rust | 17248 | 436 | `48e389dff202d89b66e9e971380beac81b6f47f853f90ff10512b12e2877c54a` |
| 33 | `crates/swarm-worker/src/artifacts.rs` | rust | 8019 | 197 | `840be123cb3d9e61f75c2d8c06540161c2cba489e1bfc82c5b51aa8167d8e0f3` |
| 34 | `crates/swarm-worker/src/audit.rs` | rust | 3178 | 83 | `7638c4d31e447c63d470342995658f23b345a10e2f8304fa82e54209e25dc844` |
| 35 | `crates/swarm-worker/src/error.rs` | rust | 1677 | 52 | `a23482e6278533a592bb3e50d26465d57643950c46da288d54256dba00e2a4d9` |
| 36 | `crates/swarm-worker/src/lib.rs` | rust | 1421 | 38 | `93ab850ac5fbe1d232f5e0f0ad152f906941f82e6ddfc06cecb58446cb7c66e4` |
| 37 | `crates/swarm-worker/src/main.rs` | rust | 5074 | 136 | `9d66ae5e85d6ea071810734d5c9fcfae14c07663f2013ace2a414e195ba16d8b` |
| 38 | `crates/swarm-worker/src/parse_step.rs` | rust | 16165 | 369 | `74f8e69be76842679fdc4f61caedda7471f1c32fb1087f784e4f9d0592315246` |
| 39 | `crates/swarm-worker/src/prompt.rs` | rust | 9171 | 205 | `54bd53c464c3beff9636d2d88369ba875f0227f547a18a0b1948ff473b29f771` |
| 40 | `crates/swarm-worker/src/retry.rs` | rust | 3238 | 103 | `f44b5d1792135f71da38191f257ecf54d4bf2facfa32f7ffb9d67a3d8bea35b9` |
| 41 | `crates/swarm-worker/src/sandbox.rs` | rust | 23714 | 654 | `5c1a28560183f0c3d676e50976416840de8dad7c43477a9630a2b9e183b2ee74` |
| 42 | `crates/swarm-worker/src/storage.rs` | rust | 5657 | 165 | `73e82baa6fea1d489c9778a4578f251dc902f351683012d2e391b864e60be4c3` |
| 43 | `crates/swarm-worker/src/worker.rs` | rust | 16438 | 413 | `469550a1de4edcd823afeead391784af10e3706cad75118373d3a4cff1f2ce32` |
| 44 | `crates/swarm-worker/tests/worker_heartbeat_f5.rs` | rust | 7825 | 227 | `b40473024e57bb17b7c895d2feeb5e1e99331e7dae06bd0f3377a39b69fc84f7` |
| 45 | `crates/swarm-worker/tests/worker_integration.rs` | rust | 19089 | 529 | `3edf9942efbcff47dd4999e073feb4f85357195deea991b73173f15bd3c8dce8` |
| 46 | `crates/swarm-worker/tests/worker_sandbox.rs` | rust | 16153 | 476 | `f0f20c05d21c0cb4fb723ef18b30e9800ef1ae57e6d73bfa17617794f6a52a95` |

### INFRA (33 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `.dockerignore` | text | 59 | 6 | `09ba81543a788f3a573bf9461060e75cc513cc3323650524a0f70a8e12e39ee5` |
| 2 | `.env.example` | dotenv | 1175 | 26 | `2fec247f468cf38696499cb9e683f245a990f0f9b4e82264e9fea6eec354c416` |
| 3 | `.github/workflows/release-gate.yml` | yaml | 2474 | 85 | `c5c24dd697087b43ad7ab8dea49d86605e8ad1c2c72df1ad178f105bfe4f2639` |
| 4 | `.gitignore` | text | 295 | 24 | `52297dbac7da5b13e52a501d80b1c50fccddf95375ad4d639a5a944b53510462` |
| 5 | `Dockerfile` | dockerfile | 2303 | 45 | `52ab907ac0ec615b9daa2de24ac838d9c96beaf27b7508ee618953c9cebc1c3e` |
| 6 | `LICENSE` | text | 1104 | 21 | `6a0f51d5703ca8719a56f87b7a5fb84a8c773f5e361487a1b28fe6580044dff9` |
| 7 | `docker-compose.yml` | yaml | 6880 | 163 | `5d06db44f4dfe8ab7d5a30116a1c6d2f141d436602f5ee89ddb78267a4d9294d` |
| 8 | `schema/schema_v0.sql` | sql | 27083 | 533 | `0349a8e214da802e6f4ecff26e0686865ea9b93a518ac49d232ec4ba3797a087` |
| 9 | `scripts/02-roles.sh` | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` |
| 10 | `scripts/03-role-patches.sh` | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` |
| 11 | `scripts/03-roles-2b.sh` | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` |
| 12 | `scripts/acceptance_allowlist.txt` | text | 1457 | 50 | `807dfb14944fe98e17b9e87b9e8cd77970f5163d48f1f661d18064c8e9ab894e` |
| 13 | `scripts/acceptance_harness.sh` | bash | 20677 | 490 | `f4b26a49b2c7284e2f0f028ed12f7bcd32480e5cfdb6f80b0fa0e3e5423e449d` |
| 14 | `scripts/build_audit_bundle.py` | python | 7014 | 183 | `ec6147bb3c3feb8a8f54313032217b276661530e552acad44690e96fabd8ad98` |
| 15 | `scripts/build_audit_index.py` | python | 5563 | 128 | `0d36cb3bdcbd645d495f5165d3b44ae12dcf9fadee76fecb9d09a00f774eeb51` |
| 16 | `scripts/build_audit_report.py` | python | 7385 | 162 | `0ce08c1494145a8d8d746b28b3cdc474ec218a14b2d3650481bb894ff4b3738f` |
| 17 | `scripts/build_project_bundle.py` | python | 8632 | 200 | `967cb9ab7656d0048692ef87ea76f28e082ab1d403a375c695869ce33a180906` |
| 18 | `scripts/run_mutations_v04.sh` | bash | 6630 | 155 | `b8cf1a047decb3c26d021f93ce61f0735cfd9245672f7e48be42c6d53120fc5a` |
| 19 | `step2/roles.sql` | sql | 5678 | 84 | `aedd115b03b5c5dcb754bb5a8025c6b063c7f7484066234aea9f11ef5048d01f` |
| 20 | `step2/roles_patch_planner_status.sql` | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` |
| 21 | `step2/roles_patch_tool_call_audit.sql` | sql | 2908 | 41 | `78f6bc1c62aa788699af3984084d84edf12d344dd6938d8157ffdcf9b65899a8` |
| 22 | `step2/test_review_gate.py` | python | 5367 | 150 | `153cbf184b1b48b4521635af9bc59472fcd6c82276020b9d8340f76bd27bc56f` |
| 23 | `step2/test_roles.py` | python | 11297 | 241 | `3dcd56ff42d4c400f7d5267358525f9ab36cbbeee5ca8c71d1bb40c6a22d544d` |
| 24 | `step2b/roles_2b.sql` | sql | 13448 | 242 | `37216bd7987df7be1a2875ffd21a709ed6f2fe5a532cc3e83ec919c9cd2e7082` |
| 25 | `step2b/test_roles_2b.py` | python | 13409 | 319 | `9266b853e06c0fb6084f8b02174016f697cc67a28d3582136482645a15465645` |
| 26 | `tests/conftest.py` | python | 957 | 23 | `7ec53a21af859ddee9f2d6b0fe790f5c067f6b264f011acd0a704da1518a6a76` |
| 27 | `tests/test_contract_form_f15.py` | python | 3252 | 84 | `587691172a6e41fefb6d1a09efc02be0bf736be03ef41b2404030aada6366399` |
| 28 | `tests/test_fail_unblock_f4.py` | python | 7200 | 162 | `14d7be23675f8dcdb30b1f12a27102a7a903a5b1fb564f138fcea9ec7f98d227` |
| 29 | `tests/test_find04_record_tool_call_identity.py` | python | 8165 | 212 | `57b6034d19430a560349376a01a5affe4d00f21e6f1a79d84377a49380c77180` |
| 30 | `tests/test_review_gate_f2.py` | python | 6018 | 172 | `327cde66762dc446a7ae1ff9aacce3c2a693804566878d4963d5f40e8e6e25b8` |
| 31 | `tests/test_review_gate_wave4.py` | python | 5016 | 143 | `0cdfa037bc1f2e48b452e46bebee668899062410cfd29060d938271130749e65` |
| 32 | `tests/test_v0.py` | python | 18567 | 519 | `2547bdf9305b7d1481ea39c0ac41491b377df51d447b0f72ad5cf6d08c380f09` |
| 33 | `tests/test_worker_identity_f3.py` | python | 5272 | 142 | `e804f7a843d907c4863d74db9d87bb422d5bf9cd69eee5ad63e06cdc9393ed4a` |

### DOCS_CORE (29 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 8511 | 162 | `dbd85bcc16c20143a273745c3b4e9b9a7f99074a240bf92104c022863a53f3dd` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 8652 | 230 | `d6720eb91f036660fef87152baa4ec133daf041f4e0b0ba56c05cd45762a5af5` |
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
| 17 | `docs/KNOWN_LIMITATIONS.md` | markdown | 18817 | 305 | `ad63b03b956216115b1f0a9a53ada04d256eae9bc889f94b5057da24b6a2a425` |
| 18 | `docs/METHODOLOGY.md` | markdown | 14539 | 242 | `d0fdfa915879e43aff622687afd730c0b6306cfd422c92ddab0f4455404a4dcf` |
| 19 | `docs/PROJECT_STATE.md` | markdown | 5027 | 84 | `760f5a56059e54cde0d16921435f04d9f4f7f4376147a8ab4aa038afe5df7f97` |
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

### DOCS (49 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 8511 | 162 | `dbd85bcc16c20143a273745c3b4e9b9a7f99074a240bf92104c022863a53f3dd` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 8652 | 230 | `d6720eb91f036660fef87152baa4ec133daf041f4e0b0ba56c05cd45762a5af5` |
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
| 17 | `docs/DISTRIBUTED_WORKER_MVP.md` | markdown | 9203 | 126 | `7e046a39fa49669ed7f20e59878b681825bec5d3361bca56b3978ccb0467006d` |
| 18 | `docs/KNOWN_LIMITATIONS.md` | markdown | 18817 | 305 | `ad63b03b956216115b1f0a9a53ada04d256eae9bc889f94b5057da24b6a2a425` |
| 19 | `docs/METHODOLOGY.md` | markdown | 14539 | 242 | `d0fdfa915879e43aff622687afd730c0b6306cfd422c92ddab0f4455404a4dcf` |
| 20 | `docs/PROJECT_STATE.md` | markdown | 5027 | 84 | `760f5a56059e54cde0d16921435f04d9f4f7f4376147a8ab4aa038afe5df7f97` |
| 21 | `docs/TZ_v0.2_step1_fixes.md` | markdown | 13920 | 398 | `994f7dae8362bde94e86ffc8df3ce2668b17c6de5138fbcd4b0bc042302a721a` |
| 22 | `docs/TZ_v0.2_step2_planner.md` | markdown | 12895 | 325 | `793f2523b85ce94448d0a96c149c35a916c7180dc6e24ffa58f3642e4155aa91` |
| 23 | `docs/TZ_v0.2_step2b_worker_identity.md` | markdown | 9783 | 200 | `74cdd5f0d13baa450787e8838092579399fd95e52ed833aea5152d8b00e72b95` |
| 24 | `docs/TZ_v0.3.1_planner_description.md` | markdown | 8264 | 230 | `0ed36aebcb2a83a397e13d3432e874d9d0931439205b358b4026ea5b53f7420a` |
| 25 | `docs/TZ_v0.3_llm_worker.md` | markdown | 11499 | 290 | `b0e6cfe44bf81b7277ff6c6933705d40d265c058ee46c98132cd54cf09a01627` |
| 26 | `docs/TZ_v0.4_shell_tools_sandbox.md` | markdown | 11169 | 287 | `67eaa042042ecee383dc9e0ac8a6f9b4ccb131935f7123ef27ec565a0d7b036b` |
| 27 | `docs/VERIFICATION_LOG.md` | markdown | 8512 | 180 | `4479463c38933b94b3fd4a50d407f98ba8a52bda3b9b38620fb87b075cd32103` |
| 28 | `docs/ci/CI_COST.md` | markdown | 5959 | 92 | `5d6948721985661df829776f1de3550e7e86c909b38b07cec0c4e1b376861ea9` |
| 29 | `docs/history/ANONYMIZED_ARCHITECTURE_DISCUSSION_v0.6.md` | markdown | 4387 | 37 | `b39f1a731c711a7d05fbefe36f57ac8a2aa05a0f8da086356a88b6fd30d0842f` |
| 30 | `docs/history/AUDITED_DOC_v15.md` | markdown | 135883 | 684 | `cc702322793060d986f9420e1178951d2c5ced8c9232637cbfc1a0e316c1d6ed` |
| 31 | `docs/history/README.md` | markdown | 4795 | 109 | `e1690bc6c81a77243e2e1c522362630d56b99fb474ced4d8b1030eba4c977e2e` |
| 32 | `docs/history/TZ_v0.2_step1_fixes_v1.md` | markdown | 16123 | 386 | `e38ac008e8ac952f911aa3360c1064ac84fba0f6a3316168e0d7a5e9768de309` |
| 33 | `docs/history/TZ_v0.2_step2_planner_v1.md` | markdown | 9648 | 257 | `f23f444e5e3c276601aa48d8b0a9d3ae3ebd8ba4cb5d7b53688cd1dc4d8ca90b` |
| 34 | `docs/history/obsolete/README.md` | markdown | 1270 | 11 | `dc1c7ca8df710f8fa244b568458461a13ab3656680cccff4c123b0fceb96c696` |
| 35 | `docs/optional/ARCHITECTURE_DECISIONS.md` | markdown | 16395 | 265 | `d5791d18ee7da97b4181ee70d0c6d77654cede38137345251f52fa6ae85ad98e` |
| 36 | `docs/optional/AUDIT_HISTORY.md` | markdown | 22591 | 396 | `5d8ce99ea426b8bad568b368c38c08a468463fb67bc33c43924632286d89dd78` |
| 37 | `docs/optional/BRIEF_FOR_AGENTS.md` | markdown | 10812 | 227 | `8ce3070c8141cab0377068d68caa41871a1aba6a86e3b561ed92ffd6c1f7e3a7` |
| 38 | `docs/optional/CLAUDE_DIALOGUE.md` | markdown | 11641 | 190 | `f337b70f3578ceb7ff4991b7570c967294beb172e62dca1e03897dd76df09dce` |
| 39 | `docs/optional/DOC_STRATEGY.md` | markdown | 10857 | 235 | `ca4b24d93476b01f6545ddfee373def01dd2cf2fb589e963bf35c7aa9c7fa22a` |
| 40 | `docs/optional/ERRORS.md` | markdown | 9837 | 214 | `eb7c8892ee7a0086e1f02ef44e008e51ce9a7f3f9484f13d9e6bce8b4690d7f1` |
| 41 | `docs/optional/NATIVE_SETUP.md` | markdown | 10822 | 331 | `a89679682d4fc39afe7ec75e10cac3d2336a5ccb7d4ca63437cd48adc6277ec9` |
| 42 | `docs/optional/NEXT_STEPS.md` | markdown | 8042 | 158 | `7b44843489ab61b314fb29e54dbd07a0bc69fde1076cedc441c78fd4adde0a3f` |
| 43 | `docs/optional/PLATFORM_COMPARISON.md` | markdown | 13790 | 200 | `27d89af9aedf6e2b3249da72b6d25f0d0de3c9ac6b44dd04e01cf021a8ea941a` |
| 44 | `docs/optional/REPORT_TEMPLATE.md` | markdown | 7475 | 202 | `cfb6796f9a34322ae2d4a2202562bb4e6a5fcbac1e9f6628dfbed13514e6603b` |
| 45 | `docs/optional/ROADMAP.md` | markdown | 10374 | 247 | `ef98362758829f76c526614b179ef2c4b6325923f0c85e5834eb759a6abaaffa` |
| 46 | `docs/optional/RUST_FINDINGS.md` | markdown | 3668 | 30 | `538039fed69765820f6e9c9e508afbeb34eddf0f3f3a227001b49deafce2ddaa` |
| 47 | `docs/optional/THREAT_MODEL.md` | markdown | 19399 | 297 | `088a4e79b375a3b9e09997bcd784b95f1b6521bc11b81671c802ff5b003c33db` |
| 48 | `docs/optional/TZ_v0.2_step2_roles.md` | markdown | 10459 | 93 | `b22b3428747c656e2673d8b33691e0527b0fe285eee885d7b7e55b2a40cf5b1f` |
| 49 | `docs/research/POSITIONING_2026.md` | markdown | 13046 | 100 | `514e13e19756c1983cc038a27b0fc26a1b84f48a69fecab9c7c91ef29ff88651` |

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
