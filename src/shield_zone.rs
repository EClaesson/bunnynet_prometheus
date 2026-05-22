use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, ShieldCategoryMetrics, ShieldZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date_multi,
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

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<ShieldZone>> {
        Box::pin(async move { client.list_shield_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a ShieldZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, ShieldZoneDayData> {
        Box::pin(async move {
            let metrics = client.get_shield_metrics(zone.shield_zone_id, date).await?;

            let mut categories = HashMap::new();
            categories.insert(
                WAF.to_string(),
                extract_category_for_date(&metrics.waf, date).context(WAF)?,
            );
            categories.insert(
                DDOS.to_string(),
                extract_category_for_date(&metrics.ddos, date).context(DDOS)?,
            );
            categories.insert(
                RATE_LIMIT.to_string(),
                extract_category_for_date(&metrics.rate_limit, date)
                    .context(RATE_LIMIT)?,
            );
            categories.insert(
                ACCESS_LISTS.to_string(),
                extract_category_for_date(&metrics.access_lists, date)
                    .context(ACCESS_LISTS)?,
            );
            categories.insert(
                BOT_DETECTION.to_string(),
                extract_category_for_date(&metrics.bot_detection, date)
                    .context(BOT_DETECTION)?,
            );
            categories.insert(
                UPLOAD_SCANNING.to_string(),
                extract_category_for_date(&metrics.upload_scanning, date)
                    .context(UPLOAD_SCANNING)?,
            );
            categories.insert(
                API_GUARDIAN.to_string(),
                extract_category_for_date(&metrics.api_guardian, date)
                    .context(API_GUARDIAN)?,
            );

            Ok(ShieldZoneDayData {
                categories,
                total_clean_requests_limit: metrics.total_clean_requests_limit,
                total_billable_requests_this_month: metrics.total_billable_requests_this_month,
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

        for category in category_keys {
            let empty = HashMap::new();
            let last_actions = last.categories.get(category).unwrap_or(&empty);
            let current_actions = current.categories.get(category).unwrap_or(&empty);
            let action_keys: HashSet<&String> = last_actions
                .keys()
                .chain(current_actions.keys())
                .collect();

            for action in action_keys {
                let total = last_actions.get(action).copied().unwrap_or(0)
                    + current_actions.get(action).copied().unwrap_or(0);
                counter!(
                    "bunnynet.shield_zone.requests",
                    "shield_zone_id" => id.to_string(),
                    "pull_zone_id" => pull_zone_id.to_string(),
                    "category" => category.clone(),
                    "action" => action.clone(),
                )
                .absolute(total);
            }
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

fn extract_category_for_date(
    cat: &ShieldCategoryMetrics,
    date: NaiveDate,
) -> Result<HashMap<String, u64>> {
    let mut actions = HashMap::with_capacity(cat.metrics.len());
    for (action, chart) in &cat.metrics {
        let value = find_chart_value_for_date_multi(chart, date)
            .with_context(|| format!("action {action}"))?;
        actions.insert(action.clone(), value);
    }
    Ok(actions)
}

#[derive(Serialize, Deserialize, Default)]
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

    fn merge_latest(&mut self, snap: Self) {
        for (category, actions) in snap.categories {
            let entry = self.categories.entry(category).or_default();
            for (action, value) in actions {
                let action_entry = entry.entry(action).or_default();
                *action_entry = (*action_entry).max(value);
            }
        }
        self.total_clean_requests_limit = snap.total_clean_requests_limit;
        self.total_billable_requests_this_month = snap.total_billable_requests_this_month;
    }
}
