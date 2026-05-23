use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::gauge;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, StorageZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date,
};

pub type StorageZoneStatsState = EntityStatsState<StorageZoneKind>;

const STORAGE_USED: &str = "storage_used";
const FILE_COUNT: &str = "file_count";

pub struct StorageZoneKind;

impl EntityType for StorageZoneKind {
    type Entity = StorageZone;
    type DayData = StorageDayData;

    const LOG_LABEL: &'static str = "storage_zone";

    fn entity_id(entity: &StorageZone) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &StorageZone) -> String {
        entity.name.clone()
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Arc<Vec<StorageZone>>> {
        Box::pin(async move { client.list_storage_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a StorageZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, StorageDayData> {
        Box::pin(async move {
            let stats = client.get_storage_zone_stats(zone.id, date, date).await?;

            let storage_used =
                find_chart_value_for_date(&stats.storage_used_chart, date).context(STORAGE_USED)?;
            let file_count =
                find_chart_value_for_date(&stats.file_count_chart, date).context(FILE_COUNT)?;

            Ok(StorageDayData {
                storage_used,
                file_count,
            })
        })
    }

    #[allow(clippy::cast_precision_loss)]
    fn emit_metrics(id: &str, name: &str, _last: &StorageDayData, current: &StorageDayData) {
        let labels = [("zone_id", id.to_string()), ("name", name.to_string())];

        gauge!("bunnynet.storage_zone.storage_used", &labels).set(current.storage_used as f64);
        gauge!("bunnynet.storage_zone.file_count", &labels).set(current.file_count as f64);
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct StorageDayData {
    pub storage_used: u64,
    pub file_count: u64,
}

impl DayData for StorageDayData {
    fn accumulate(&mut self, _day: Self) {}

    fn merge_latest(&mut self, snap: Self) {
        self.storage_used = snap.storage_used;
        self.file_count = snap.file_count;
    }
}
