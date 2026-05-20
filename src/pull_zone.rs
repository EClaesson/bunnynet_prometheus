use std::collections::HashSet;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, GeoTrafficDistribution, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date,
};

pub type PullZoneStatsState = EntityStatsState<PullZoneKind>;

pub struct PullZoneKind;

impl EntityType for PullZoneKind {
    type Entity = PullZone;
    type DayData = PullZoneDayData;

    const LOG_LABEL: &'static str = "Pull zone";

    fn entity_id(entity: &PullZone) -> u64 {
        entity.id
    }

    fn entity_label(entity: &PullZone) -> &str {
        &entity.name
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<PullZone>> {
        Box::pin(async move { client.list_pull_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a PullZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, PullZoneDayData> {
        Box::pin(async move {
            let stats = client.get_pull_zone_stats(zone.id, date, date).await?;

            let origin_response_time =
                find_chart_value_for_date(&stats.origin_response_time_chart, date)
                    .context("Origin response time")?;
            let cache_hit_rate = find_chart_value_for_date(&stats.cache_hit_rate_chart, date)
                .context("Cache hit rate")?;
            let bandwidth_used = find_chart_value_for_date(&stats.bandwidth_used_chart, date)
                .context("Bandwidth used")?;
            let bandwidth_cached = find_chart_value_for_date(&stats.bandwidth_cached_chart, date)
                .context("Bandwidth cached")?;
            let requests_served = find_chart_value_for_date(&stats.requests_served_chart, date)
                .context("Requests served")?;
            let pull_requests_pulled =
                find_chart_value_for_date(&stats.pull_requests_pulled_chart, date)
                    .context("Pull requests pulled")?;
            let origin_shield_bandwidth_used =
                find_chart_value_for_date(&stats.origin_shield_bandwidth_used_chart, date)
                    .context("Origin shield bandwidth used")?;
            let origin_shield_internal_bandwidth_used = find_chart_value_for_date(
                &stats.origin_shield_internal_bandwidth_used_chart,
                date,
            )
            .context("Origin shield internal bandwidth used")?;
            let origin_traffic = find_chart_value_for_date(&stats.origin_traffic_chart, date)
                .context("Origin traffic")?;
            let errors_3xx = find_chart_value_for_date(&stats.errors_3xx_chart, date)
                .context("Errors 3xx")?;
            let errors_4xx = find_chart_value_for_date(&stats.errors_4xx_chart, date)
                .context("Errors 4xx")?;
            let errors_5xx = find_chart_value_for_date(&stats.errors_5xx_chart, date)
                .context("Errors 5xx")?;

            Ok(PullZoneDayData {
                origin_response_time,
                cache_hit_rate,
                bandwidth_used,
                bandwidth_cached,
                requests_served,
                pull_requests_pulled,
                origin_shield_bandwidth_used,
                origin_shield_internal_bandwidth_used,
                origin_traffic,
                errors_3xx,
                errors_4xx,
                errors_5xx,
                geo_traffic_distribution: stats.geo_traffic_distribution,
            })
        })
    }

    fn emit_metrics(id: u64, name: &str, last: &PullZoneDayData, current: &PullZoneDayData) {
        let id_str = id.to_string();
        let labels = [("zone_id", id_str.clone()), ("name", name.to_string())];

        counter!("bunnynet.pull_zone.bandwidth_used", &labels)
            .absolute(last.bandwidth_used + current.bandwidth_used);
        counter!("bunnynet.pull_zone.bandwidth_cached", &labels)
            .absolute(last.bandwidth_cached + current.bandwidth_cached);
        counter!("bunnynet.pull_zone.requests_served", &labels)
            .absolute(last.requests_served + current.requests_served);
        counter!("bunnynet.pull_zone.pull_requests_pulled", &labels)
            .absolute(last.pull_requests_pulled + current.pull_requests_pulled);
        counter!("bunnynet.pull_zone.origin_shield_bandwidth_used", &labels).absolute(
            last.origin_shield_bandwidth_used + current.origin_shield_bandwidth_used,
        );
        counter!(
            "bunnynet.pull_zone.origin_shield_internal_bandwidth_used",
            &labels
        )
        .absolute(
            last.origin_shield_internal_bandwidth_used
                + current.origin_shield_internal_bandwidth_used,
        );
        counter!("bunnynet.pull_zone.origin_traffic", &labels)
            .absolute(last.origin_traffic + current.origin_traffic);
        counter!("bunnynet.pull_zone.errors_3xx", &labels)
            .absolute(last.errors_3xx + current.errors_3xx);
        counter!("bunnynet.pull_zone.errors_4xx", &labels)
            .absolute(last.errors_4xx + current.errors_4xx);
        counter!("bunnynet.pull_zone.errors_5xx", &labels)
            .absolute(last.errors_5xx + current.errors_5xx);

        gauge!("bunnynet.pull_zone.origin_response_time", &labels)
            .set(current.origin_response_time);
        gauge!("bunnynet.pull_zone.cache_hit_rate", &labels).set(current.cache_hit_rate);

        let unique_regions: HashSet<&String> = last
            .geo_traffic_distribution
            .keys()
            .chain(current.geo_traffic_distribution.keys())
            .collect();

        for region in unique_regions {
            let total = last
                .geo_traffic_distribution
                .get(region)
                .copied()
                .unwrap_or(0)
                + current
                    .geo_traffic_distribution
                    .get(region)
                    .copied()
                    .unwrap_or(0);

            counter!(
                "bunnynet.pull_zone.geo_traffic",
                "zone_id" => id_str.clone(),
                "name" => name.to_string(),
                "region" => region.clone(),
            )
            .absolute(total);
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct PullZoneDayData {
    pub origin_response_time: f64,
    pub cache_hit_rate: f64,
    pub bandwidth_used: u64,
    pub bandwidth_cached: u64,
    pub requests_served: u64,
    pub pull_requests_pulled: u64,
    pub origin_shield_bandwidth_used: u64,
    pub origin_shield_internal_bandwidth_used: u64,
    pub origin_traffic: u64,
    pub errors_3xx: u64,
    pub errors_4xx: u64,
    pub errors_5xx: u64,
    pub geo_traffic_distribution: GeoTrafficDistribution,
}

impl DayData for PullZoneDayData {
    fn accumulate(&mut self, day: Self) {
        self.bandwidth_used += day.bandwidth_used;
        self.bandwidth_cached += day.bandwidth_cached;
        self.requests_served += day.requests_served;
        self.pull_requests_pulled += day.pull_requests_pulled;
        self.origin_shield_bandwidth_used += day.origin_shield_bandwidth_used;
        self.origin_shield_internal_bandwidth_used += day.origin_shield_internal_bandwidth_used;
        self.origin_traffic += day.origin_traffic;
        self.errors_3xx += day.errors_3xx;
        self.errors_4xx += day.errors_4xx;
        self.errors_5xx += day.errors_5xx;
        for (region, value) in day.geo_traffic_distribution {
            *self.geo_traffic_distribution.entry(region).or_default() += value;
        }
    }

    fn merge_latest(&mut self, snap: Self) {
        self.bandwidth_used = self.bandwidth_used.max(snap.bandwidth_used);
        self.bandwidth_cached = self.bandwidth_cached.max(snap.bandwidth_cached);
        self.requests_served = self.requests_served.max(snap.requests_served);
        self.pull_requests_pulled = self.pull_requests_pulled.max(snap.pull_requests_pulled);
        self.origin_shield_bandwidth_used = self
            .origin_shield_bandwidth_used
            .max(snap.origin_shield_bandwidth_used);
        self.origin_shield_internal_bandwidth_used = self
            .origin_shield_internal_bandwidth_used
            .max(snap.origin_shield_internal_bandwidth_used);
        self.origin_traffic = self.origin_traffic.max(snap.origin_traffic);
        self.errors_3xx = self.errors_3xx.max(snap.errors_3xx);
        self.errors_4xx = self.errors_4xx.max(snap.errors_4xx);
        self.errors_5xx = self.errors_5xx.max(snap.errors_5xx);
        self.origin_response_time = snap.origin_response_time;
        self.cache_hit_rate = snap.cache_hit_rate;
        for (region, value) in snap.geo_traffic_distribution {
            let entry = self.geo_traffic_distribution.entry(region).or_default();
            *entry = (*entry).max(value);
        }
    }
}
