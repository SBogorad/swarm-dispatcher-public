# Аудит-пакет проекта `swarm-dispatcher` — индекс

**Сгенерировано:** 2026-10-10T20:53:49+07:00
**Генератор:** `scripts/build_project_bundle.py` + `scripts/build_audit_index.py`
**Схема:** `swarm-dispatcher.audit-bundle.v2`

Предназначено для передачи **внешнему аудитору (LLM)**: полный исходный код, инфраструктура и документация в машиночитаемых XML-томах. Каждый файл — с полным текстом и двумя хешами (`sha256`, `md5`). Секреты исключены.

## Состав пакета

| Том | Содержание | Файлов | Строк | Исходники, байт | XML, байт |
|-----|------------|-------:|------:|----------------:|----------:|
| `CODE` | Код проекта (Rust + тесты + манифесты) | 45 | 11824 | 377199 | 398756 |
| `INFRA` | Инфраструктура (SQL, init-скрипты, контейнеры, конфиг) | 26 | 3787 | 163016 | 175765 |
| `DOCS_CORE` | Ключевая документация (ТЗ, архитектура, ограничения, отчёты) | 29 | 5920 | 290098 | 304203 |
| `DOCS` | Документация и отчёты (без писем history/inbound) | 48 | 10278 | 620630 | 643360 |
| **Итого** | | **148** | | **1450943** | **1522084** |

## Хеши томов (для передачи аудитору)

### CODE — `PROJECT_CODE_BUNDLE_v0.4.xml`

- Размер: 398756 байт
- `sha256`: `e95fe68289df62e2be5108c5372478907451b17adece7f674e34f6ddddc4b3b0`
- `md5`: `73b1b5baa06b1c1c08790d34f311db4d`

### INFRA — `PROJECT_INFRA_BUNDLE_v0.4.xml`

- Размер: 175765 байт
- `sha256`: `8a9952e107370638d5f8314f832ba3a0cb68ed282a4b8ff789cb226563009023`
- `md5`: `454485519cbca41b2c2c221a992ec2b9`

### DOCS_CORE — `PROJECT_DOCS_CORE_BUNDLE_v0.4.xml`

- Размер: 304203 байт
- `sha256`: `409f6e30f8ad82e9eae61e221a47c82ea72ed04e6e84565786799ec5adea8264`
- `md5`: `cb75d7781fbc65bbdfe255dee44312a2`

### DOCS — `PROJECT_DOCS_BUNDLE_v0.4.xml`

- Размер: 643360 байт
- `sha256`: `fd25fa574c4f335dcf6009983d77cee3912ce51eea20d389c259310cf5147f5a`
- `md5`: `05caed02b932f61c09330c2b8669c531`

## Исключено сознательно

- `.env`, `.env.bak.*` — **секреты** (пароли ролей, ключи LLM).
- `target/`, `.venv/`, `__pycache__/`, `.pytest_cache/` — сборка/кэш.
- `.docx`, `.png` и прочие бинарники.
- `docs/history/inbound/*` — переписка (не код; при необходимости — отдельно).

Вместо `.env` включён `.env.example` (структура без значений).

## Манифест по томам

### CODE (45 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `Cargo.lock` | toml | 88617 | 3617 | `522f721dee695dd2da851f9cb11bfcbed40a409e1554247a8fededbdb5b1f625` |
| 2 | `Cargo.toml` | toml | 865 | 36 | `7a93607700ff3a699874078a136f1a99a90338d20846de2c62e6c35201bb730b` |
| 3 | `crates/swarm-cli/Cargo.toml` | toml | 225 | 10 | `43cacce0e3960bb63925656b3ac96ccece391d295e041a7242844b134eb13f46` |
| 4 | `crates/swarm-cli/src/main.rs` | rust | 6676 | 205 | `7202eb0ae1412fa3686c1067e7281b1d1c3a06de5504b35e17825e2b3c608e2b` |
| 5 | `crates/swarm-core/Cargo.toml` | toml | 345 | 17 | `aa491b9d7ab721880f0f86efaa5c02875e9d3ab1a1dba37e474f2c70d81c1f40` |
| 6 | `crates/swarm-core/src/dispatcher.rs` | rust | 12262 | 361 | `634723de103c29672bd411f9a8e6f975b60e5c1e4aab7156f7357fad59f40640` |
| 7 | `crates/swarm-core/src/errors.rs` | rust | 6948 | 189 | `b8d8415d10ceb9010a171303247c749c8eaa3d8521611509d084830bb082b6ba` |
| 8 | `crates/swarm-core/src/lib.rs` | rust | 235 | 7 | `b9210371c22830287ec2681249d835ddc7e793d67a8078ddc5c228586ec277b0` |
| 9 | `crates/swarm-core/src/types.rs` | rust | 9084 | 272 | `d87f95640ed83607efed6ec875a75b30c407ef81a4b2e0782e3f49783e7ce4f2` |
| 10 | `crates/swarm-core/tests/alt/integration_alt.rs` | rust | 6247 | 190 | `f7be51e6a0b02cf633cee01de39606e252572800e8ff01013e1021c51fff9ee8` |
| 11 | `crates/swarm-core/tests/integration.rs` | rust | 12002 | 349 | `1330be7a320c614c59946060fff44fe8e70f8c531b27c8a70daa3205d84e2746` |
| 12 | `crates/swarm-core/tests/integration_verified.rs` | rust | 9759 | 315 | `ce5ee3b8e69e8fdcdeadbd817277f5a00fbf3d2d813f0731b03a5a65ae384fb7` |
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
| 26 | `crates/swarm-planner/tests/planner_integration.rs` | rust | 10664 | 282 | `5f5504700d08f50c1c51f8f1d8ee44798099e477ba6acf65acefae97fe6fa520` |
| 27 | `crates/swarm-watchdog/Cargo.toml` | toml | 227 | 10 | `214a53d97e923842faf74e5a0f30ae8ea4277b65b75d74554f22fc20a45e6fab` |
| 28 | `crates/swarm-watchdog/src/main.rs` | rust | 3107 | 84 | `e439de2468b655e310a0b0d03f218883f986a7607722593f8176cb93e59dfb3f` |
| 29 | `crates/swarm-worker/Cargo.toml` | toml | 902 | 32 | `7eac49b458f5f22091ace3f9c04b4e747e3a1ab3e9079f04fe6b89986b6659ef` |
| 30 | `crates/swarm-worker/examples/agent_loop_demo.rs` | rust | 4840 | 123 | `1380b6ba0651b30a89116828b27fcd1256d9cbeada1623ee6674f404a4ec937f` |
| 31 | `crates/swarm-worker/src/agent_loop.rs` | rust | 15103 | 453 | `d2d4b89cfcedbab6f54cb699335c20928a41c20f51afa64eab3b03a8d061cc03` |
| 32 | `crates/swarm-worker/src/allowlist.rs` | rust | 17248 | 436 | `48e389dff202d89b66e9e971380beac81b6f47f853f90ff10512b12e2877c54a` |
| 33 | `crates/swarm-worker/src/artifacts.rs` | rust | 8019 | 197 | `840be123cb3d9e61f75c2d8c06540161c2cba489e1bfc82c5b51aa8167d8e0f3` |
| 34 | `crates/swarm-worker/src/audit.rs` | rust | 3178 | 83 | `7638c4d31e447c63d470342995658f23b345a10e2f8304fa82e54209e25dc844` |
| 35 | `crates/swarm-worker/src/error.rs` | rust | 1458 | 47 | `db458aea5bef333dab5b3dc00eebabc30cbac22c3c613745759f517369e1cf10` |
| 36 | `crates/swarm-worker/src/lib.rs` | rust | 853 | 26 | `988dfa0549b14748192649ea40cbbcffb2f9e0dc17a47471535720b6c2b903ba` |
| 37 | `crates/swarm-worker/src/main.rs` | rust | 5074 | 136 | `9d66ae5e85d6ea071810734d5c9fcfae14c07663f2013ace2a414e195ba16d8b` |
| 38 | `crates/swarm-worker/src/parse_step.rs` | rust | 16165 | 369 | `74f8e69be76842679fdc4f61caedda7471f1c32fb1087f784e4f9d0592315246` |
| 39 | `crates/swarm-worker/src/prompt.rs` | rust | 9035 | 201 | `59d9dd3c8a227d15948f5023e0c269929405a37f23476e32079900c14ecb86d1` |
| 40 | `crates/swarm-worker/src/retry.rs` | rust | 3238 | 103 | `f44b5d1792135f71da38191f257ecf54d4bf2facfa32f7ffb9d67a3d8bea35b9` |
| 41 | `crates/swarm-worker/src/sandbox.rs` | rust | 20825 | 592 | `b1e1db54631d8de91a60da8e83771227cd704926f8ffd5bfda59f73112e640c0` |
| 42 | `crates/swarm-worker/src/storage.rs` | rust | 5657 | 165 | `73e82baa6fea1d489c9778a4578f251dc902f351683012d2e391b864e60be4c3` |
| 43 | `crates/swarm-worker/src/worker.rs` | rust | 10016 | 293 | `bf456e6b45963c86a19288415f4a45e92498c66e434b2f03cb065b2bef67303d` |
| 44 | `crates/swarm-worker/tests/worker_integration.rs` | rust | 18954 | 528 | `4f96891baa72a63fd7bbf6741bb4f629032ee171a5be429489912ddbdefca20e` |
| 45 | `crates/swarm-worker/tests/worker_sandbox.rs` | rust | 16153 | 476 | `f0f20c05d21c0cb4fb723ef18b30e9800ef1ae57e6d73bfa17617794f6a52a95` |

### INFRA (26 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `.dockerignore` | text | 59 | 6 | `09ba81543a788f3a573bf9461060e75cc513cc3323650524a0f70a8e12e39ee5` |
| 2 | `.env.example` | dotenv | 1175 | 26 | `2fec247f468cf38696499cb9e683f245a990f0f9b4e82264e9fea6eec354c416` |
| 3 | `.gitignore` | text | 295 | 24 | `52297dbac7da5b13e52a501d80b1c50fccddf95375ad4d639a5a944b53510462` |
| 4 | `Dockerfile` | dockerfile | 1477 | 35 | `add45b22cb1a278bdce041848d4ff43fe12e30f6e5f6b0d63f1df8968e71ce0c` |
| 5 | `LICENSE` | text | 1104 | 21 | `6a0f51d5703ca8719a56f87b7a5fb84a8c773f5e361487a1b28fe6580044dff9` |
| 6 | `docker-compose.yml` | yaml | 5478 | 141 | `73f6f467afdffbcd96c67023827b101aff2355b3dfccc45c9338f5e9b59f5ce5` |
| 7 | `schema/schema_v0.sql` | sql | 23179 | 479 | `4f9ea6398fbd7f9671a3b3d3fd280252c12615304ba9cd4247e3f64b2bcd7482` |
| 8 | `scripts/02-roles.sh` | bash | 831 | 20 | `cfecd3af0e550d0c7c426053920e68592b0081b75528714567f7dd65df533b64` |
| 9 | `scripts/03-role-patches.sh` | bash | 1169 | 26 | `e5374dad91f01310de5634f717c32e50fa60a6398ffda4b5cc8457777ad0976d` |
| 10 | `scripts/03-roles-2b.sh` | bash | 934 | 24 | `2e4ce315bfc988738c29568641d3ada6d64d746f2a4dbd8506f994951742a609` |
| 11 | `scripts/acceptance_allowlist.txt` | text | 1317 | 42 | `0dd86cc2075343760d394c69f799ea73e1d47385d066ff17fdf5aa2747de6c54` |
| 12 | `scripts/acceptance_harness.sh` | bash | 18792 | 460 | `0cde74173effa77444aac398435f1ff8f1ef235ca77a65a990c2a7b311c09d7b` |
| 13 | `scripts/build_audit_bundle.py` | python | 6990 | 183 | `b9afef90f0b8112a19901b137e9b5ec440d0b8547d31bd6dba43c8d16b757054` |
| 14 | `scripts/build_audit_index.py` | python | 5563 | 128 | `0d36cb3bdcbd645d495f5165d3b44ae12dcf9fadee76fecb9d09a00f774eeb51` |
| 15 | `scripts/build_audit_report.py` | python | 7385 | 162 | `0ce08c1494145a8d8d746b28b3cdc474ec218a14b2d3650481bb894ff4b3738f` |
| 16 | `scripts/build_project_bundle.py` | python | 8591 | 199 | `dc91a369fe6f784622cdee2ac49629e4e210c229acb7639497af0e0c43ce7d75` |
| 17 | `scripts/run_mutations_v04.sh` | bash | 6630 | 155 | `b8cf1a047decb3c26d021f93ce61f0735cfd9245672f7e48be42c6d53120fc5a` |
| 18 | `step2/roles.sql` | sql | 5275 | 80 | `bfdabc1b44d7fe136eb1e551fb631c45fc8c65e5f29ffd9eba4080efa2a312a6` |
| 19 | `step2/roles_patch_planner_status.sql` | sql | 766 | 12 | `59d10d8e029f4030fd00117444f092bfbb4d485430d0ee7c4dca5cdf6abab63f` |
| 20 | `step2/roles_patch_submit_review.sql` | sql | 4046 | 87 | `66d2483c1931da18098ea83f2b3335f8f0392ec1790221bf5f25a85b8ae75a54` |
| 21 | `step2/roles_patch_tool_call_audit.sql` | sql | 2630 | 38 | `c75ef22471ea33a9731eaae95b73424b0f0926437324f8f3a3bda5811fabbf9a` |
| 22 | `step2/test_review_gate.py` | python | 5002 | 144 | `1a6d04694b7e65ec8c069add60fce3e3a1be109b8e4d2bce7e77bd55c444b6bb` |
| 23 | `step2/test_roles.py` | python | 11297 | 241 | `3dcd56ff42d4c400f7d5267358525f9ab36cbbeee5ca8c71d1bb40c6a22d544d` |
| 24 | `step2b/roles_2b.sql` | sql | 12338 | 232 | `177fb6183080898d52159e1fb506b0334ad2e0dd3e12477ae739687a23567afd` |
| 25 | `step2b/test_roles_2b.py` | python | 12156 | 303 | `1d4887b4544cde1fbd7547791a120e752c8181e3c3413102f0d938cf479f8bc7` |
| 26 | `tests/test_v0.py` | python | 18537 | 519 | `88a3ffdf56dc62f967ce516a7aa5bc650dbf7cd2eafbe67de093c4abd94ffa95` |

### DOCS_CORE (29 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 8511 | 162 | `dbd85bcc16c20143a273745c3b4e9b9a7f99074a240bf92104c022863a53f3dd` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 8581 | 229 | `a7213f0200526dda80ba2855c4338483f8a67e0d82d1c1138f7ac537d4e3977b` |
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
| 17 | `docs/KNOWN_LIMITATIONS.md` | markdown | 13880 | 254 | `9a9bad5f7389f70663892837b0f6402f9d05a304fee027660219fd193bba51d3` |
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

### DOCS (48 файлов)

| # | Путь | Язык | Байт | Строк | sha256 |
|--:|------|------|-----:|------:|--------|
| 1 | `CHANGELOG.md` | markdown | 8511 | 162 | `dbd85bcc16c20143a273745c3b4e9b9a7f99074a240bf92104c022863a53f3dd` |
| 2 | `MANIFEST.md` | markdown | 6283 | 139 | `6ccc2c3293c26aed8831fd4f917358e9cc5268db80cbb4f2b8dfb12daf070798` |
| 3 | `README.md` | markdown | 8581 | 229 | `a7213f0200526dda80ba2855c4338483f8a67e0d82d1c1138f7ac537d4e3977b` |
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
| 18 | `docs/KNOWN_LIMITATIONS.md` | markdown | 13880 | 254 | `9a9bad5f7389f70663892837b0f6402f9d05a304fee027660219fd193bba51d3` |
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
