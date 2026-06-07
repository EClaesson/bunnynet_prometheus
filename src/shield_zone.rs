use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::gauge;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, ShieldCategoryMetrics, ShieldMetrics, ShieldZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, emit_labeled_counter,
    find_chart_value_for_date_lenient, sum_chart_values_in_range,
};

pub type ShieldZoneStatsState = EntityStatsState<ShieldZoneKind>;

const WAF: &str = "waf";
const DDOS: &str = "ddos";
const RATE_LIMIT: &str = "rate_limit";
const ACCESS_LISTS: &str = "access_lists";
const BOT_DETECTION: &str = "bot_detection";
const UPLOAD_SCANNING: &str = "upload_scanning";
const API_GUARDIAN: &str = "api_guardian";

pub struct ShieldZoneKind;

impl EntityType for ShieldZoneKind {
    type Entity = ShieldZone;
    type DayData = ShieldZoneDayData;

    const LOG_LABEL: &'static str = "shield_zone";

    fn entity_id(entity: &ShieldZone) -> String {
        entity.shield_zone_id.to_string()
    }

    fn entity_label(entity: &ShieldZone) -> String {
        entity
            .pull_zone_id
            .map(|id| id.to_string())
            .unwrap_or_default()
    }

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<ShieldZone>>> {
        Box::pin(async move { api_client.list_shield_zones().await })
    }

    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        zone: &'a ShieldZone,
        date: NaiveDate,
        allow_missing: bool,
    ) -> FetchFuture<'a, ShieldZoneDayData> {
        Box::pin(async move {
            let metrics = api_client
                .get_shield_metrics(zone.shield_zone_id, date, date)
                .await?;

            let categories = category_refs(&metrics)
                .into_iter()
                .map(|(name, category)| {
                    let actions =
                        extract_category_for_date(category, date, allow_missing).context(name)?;
                    Ok::<_, anyhow::Error>((name.to_string(), actions))
                })
                .collect::<Result<_>>()?;

            Ok(ShieldZoneDayData {
                categories,
                total_clean_requests_limit: metrics.total_clean_requests_limit,
                total_billable_requests_this_month: metrics.total_billable_requests_this_month,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        zone: &'a ShieldZone,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, ShieldZoneDayData> {
        Box::pin(async move {
            let metrics = api_client
                .get_shield_metrics(zone.shield_zone_id, from, to)
                .await?;

            let categories = category_refs(&metrics)
                .into_iter()
                .map(|(name, cat)| (name.to_string(), sum_category_in_range(cat, from, to)))
                .collect();

            Ok(ShieldZoneDayData {
                categories,
                total_clean_requests_limit: 0,
                total_billable_requests_this_month: 0,
            })
        })
    }

    fn emit_metrics(
        id: &str,
        pull_zone_id: &str,
        last: &ShieldZoneDayData,
        current: &ShieldZoneDayData,
    ) {
        let category_keys: HashSet<&String> = last
            .categories
            .keys()
            .chain(current.categories.keys())
            .collect();

        let empty = HashMap::new();
        for category in category_keys {
            let last_actions = last.categories.get(category).unwrap_or(&empty);
            let current_actions = current.categories.get(category).unwrap_or(&empty);
            let labels = [
                ("shield_zone_id", id.to_string()),
                ("pull_zone_id", pull_zone_id.to_string()),
                ("category", category.clone()),
            ];
            emit_labeled_counter(
                "bunnynet.shield_zone.requests",
                last_actions,
                current_actions,
                "action",
                &labels,
            );
        }

        let gauge_labels = [
            ("shield_zone_id", id.to_string()),
            ("pull_zone_id", pull_zone_id.to_string()),
        ];
        #[allow(clippy::cast_precision_loss)]
        gauge!("bunnynet.shield_zone.clean_requests_limit", &gauge_labels)
            .set(current.total_clean_requests_limit as f64);
        #[allow(clippy::cast_precision_loss)]
        gauge!(
            "bunnynet.shield_zone.billable_requests_this_month",
            &gauge_labels
        )
        .set(current.total_billable_requests_this_month as f64);
    }
}

const fn category_refs(metrics: &ShieldMetrics) -> [(&'static str, &ShieldCategoryMetrics); 7] {
    [
        (WAF, &metrics.waf),
        (DDOS, &metrics.ddos),
        (RATE_LIMIT, &metrics.rate_limit),
        (ACCESS_LISTS, &metrics.access_lists),
        (BOT_DETECTION, &metrics.bot_detection),
        (UPLOAD_SCANNING, &metrics.upload_scanning),
        (API_GUARDIAN, &metrics.api_guardian),
    ]
}

fn extract_category_for_date(
    cat: &ShieldCategoryMetrics,
    date: NaiveDate,
    allow_missing: bool,
) -> Result<HashMap<String, u64>> {
    let mut actions = HashMap::with_capacity(cat.metrics.len());
    for (action, chart) in &cat.metrics {
        let value = find_chart_value_for_date_lenient(chart, date, allow_missing)
            .with_context(|| format!("action {action}"))?;
        actions.insert(action.clone(), value);
    }
    Ok(actions)
}

fn sum_category_in_range(
    cat: &ShieldCategoryMetrics,
    from: NaiveDate,
    to: NaiveDate,
) -> HashMap<String, u64> {
    let mut actions = HashMap::with_capacity(cat.metrics.len());
    for (action, chart) in &cat.metrics {
        actions.insert(action.clone(), sum_chart_values_in_range(chart, from, to));
    }
    actions
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ShieldZoneDayData {
    pub categories: HashMap<String, HashMap<String, u64>>,
    pub total_clean_requests_limit: u64,
    pub total_billable_requests_this_month: u64,
}

impl DayData for ShieldZoneDayData {
    fn accumulate(&mut self, day: Self) {
        for (category, actions) in day.categories {
            let entry = self.categories.entry(category).or_default();
            for (action, value) in actions {
                *entry.entry(action).or_default() += value;
            }
        }
    }

    fn merge_latest(&mut self, snapshot: Self) {
        for (category, actions) in snapshot.categories {
            let entry = self.categories.entry(category).or_default();
            for (action, value) in actions {
                let action_entry = entry.entry(action).or_default();
                *action_entry = (*action_entry).max(value);
            }
        }
        self.total_clean_requests_limit = snapshot.total_clean_requests_limit;
        self.total_billable_requests_this_month = snapshot.total_billable_requests_this_month;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc
)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn category(actions: &[(&str, u64)]) -> HashMap<String, u64> {
        actions
            .iter()
            .map(|(k, v)| ((*k).to_string(), *v))
            .collect()
    }

    fn data(
        categories: &[(&str, &[(&str, u64)])],
        clean_limit: u64,
        billable: u64,
    ) -> ShieldZoneDayData {
        let mut map = HashMap::new();
        for (cat, actions) in categories {
            map.insert((*cat).to_string(), category(actions));
        }
        ShieldZoneDayData {
            categories: map,
            total_clean_requests_limit: clean_limit,
            total_billable_requests_this_month: billable,
        }
    }

    fn shield_category(actions: &[(&str, &[(&str, u64)])]) -> ShieldCategoryMetrics {
        let mut metrics = HashMap::new();
        for (action, points) in actions {
            let chart: HashMap<String, u64> =
                points.iter().map(|(k, v)| ((*k).to_string(), *v)).collect();
            metrics.insert((*action).to_string(), chart);
        }
        ShieldCategoryMetrics { metrics }
    }

    #[test]
    fn entity_label_is_empty_when_pull_zone_id_is_none() {
        let with_pz = ShieldZone {
            shield_zone_id: 1,
            pull_zone_id: Some(42),
        };
        assert_eq!(ShieldZoneKind::entity_label(&with_pz), "42");

        let without_pz = ShieldZone {
            shield_zone_id: 2,
            pull_zone_id: None,
        };
        assert_eq!(ShieldZoneKind::entity_label(&without_pz), "");
    }

    #[test]
    fn accumulate_sums_overlapping_actions_and_inserts_new_categories() {
        let mut state = data(&[("waf", &[("block", 5), ("allow", 10)])], 0, 0);
        state.accumulate(data(
            &[("waf", &[("block", 3)]), ("ddos", &[("drop", 7)])],
            0,
            0,
        ));
        let waf = state.categories.get("waf").unwrap();
        assert_eq!(waf.get("block"), Some(&8));
        assert_eq!(waf.get("allow"), Some(&10));
        assert_eq!(state.categories.get("ddos").unwrap().get("drop"), Some(&7));
    }

    #[test]
    fn merge_latest_max_per_action_keeps_both_sides_overwrites_total_gauges() {
        let mut state = data(&[("waf", &[("block", 10), ("allow", 3)])], 1000, 500);
        state.merge_latest(data(
            &[
                ("waf", &[("block", 5), ("allow", 8)]),
                ("ddos", &[("drop", 7)]),
            ],
            1,
            1,
        ));
        let waf = state.categories.get("waf").unwrap();
        assert_eq!(waf.get("block"), Some(&10));
        assert_eq!(waf.get("allow"), Some(&8));
        assert_eq!(state.categories.get("ddos").unwrap().get("drop"), Some(&7));
        assert_eq!(state.total_clean_requests_limit, 1);
        assert_eq!(state.total_billable_requests_this_month, 1);
    }

    #[test]
    fn extract_category_for_date_maps_each_action_and_errors_when_any_action_missing_date() {
        let cat = shield_category(&[
            ("block", &[("2026-05-24T00:00:00", 7)]),
            ("allow", &[("2026-05-24T00:00:00", 12)]),
        ]);
        let result = extract_category_for_date(&cat, date(2026, 5, 24), false).unwrap();
        assert_eq!(result.get("block"), Some(&7));
        assert_eq!(result.get("allow"), Some(&12));

        let missing = shield_category(&[
            ("block", &[("2026-05-24T00:00:00", 7)]),
            ("allow", &[("2026-05-25T00:00:00", 12)]),
        ]);
        assert!(extract_category_for_date(&missing, date(2026, 5, 24), false).is_err());
    }

    #[test]
    fn extract_category_for_date_allow_missing_treats_missing_action_as_zero() {
        let missing = shield_category(&[
            ("block", &[("2026-05-24T00:00:00", 7)]),
            ("allow", &[("2026-05-25T00:00:00", 12)]),
        ]);
        let result = extract_category_for_date(&missing, date(2026, 5, 24), true).unwrap();
        assert_eq!(result.get("block"), Some(&7));
        assert_eq!(result.get("allow"), Some(&0));
    }

    #[test]
    fn sum_category_in_range_sums_each_action_chart() {
        let cat = shield_category(&[
            (
                "block",
                &[
                    ("2026-05-24T00:00:00", 5),
                    ("2026-05-25T00:00:00", 10),
                    ("2026-05-26T00:00:00", 100),
                ],
            ),
            ("allow", &[("2026-05-25T00:00:00", 3)]),
            ("scan", &[("2026-05-30T00:00:00", 99)]),
        ]);
        let result = sum_category_in_range(&cat, date(2026, 5, 24), date(2026, 5, 25));
        assert_eq!(result.get("block"), Some(&15));
        assert_eq!(result.get("allow"), Some(&3));
        assert_eq!(result.get("scan"), Some(&0));
    }
}
