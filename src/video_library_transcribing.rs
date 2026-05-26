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

pub type VideoLibraryTranscribingStatsState = EntityStatsState<VideoLibraryTranscribingKind>;

pub struct VideoLibraryTranscribingKind;

impl EntityType for VideoLibraryTranscribingKind {
    type Entity = VideoLibrary;
    type DayData = VideoLibraryTranscribingDayData;

    const LOG_LABEL: &'static str = "video_library_transcribing";

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
    ) -> FetchFuture<'a, VideoLibraryTranscribingDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_transcribing_stats(library.id, date, date)
                .await?;

            let transcription_seconds = f64_to_u64(
                find_chart_value_for_date(&statistics.transcription_seconds_chart, date)
                    .context("transcription_seconds")?,
            );

            Ok(VideoLibraryTranscribingDayData {
                transcription_seconds,
            })
        })
    }

    fn fetch_range<'a>(
        api_client: &'a ApiClient,
        library: &'a VideoLibrary,
        from: NaiveDate,
        to: NaiveDate,
    ) -> FetchFuture<'a, VideoLibraryTranscribingDayData> {
        Box::pin(async move {
            let statistics = api_client
                .get_video_library_transcribing_stats(library.id, from, to)
                .await?;

            let transcription_seconds =
                sum_chart_f64_as_u64(&statistics.transcription_seconds_chart);

            Ok(VideoLibraryTranscribingDayData {
                transcription_seconds,
            })
        })
    }

    fn emit_metrics(
        id: &str,
        name: &str,
        last: &VideoLibraryTranscribingDayData,
        current: &VideoLibraryTranscribingDayData,
    ) {
        let labels = [("library_id", id.to_string()), ("name", name.to_string())];

        counter!("bunnynet.video_library_transcribing.seconds", &labels)
            .absolute(last.transcription_seconds + current.transcription_seconds);
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct VideoLibraryTranscribingDayData {
    pub transcription_seconds: u64,
}

impl DayData for VideoLibraryTranscribingDayData {
    fn accumulate(&mut self, day: Self) {
        self.transcription_seconds += day.transcription_seconds;
    }

    fn merge_latest(&mut self, snapshot: Self) {
        self.transcription_seconds = self
            .transcription_seconds
            .max(snapshot.transcription_seconds);
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
        let mut state = VideoLibraryTranscribingDayData {
            transcription_seconds: 10,
        };
        state.accumulate(VideoLibraryTranscribingDayData {
            transcription_seconds: 3,
        });
        assert_eq!(state.transcription_seconds, 13);
    }

    #[test]
    fn merge_latest_takes_max() {
        let mut state = VideoLibraryTranscribingDayData {
            transcription_seconds: 50,
        };
        state.merge_latest(VideoLibraryTranscribingDayData {
            transcription_seconds: 30,
        });
        assert_eq!(state.transcription_seconds, 50);
    }
}
