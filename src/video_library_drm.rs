use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, VideoLibrary};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
};

pub type VideoLibraryDrmStatsState = EntityStatsState<VideoLibraryDrmKind>;

const LICENSES_ISSUED: &str = "licenses_issued";

pub struct VideoLibraryDrmKind;

impl EntityType for VideoLibraryDrmKind {
    type Entity = VideoLibrary;
    type DayData = VideoLibraryDrmDayData;

    const LOG_LABEL: &'static str = "Video library DRM";

    fn entity_id(entity: &VideoLibrary) -> u64 {
        entity.id
    }

    fn entity_label(entity: &VideoLibrary) -> String {
        entity.name.clone()
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
                    .context(LICENSES_ISSUED)?,
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
