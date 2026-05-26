use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use metrics::{counter, gauge};
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, Application, ApplicationVolumeChart};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
    sum_chart_f64_as_u64,
};

pub type ApplicationStatsState = EntityStatsState<ApplicationKind>;

pub struct ApplicationKind;

impl EntityType for ApplicationKind {
    type Entity = Application;
    type DayData = ApplicationDayData;

    const LOG_LABEL: &'static str = "application";

    fn entity_id(entity: &Application) -> String {
        entity.id.clone()
    }

    fn entity_label(entity: &Application) -> String {
        entity.name.clone()
    }

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<Application>>> {
        Box::pin(async move { api_client.list_applications().await })
    }

    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        application: &'a Application,
        date: NaiveDate,
    ) -> FetchFuture<'a, ApplicationDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_application_stats(&application.id, date, date)
                .await?;

            let target_latency = find_chart_value_for_date(&statistics.target_latency_chart, date)
                .context("target_latency")?;
            let active_regions = find_chart_value_for_date(&statistics.active_regions_chart, date)
                .context("active_regions")?
                .unwrap_or(0.0);
            let latency =
                find_chart_value_for_date(&statistics.latency_chart, date).context("latency")?;
            let instances = find_chart_value_for_date(&statistics.instances_chart, date)
                .context("instances")?
                .unwrap_or(0.0);
            let cpu_usage = find_chart_value_for_date(&statistics.cpu_usage_chart, date)
                .context("cpu_usage")?;
            let ram_usage = find_chart_value_for_date(&statistics.ram_usage_chart, date)
                .context("ram_usage")?;
            let traffic = f64_to_u64(
                find_chart_value_for_date(&statistics.traffic_chart, date).context("traffic")?,
            );
            let volume_usage =
                extract_volume_chart_for_date(&statistics.volumes_split_usage_chart, date)
                    .context("volume_usage")?;
            let volume_capacity =
                extract_volume_chart_for_date(&statistics.volumes_split_capacity_chart, date)
                    .context("volume_capacity")?;

            Ok(ApplicationDayData {
                target_latency,
                active_regions,
                latency,
                instances,
                cpu_usage,
                ram_usage,
                traffic,
                volume_usage,
                volume_capacity,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        application: &'a Application,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, ApplicationDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_application_stats(&application.id, from, to)
                .await?;

            Ok(ApplicationDayData {
                traffic: sum_chart_f64_as_u64(&statistics.traffic_chart),
                ..ApplicationDayData::default()
            })
        })
    }

    fn emit_metrics(id: &str, name: &str, last: &ApplicationDayData, current: &ApplicationDayData) {
        let application_id = id.to_string();
        let application_name = name.to_string();
        let labels = [
            ("app_id", application_id.clone()),
            ("name", application_name.clone()),
        ];

        gauge!("bunnynet.application.target_latency", &labels).set(current.target_latency);
        gauge!("bunnynet.application.active_regions", &labels).set(current.active_regions);
        gauge!("bunnynet.application.latency", &labels).set(current.latency);
        gauge!("bunnynet.application.instances", &labels).set(current.instances);
        gauge!("bunnynet.application.cpu_usage", &labels).set(current.cpu_usage);
        gauge!("bunnynet.application.ram_usage", &labels).set(current.ram_usage);
        counter!("bunnynet.application.traffic", &labels).absolute(last.traffic + current.traffic);

        for (volume, value) in &current.volume_usage {
            gauge!(
                "bunnynet.application.volume_usage",
                "app_id" => application_id.clone(),
                "name" => application_name.clone(),
                "volume" => volume.clone(),
            )
            .set(*value);
        }

        for (volume, value) in &current.volume_capacity {
            gauge!(
                "bunnynet.application.volume_capacity",
                "app_id" => application_id.clone(),
                "name" => application_name.clone(),
                "volume" => volume.clone(),
            )
            .set(*value);
        }
    }
}

fn extract_volume_chart_for_date(
    chart: &ApplicationVolumeChart,
    date: NaiveDate,
) -> Result<HashMap<String, f64>> {
    let mut result = HashMap::with_capacity(chart.len());
    for (volume, time_series) in chart {
        let value = find_chart_value_for_date(time_series, date)
            .with_context(|| format!("volume {volume}"))?;
        result.insert(volume.clone(), value);
    }
    Ok(result)
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ApplicationDayData {
    pub target_latency: f64,
    pub active_regions: f64,
    pub latency: f64,
    pub instances: f64,
    pub cpu_usage: f64,
    pub ram_usage: f64,
    pub traffic: u64,
    pub volume_usage: HashMap<String, f64>,
    pub volume_capacity: HashMap<String, f64>,
}

impl DayData for ApplicationDayData {
    fn accumulate(&mut self, day: Self) {
        self.traffic += day.traffic;
    }

    fn merge_latest(&mut self, snapshot: Self) {
        self.target_latency = snapshot.target_latency;
        self.active_regions = snapshot.active_regions;
        self.latency = snapshot.latency;
        self.instances = snapshot.instances;
        self.cpu_usage = snapshot.cpu_usage;
        self.ram_usage = snapshot.ram_usage;
        self.traffic = self.traffic.max(snapshot.traffic);
        self.volume_usage = snapshot.volume_usage;
        self.volume_capacity = snapshot.volume_capacity;
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::float_cmp,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn sample(seed: u64) -> ApplicationDayData {
        let mut volume_usage = HashMap::new();
        volume_usage.insert("data".to_string(), seed as f64);
        let mut volume_capacity = HashMap::new();
        volume_capacity.insert("data".to_string(), (seed as f64) * 10.0);
        ApplicationDayData {
            target_latency: seed as f64,
            active_regions: (seed as f64) + 1.0,
            latency: (seed as f64) + 2.0,
            instances: (seed as f64) + 3.0,
            cpu_usage: (seed as f64) / 100.0,
            ram_usage: (seed as f64) / 50.0,
            traffic: seed * 1000,
            volume_usage,
            volume_capacity,
        }
    }

    fn volume_chart(volumes: &[(&str, &[(&str, f64)])]) -> ApplicationVolumeChart {
        let mut chart = ApplicationVolumeChart::new();
        for (volume, points) in volumes {
            let series: HashMap<String, f64> =
                points.iter().map(|(k, v)| ((*k).to_string(), *v)).collect();
            chart.insert((*volume).to_string(), series);
        }
        chart
    }

    #[test]
    fn accumulate_sums_traffic_only() {
        let mut state = sample(10);
        let original = sample(10);
        state.accumulate(sample(3));
        assert_eq!(state.traffic, 13_000);
        assert_eq!(state.target_latency, original.target_latency);
        assert_eq!(state.active_regions, original.active_regions);
        assert_eq!(state.latency, original.latency);
        assert_eq!(state.instances, original.instances);
        assert_eq!(state.cpu_usage, original.cpu_usage);
        assert_eq!(state.ram_usage, original.ram_usage);
        assert_eq!(state.volume_usage, original.volume_usage);
        assert_eq!(state.volume_capacity, original.volume_capacity);
    }

    #[test]
    fn merge_latest_overwrites_gauges_and_takes_max_of_traffic() {
        let mut state = sample(10);
        state.merge_latest(sample(3));
        assert_eq!(state.target_latency, 3.0);
        assert_eq!(state.active_regions, 4.0);
        assert_eq!(state.latency, 5.0);
        assert_eq!(state.instances, 6.0);
        assert_eq!(state.cpu_usage, 0.03);
        assert_eq!(state.ram_usage, 0.06);
        assert_eq!(state.traffic, 10_000);
    }

    #[test]
    fn merge_latest_replaces_volume_maps_wholesale() {
        let mut state = sample(10);
        state.volume_usage.insert("logs".to_string(), 999.0);
        state.merge_latest(sample(3));
        assert_eq!(state.volume_usage.get("data"), Some(&3.0));
        assert!(!state.volume_usage.contains_key("logs"));
        assert_eq!(state.volume_capacity.get("data"), Some(&30.0));
    }

    #[test]
    fn extract_volume_chart_for_date_maps_each_volume_and_errors_on_missing_date() {
        let chart = volume_chart(&[
            ("data", &[("2026-05-24", 12.5)]),
            ("logs", &[("2026-05-24", 4.0)]),
        ]);
        let result = extract_volume_chart_for_date(&chart, date(2026, 5, 24)).unwrap();
        assert_eq!(result.get("data"), Some(&12.5));
        assert_eq!(result.get("logs"), Some(&4.0));

        let missing = volume_chart(&[
            ("data", &[("2026-05-24", 12.5)]),
            ("logs", &[("2026-05-25", 4.0)]),
        ]);
        assert!(extract_volume_chart_for_date(&missing, date(2026, 5, 24)).is_err());
    }
}
