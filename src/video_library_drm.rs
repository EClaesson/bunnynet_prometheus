use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, VideoLibrary};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
    sum_chart_f64_as_u64,
};

pub type VideoLibraryDrmStatsState = EntityStatsState<VideoLibraryDrmKind>;

pub struct VideoLibraryDrmKind;

impl EntityType for VideoLibraryDrmKind {
    type Entity = VideoLibrary;
    type DayData = VideoLibraryDrmDayData;

    const LOG_LABEL: &'static str = "video_library_drm";

    fn entity_id(entity: &VideoLibrary) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &VideoLibrary) -> String {
        entity.name.clone()
    }

    fn list(api_client: &ApiClient) -> FetchFuture<'_, Arc<Vec<VideoLibrary>>> {
        Box::pin(async move { api_client.list_video_libraries().await })
    }

    fn fetch_day<'a>(
        api_client: &'a ApiClient,
        library: &'a VideoLibrary,
        date: NaiveDate,
        allow_missing: bool,
    ) -> FetchFuture<'a, VideoLibraryDrmDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_drm_stats(library.id, date, date)
                .await?;

            let licenses_issued = f64_to_u64(
                find_chart_value_for_date(&statistics.licenses_issued_chart, date, allow_missing)
                    .context("licenses_issued")?,
            );

            Ok(VideoLibraryDrmDayData { licenses_issued })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        library: &'a VideoLibrary,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, VideoLibraryDrmDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_drm_stats(library.id, from, to)
                .await?;

            let licenses_issued = sum_chart_f64_as_u64(&statistics.licenses_issued_chart);

            Ok(VideoLibraryDrmDayData { licenses_issued })
        })
    }

    fn emit_metrics(
        id: &str,
        name: &str,
        last: &VideoLibraryDrmDayData,
        current: &VideoLibraryDrmDayData,
    ) {
        let labels = [("library_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.video_library_drm.licenses_issued", &labels)
            .absolute(last.licenses_issued + current.licenses_issued);
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct VideoLibraryDrmDayData {
    pub licenses_issued: u64,
}

impl DayData for VideoLibraryDrmDayData {
    fn accumulate(&mut self, day: Self) {
        self.licenses_issued += day.licenses_issued;
    }

    fn merge_latest(&mut self, snapshot: Self) {
        self.licenses_issued = self.licenses_issued.max(snapshot.licenses_issued);
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

    #[test]
    fn accumulate_sums() {
        let mut state = VideoLibraryDrmDayData {
            licenses_issued: 10,
        };
        state.accumulate(VideoLibraryDrmDayData { licenses_issued: 3 });
        assert_eq!(state.licenses_issued, 13);
    }

    #[test]
    fn merge_latest_takes_max() {
        let mut state = VideoLibraryDrmDayData {
            licenses_issued: 50,
        };
        state.merge_latest(VideoLibraryDrmDayData {
            licenses_issued: 30,
        });
        assert_eq!(state.licenses_issued, 50);
    }
}
