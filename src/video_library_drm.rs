use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, VideoLibrary};
use crate::zone_stats::{
    DayData, FetchFuture, ZoneStatsState, ZoneType, f64_to_u64, find_chart_value_for_date,
};

pub type VideoLibraryDrmStatsState = ZoneStatsState<VideoLibraryDrmKind>;

pub struct VideoLibraryDrmKind;

impl ZoneType for VideoLibraryDrmKind {
    type Entity = VideoLibrary;
    type DayData = VideoLibraryDrmDayData;

    const LOG_LABEL: &'static str = "Video library DRM";

    fn entity_id(entity: &VideoLibrary) -> u64 {
        entity.id
    }

    fn entity_label(entity: &VideoLibrary) -> &str {
        &entity.name
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<VideoLibrary>> {
        Box::pin(async move { client.list_video_libraries().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        library: &'a VideoLibrary,
        date: NaiveDate,
    ) -> FetchFuture<'a, VideoLibraryDrmDayData> {
        Box::pin(async move {
            let stats = client
                .get_video_library_drm_stats(library.id, date, date)
                .await?;

            let licenses_issued = f64_to_u64(
                find_chart_value_for_date(&stats.licenses_issued_chart, date)
                    .context("Licenses issued")?,
            );

            Ok(VideoLibraryDrmDayData { licenses_issued })
        })
    }

    fn emit_metrics(
        id: u64,
        name: &str,
        last: &VideoLibraryDrmDayData,
        current: &VideoLibraryDrmDayData,
    ) {
        let labels = [("library_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.video_library_drm.licenses_issued", &labels)
            .absolute(last.licenses_issued + current.licenses_issued);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct VideoLibraryDrmDayData {
    pub licenses_issued: u64,
}

impl DayData for VideoLibraryDrmDayData {
    fn accumulate(&mut self, day: Self) {
        self.licenses_issued += day.licenses_issued;
    }

    fn merge_latest(&mut self, snap: Self) {
        self.licenses_issued = self.licenses_issued.max(snap.licenses_issued);
    }
}
