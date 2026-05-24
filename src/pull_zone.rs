use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, GeoTrafficDistribution, PullZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, emit_labeled_counter,
    find_chart_value_for_date, sum_chart_values,
};

pub type PullZoneStatsState = EntityStatsState<PullZoneKind>;

const ORIGIN_RESPONSE_TIME: &str = "origin_response_time";
const CACHE_HIT_RATE: &str = "cache_hit_rate";
const BANDWIDTH_USED: &str = "bandwidth_used";
const BANDWIDTH_CACHED: &str = "bandwidth_cached";
const REQUESTS_SERVED: &str = "requests_served";
const PULL_REQUESTS_PULLED: &str = "pull_requests_pulled";
const ORIGIN_SHIELD_BANDWIDTH_USED: &str = "origin_shield_bandwidth_used";
const ORIGIN_SHIELD_INTERNAL_BANDWIDTH_USED: &str = "origin_shield_internal_bandwidth_used";
const ORIGIN_TRAFFIC: &str = "origin_traffic";
const ERRORS_3XX: &str = "errors_3xx";
const ERRORS_4XX: &str = "errors_4xx";
const ERRORS_5XX: &str = "errors_5xx";

pub struct PullZoneKind;

impl EntityType for PullZoneKind {
    type Entity = PullZone;
    type DayData = PullZoneDayData;

    const LOG_LABEL: &'static str = "pull_zone";

    fn entity_id(entity: &PullZone) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &PullZone) -> String {
        entity.name.clone()
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Arc<Vec<PullZone>>> {
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
                    .context(ORIGIN_RESPONSE_TIME)?;
            let cache_hit_rate = find_chart_value_for_date(&stats.cache_hit_rate_chart, date)
                .context(CACHE_HIT_RATE)?;
            let bandwidth_used = find_chart_value_for_date(&stats.bandwidth_used_chart, date)
                .context(BANDWIDTH_USED)?;
            let bandwidth_cached = find_chart_value_for_date(&stats.bandwidth_cached_chart, date)
                .context(BANDWIDTH_CACHED)?;
            let requests_served = find_chart_value_for_date(&stats.requests_served_chart, date)
                .context(REQUESTS_SERVED)?;
            let pull_requests_pulled =
                find_chart_value_for_date(&stats.pull_requests_pulled_chart, date)
                    .context(PULL_REQUESTS_PULLED)?;
            let origin_shield_bandwidth_used =
                find_chart_value_for_date(&stats.origin_shield_bandwidth_used_chart, date)
                    .context(ORIGIN_SHIELD_BANDWIDTH_USED)?;
            let origin_shield_internal_bandwidth_used =
                find_chart_value_for_date(&stats.origin_shield_internal_bandwidth_used_chart, date)
                    .context(ORIGIN_SHIELD_INTERNAL_BANDWIDTH_USED)?;
            let origin_traffic = find_chart_value_for_date(&stats.origin_traffic_chart, date)
                .context(ORIGIN_TRAFFIC)?;
            let errors_3xx =
                find_chart_value_for_date(&stats.errors_3xx_chart, date).context(ERRORS_3XX)?;
            let errors_4xx =
                find_chart_value_for_date(&stats.errors_4xx_chart, date).context(ERRORS_4XX)?;
            let errors_5xx =
                find_chart_value_for_date(&stats.errors_5xx_chart, date).context(ERRORS_5XX)?;

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

    fn fetch_range<'a>(
        client: &'a ApiClient,
        zone: &'a PullZone,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, PullZoneDayData> {
        Box::pin(async move {
            let stats = client.get_pull_zone_stats(zone.id, from, to).await?;

            Ok(PullZoneDayData {
                origin_response_time: 0.0,
                cache_hit_rate: 0.0,
                bandwidth_used: sum_chart_values(&stats.bandwidth_used_chart),
                bandwidth_cached: sum_chart_values(&stats.bandwidth_cached_chart),
                requests_served: sum_chart_values(&stats.requests_served_chart),
                pull_requests_pulled: sum_chart_values(&stats.pull_requests_pulled_chart),
                origin_shield_bandwidth_used: sum_chart_values(
                    &stats.origin_shield_bandwidth_used_chart,
                ),
                origin_shield_internal_bandwidth_used: sum_chart_values(
                    &stats.origin_shield_internal_bandwidth_used_chart,
                ),
                origin_traffic: sum_chart_values(&stats.origin_traffic_chart),
                errors_3xx: sum_chart_values(&stats.errors_3xx_chart),
                errors_4xx: sum_chart_values(&stats.errors_4xx_chart),
                errors_5xx: sum_chart_values(&stats.errors_5xx_chart),
                geo_traffic_distribution: stats.geo_traffic_distribution,
            })
        })
    }

    fn emit_metrics(id: &str, name: &str, last: &PullZoneDayData, current: &PullZoneDayData) {
        let labels = [("zone_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.pull_zone.bandwidth_used", &labels)
            .absolute(last.bandwidth_used + current.bandwidth_used);
        counter!("bunnynet.pull_zone.bandwidth_cached", &labels)
            .absolute(last.bandwidth_cached + current.bandwidth_cached);
        counter!("bunnynet.pull_zone.requests_served", &labels)
            .absolute(last.requests_served + current.requests_served);
        counter!("bunnynet.pull_zone.pull_requests_pulled", &labels)
            .absolute(last.pull_requests_pulled + current.pull_requests_pulled);
        counter!("bunnynet.pull_zone.origin_shield_bandwidth_used", &labels)
            .absolute(last.origin_shield_bandwidth_used + current.origin_shield_bandwidth_used);
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

        emit_labeled_counter(
            "bunnynet.pull_zone.geo_traffic",
            &last.geo_traffic_distribution,
            &current.geo_traffic_distribution,
            "region",
            &labels,
        );
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
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
