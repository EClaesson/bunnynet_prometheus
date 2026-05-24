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
    ) -> FetchFuture<'a, ShieldZoneDayData> {
        Box::pin(async move {
            let metrics = api_client
                .get_shield_metrics(zone.shield_zone_id, date, date)
                .await?;

            let categories = category_refs(&metrics)
                .into_iter()
                .map(|(name, category)| {
                    let actions = extract_category_for_date(category, date).context(name)?;
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
) -> Result<HashMap<String, u64>> {
    let mut actions = HashMap::with_capacity(cat.metrics.len());
    for (action, chart) in &cat.metrics {
        let value = find_chart_value_for_date_lenient(chart, date)
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
