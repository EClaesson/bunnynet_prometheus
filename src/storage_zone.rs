use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, StorageZone};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, find_chart_value_for_date,
};

pub type StorageZoneStatsState = EntityStatsState<StorageZoneKind>;

pub struct StorageZoneKind;

impl EntityType for StorageZoneKind {
    type Entity = StorageZone;
    type DayData = StorageDayData;

    const LOG_LABEL: &'static str = "storage zone";

    fn entity_id(entity: &StorageZone) -> u64 {
        entity.id
    }

    fn entity_label(entity: &StorageZone) -> &str {
        &entity.name
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<StorageZone>> {
        Box::pin(async move { client.list_storage_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a StorageZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, StorageDayData> {
        Box::pin(async move {
            let stats = client.get_storage_zone_stats(zone.id, date, date).await?;

            let storage_used = find_chart_value_for_date(&stats.storage_used_chart, date)
                .context("Storage used")?;
            let file_count =
                find_chart_value_for_date(&stats.file_count_chart, date).context("File count")?;

            Ok(StorageDayData {
                storage_used,
                file_count,
            })
        })
    }

    fn emit_metrics(id: u64, name: &str, last: &StorageDayData, current: &StorageDayData) {
        let labels = [("zone_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.storage_zone.storage_used", &labels)
            .absolute(last.storage_used + current.storage_used);
        counter!("bunnynet.storage_zone.file_count", &labels)
            .absolute(last.file_count + current.file_count);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct StorageDayData {
    pub storage_used: u64,
    pub file_count: u64,
}

impl DayData for StorageDayData {
    fn accumulate(&mut self, day: Self) {
        self.storage_used += day.storage_used;
        self.file_count += day.file_count;
    }

    fn merge_latest(&mut self, snap: Self) {
        self.storage_used = self.storage_used.max(snap.storage_used);
        self.file_count = self.file_count.max(snap.file_count);
    }
}
