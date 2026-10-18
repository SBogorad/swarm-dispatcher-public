//! Issue #7/#10: выбор LLM-модели по цене **успешного** результата.
//!
//! Проблема: модель задавалась жёстко (`WORKER_LLM_MODEL`) — по названию, а не
//! по измеренной стоимости доведённой до `done` задачи. Дешёвая модель, которая
//! валится в `max_rework`, дороже дорогой, которая сдаёт с первого раза.
//!
//! Что здесь есть:
//! - [`ModelCatalog`] — кандидаты с ценами (источник: `GET /api/pricing`
//!   relay.fast; формула `input$/1M = model_ratio * group_ratio * 2`,
//!   `output$/1M = input * completion_ratio`, сверена с `/api/log/token`).
//! - [`ModelStat`] — **измерения** по модели: сколько задач, доля успеха,
//!   цена успешного результата (`scripts/model_quality_report.py` считает их
//!   из view `llm_model_quality`).
//! - [`select_model`] — чистая функция выбора: сначала по измерениям
//!   (если набрана статистика), иначе по прайсу. Без БД и без сети — поэтому
//!   полностью покрыта юнит-тестами.
//!
//! Поведение по умолчанию не меняется: без `SWARM_MODEL_AUTOSELECT=1`
//! селектор не подключён, модель берётся из конфига, как раньше.

use serde::Deserialize;

/// Кандидат каталога моделей.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCandidate {
    pub name: String,
    /// USD за 1M входных токенов.
    pub input_usd_per_1m: f64,
    /// USD за 1M выходных токенов.
    pub output_usd_per_1m: f64,
    /// Уровень качества модели (1..=3). Задачу уровня `T` можно отдать только
    /// модели с `quality_tier >= T`: слабая модель на сложной задаче жжёт
    /// ретраи и дороже мощной.
    pub quality_tier: u8,
    /// Доступна ли модель сейчас (есть в `GET /v1/models`).
    pub enabled: bool,
}

impl ModelCandidate {
    /// Оценка цены для ранжирования по прайсу.
    ///
    /// Токены асимметричны: на боевых вызовах вход ≈ 220, выход ≈ 400
    /// (`/api/log/token`), т.е. выход дороже по объёму. Берём
    /// `input + output * expected_output_ratio`, чтобы дешёвая по входу, но
    /// дорогая по выходу модель не выигрывала обманчиво.
    fn list_price_score(&self, expected_output_ratio: f64) -> f64 {
        self.input_usd_per_1m + self.output_usd_per_1m * expected_output_ratio
    }
}

/// Измерения по модели (цена именно успешного результата).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelStat {
    pub model: String,
    /// Сколько задач с этой моделью доведено до терминального состояния.
    pub tasks: u64,
    /// Доля задач, дошедших до `done` (0.0..=1.0).
    pub success_rate: f64,
    /// Стоимость успешного результата, USD (`total_cost / tasks_done`).
    pub cost_per_success_usd: f64,
}

/// Почему выбрана эта модель.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionBasis {
    /// По измеренной цене успешного результата.
    Measured,
    /// По прайсу каталога (измерений не хватило).
    ListPrice,
}

/// Результат выбора.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub model: String,
    pub basis: SelectionBasis,
    pub reason: String,
}

/// Каталог моделей (парсится из JSON — новых зависимостей не тянем).
#[derive(Debug, Clone)]
pub struct ModelCatalog {
    pub models: Vec<ModelCandidate>,
}

#[derive(Deserialize)]
struct CatalogFile {
    models: Vec<CatalogEntry>,
}

#[derive(Deserialize)]
struct CatalogEntry {
    name: String,
    input_usd_per_1m: f64,
    output_usd_per_1m: f64,
    #[serde(default = "default_quality_tier")]
    quality_tier: u8,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_quality_tier() -> u8 {
    1
}
fn default_true() -> bool {
    true
}

impl ModelCatalog {
    /// Разбор JSON-каталога. Ошибка — если пусто или у модели невалидная цена.
    pub fn from_json_str(s: &str) -> Result<Self, String> {
        let parsed: CatalogFile =
            serde_json::from_str(s).map_err(|e| format!("model catalog JSON: {e}"))?;
        if parsed.models.is_empty() {
            return Err("model catalog is empty".into());
        }
        let mut models = Vec::with_capacity(parsed.models.len());
        for e in parsed.models {
            if e.name.trim().is_empty() {
                return Err("model catalog: empty model name".into());
            }
            if !e.input_usd_per_1m.is_finite()
                || !e.output_usd_per_1m.is_finite()
                || e.input_usd_per_1m < 0.0
                || e.output_usd_per_1m < 0.0
            {
                return Err(format!("model catalog: bad price for {}", e.name));
            }
            if e.quality_tier == 0 {
                return Err(format!("model catalog: quality_tier 0 for {}", e.name));
            }
            models.push(ModelCandidate {
                name: e.name,
                input_usd_per_1m: e.input_usd_per_1m,
                output_usd_per_1m: e.output_usd_per_1m,
                quality_tier: e.quality_tier,
                enabled: e.enabled,
            });
        }
        Ok(Self { models })
    }

    /// Каталог из одного текущего значения — дефолт, не меняющий поведение.
    pub fn single(model: &str) -> Self {
        Self {
            models: vec![ModelCandidate {
                name: model.to_string(),
                input_usd_per_1m: 0.0,
                output_usd_per_1m: 0.0,
                quality_tier: 1,
                enabled: true,
            }],
        }
    }
}

/// Выбор модели.
///
/// `task_tier` — требуемый уровень задачи (из контракта, по умолчанию 1).
/// Возвращает `None`, если подходящих моделей нет (тогда вызывающий берёт
/// модель из конфига — деградация без падения).
///
/// Порядок:
/// 1. фильтр `enabled && quality_tier >= task_tier`;
/// 2. среди них — модели с набранной статистикой (`tasks >= min_samples`) и
///    приемлемым успехом (`success_rate >= min_success_rate`); берём минимум
///    `cost_per_success_usd` — это и есть «цена успешного результата»;
/// 3. если таких нет — минимум по прайсу (`list_price_score`).
pub fn select_model(
    catalog: &ModelCatalog,
    task_tier: u8,
    stats: &[ModelStat],
    min_samples: u64,
    min_success_rate: f64,
    expected_output_ratio: f64,
) -> Option<Selection> {
    let eligible: Vec<&ModelCandidate> = catalog
        .models
        .iter()
        .filter(|m| m.enabled && m.quality_tier >= task_tier)
        .collect();
    if eligible.is_empty() {
        return None;
    }

    // 2. По измерениям.
    let mut best_measured: Option<(&ModelCandidate, &ModelStat)> = None;
    for m in &eligible {
        if let Some(s) = stats.iter().find(|s| s.model == m.name) {
            if s.tasks >= min_samples && s.success_rate >= min_success_rate {
                let better = match best_measured {
                    None => true,
                    Some((_, cur)) => s.cost_per_success_usd < cur.cost_per_success_usd,
                };
                if better {
                    best_measured = Some((m, s));
                }
            }
        }
    }
    if let Some((m, s)) = best_measured {
        return Some(Selection {
            model: m.name.clone(),
            basis: SelectionBasis::Measured,
            reason: format!(
                "измерено: cost_per_success=${:.6} success={:.0}% tasks={}",
                s.cost_per_success_usd,
                s.success_rate * 100.0,
                s.tasks
            ),
        });
    }

    // 3. По прайсу.
    let best = eligible.into_iter().min_by(|a, b| {
        a.list_price_score(expected_output_ratio)
            .partial_cmp(&b.list_price_score(expected_output_ratio))
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    Some(Selection {
        model: best.name.clone(),
        basis: SelectionBasis::ListPrice,
        reason: format!(
            "прайс: ${:.4}/1M in + ${:.4}/1M out x{:.1}",
            best.input_usd_per_1m, best.output_usd_per_1m, expected_output_ratio
        ),
    })
}

/// Готовая конфигурация выбора модели для воркера.
#[derive(Debug, Clone)]
pub struct ModelSelector {
    pub catalog: ModelCatalog,
    pub stats: Vec<ModelStat>,
    pub min_samples: u64,
    pub min_success_rate: f64,
    pub expected_output_ratio: f64,
    /// Уровень задачи по умолчанию, если в контракте его нет.
    pub default_task_tier: u8,
}

impl ModelSelector {
    /// Уровень задачи из контракта (`quality_tier`), иначе дефолт.
    pub fn task_tier(&self, contract: &serde_json::Value) -> u8 {
        contract
            .get("quality_tier")
            .and_then(|v| v.as_u64())
            .map(|v| v.clamp(1, 255) as u8)
            .unwrap_or(self.default_task_tier)
    }

    /// Выбрать модель под контракт. `None` → брать модель из конфига.
    pub fn choose(&self, contract: &serde_json::Value) -> Option<Selection> {
        self.choose_with_rework(contract, 0)
    }

    /// Выбрать модель под контракт с учётом номера реворка.
    ///
    /// При `rework > 0` эффективный уровень задачи поднимается на номер
    /// попытки с потолком 3: модель, на которой задача уже провалилась,
    /// повторно вслепую не берётся. `reason` показывает подъём `tier=T->E`.
    pub fn choose_with_rework(
        &self,
        contract: &serde_json::Value,
        rework: i32,
    ) -> Option<Selection> {
        let task_tier = self.task_tier(contract);
        let effective: u8 = if rework > 0 {
            (task_tier + rework.min(3) as u8).min(3)
        } else {
            task_tier
        };
        let mut sel = select_model(
            &self.catalog,
            effective,
            &self.stats,
            self.min_samples,
            self.min_success_rate,
            self.expected_output_ratio,
        )?;
        if rework > 0 {
            sel.reason = format!(
                "rework={} tier={}->{} {}",
                rework, task_tier, effective, sel.reason
            );
        }
        Some(sel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> ModelCatalog {
        ModelCatalog::from_json_str(
            r#"{"models":[
                {"name":"cheap","input_usd_per_1m":0.0075,"output_usd_per_1m":0.0235,"quality_tier":1},
                {"name":"mid","input_usd_per_1m":0.015,"output_usd_per_1m":0.060,"quality_tier":2},
                {"name":"strong","input_usd_per_1m":0.066,"output_usd_per_1m":0.198,"quality_tier":3},
                {"name":"off","input_usd_per_1m":0.001,"output_usd_per_1m":0.001,"quality_tier":1,"enabled":false}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_catalog() {
        let c = cat();
        assert_eq!(c.models.len(), 4);
        assert_eq!(c.models[0].name, "cheap");
        assert!(!c.models[3].enabled);
    }

    #[test]
    fn rejects_empty_and_bad_price() {
        assert!(ModelCatalog::from_json_str(r#"{"models":[]}"#).is_err());
        assert!(ModelCatalog::from_json_str(
            r#"{"models":[{"name":"x","input_usd_per_1m":-1,"output_usd_per_1m":1}]}"#
        )
        .is_err());
        assert!(ModelCatalog::from_json_str(
            r#"{"models":[{"name":"","input_usd_per_1m":1,"output_usd_per_1m":1}]}"#
        )
        .is_err());
        assert!(ModelCatalog::from_json_str("not json").is_err());
    }

    /// Без измерений — самый дешёвый подходящий по прайсу.
    #[test]
    fn falls_back_to_list_price() {
        let sel = select_model(&cat(), 1, &[], 3, 0.5, 2.0).unwrap();
        assert_eq!(sel.model, "cheap");
        assert_eq!(sel.basis, SelectionBasis::ListPrice);
    }

    /// Сложная задача не отдаётся слабой модели.
    #[test]
    fn respects_task_tier() {
        let sel = select_model(&cat(), 3, &[], 3, 0.5, 2.0).unwrap();
        assert_eq!(sel.model, "strong", "tier=3 доступен только strong");
    }

    /// Дешёвая модель, которая валится, не выигрывает у мощной, которая сдаёт.
    #[test]
    fn measured_beats_list_price() {
        let stats = vec![
            ModelStat {
                model: "cheap".into(),
                tasks: 10,
                success_rate: 0.2,
                cost_per_success_usd: 0.050,
            },
            ModelStat {
                model: "mid".into(),
                tasks: 10,
                success_rate: 0.9,
                cost_per_success_usd: 0.008,
            },
        ];
        let sel = select_model(&cat(), 2, &stats, 3, 0.5, 2.0).unwrap();
        assert_eq!(sel.model, "mid");
        assert_eq!(sel.basis, SelectionBasis::Measured);
    }

    /// Мало данных — не доверяем измерению, падаем на прайс.
    #[test]
    fn insufficient_samples_use_list_price() {
        let stats = vec![ModelStat {
            model: "mid".into(),
            tasks: 1,
            success_rate: 1.0,
            cost_per_success_usd: 0.001,
        }];
        let sel = select_model(&cat(), 2, &stats, 3, 0.5, 2.0).unwrap();
        assert_eq!(sel.model, "mid");
        assert_eq!(sel.basis, SelectionBasis::ListPrice);
    }

    /// Низкий успех — модель не берём, даже если дёшева по успеху.
    #[test]
    fn low_success_rate_is_excluded() {
        let stats = vec![
            ModelStat {
                model: "mid".into(),
                tasks: 20,
                success_rate: 0.1,
                cost_per_success_usd: 0.001,
            },
            ModelStat {
                model: "strong".into(),
                tasks: 20,
                success_rate: 0.8,
                cost_per_success_usd: 0.02,
            },
        ];
        let sel = select_model(&cat(), 2, &stats, 3, 0.5, 2.0).unwrap();
        assert_eq!(sel.model, "strong");
        assert_eq!(sel.basis, SelectionBasis::Measured);
    }

    /// Отключённые модели не участвуют.
    #[test]
    fn disabled_models_excluded() {
        let c = ModelCatalog::from_json_str(
            r#"{"models":[
                {"name":"a","input_usd_per_1m":0.001,"output_usd_per_1m":0.001,"enabled":false},
                {"name":"b","input_usd_per_1m":0.5,"output_usd_per_1m":0.5}
            ]}"#,
        )
        .unwrap();
        let sel = select_model(&c, 1, &[], 3, 0.5, 2.0).unwrap();
        assert_eq!(sel.model, "b");
    }

    /// Нет подходящих — None (вызывающий берёт модель из конфига).
    #[test]
    fn no_eligible_returns_none() {
        let sel = select_model(&cat(), 3, &[], 3, 0.5, 2.0);
        assert!(sel.is_some());
        let c = ModelCatalog::from_json_str(
            r#"{"models":[{"name":"weak","input_usd_per_1m":0.01,"output_usd_per_1m":0.01,"quality_tier":1}]}"#,
        )
        .unwrap();
        assert!(select_model(&c, 3, &[], 3, 0.5, 2.0).is_none());
    }

    /// Учёт асимметрии токенов: дорогая по выходу модель не выигрывает.
    #[test]
    fn output_price_matters() {
        let c = ModelCatalog::from_json_str(
            r#"{"models":[
                {"name":"in-cheap-out-rich","input_usd_per_1m":0.001,"output_usd_per_1m":0.5},
                {"name":"balanced","input_usd_per_1m":0.02,"output_usd_per_1m":0.02}
            ]}"#,
        )
        .unwrap();
        let sel = select_model(&c, 1, &[], 3, 0.5, 2.0).unwrap();
        assert_eq!(
            sel.model, "balanced",
            "0.001+0.5*2=1.001 > 0.02+0.02*2=0.06"
        );
    }

    #[test]
    fn task_tier_from_contract() {
        let s = ModelSelector {
            catalog: cat(),
            stats: vec![],
            min_samples: 3,
            min_success_rate: 0.5,
            expected_output_ratio: 2.0,
            default_task_tier: 1,
        };
        assert_eq!(s.task_tier(&serde_json::json!({"quality_tier": 3})), 3);
        assert_eq!(s.task_tier(&serde_json::json!({})), 1);
        assert_eq!(
            s.task_tier(&serde_json::json!({"quality_tier": 0})),
            1,
            "клампится к >=1"
        );
        let sel = s.choose(&serde_json::json!({"quality_tier": 3})).unwrap();
        assert_eq!(sel.model, "strong");
    }
}
