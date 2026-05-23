use std::sync::Arc;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, VideoLibrary};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
};

pub type VideoLibraryTranscribingStatsState = EntityStatsState<VideoLibraryTranscribingKind>;

const TRANSCRIPTION_SECONDS: &str = "transcription_seconds";

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

    fn list(client: &ApiClient) -> FetchFuture<'_, Arc<Vec<VideoLibrary>>> {
        Box::pin(async move { client.list_video_libraries().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        library: &'a VideoLibrary,
        date: NaiveDate,
    ) -> FetchFuture<'a, VideoLibraryTranscribingDayData> {
        Box::pin(async move {
            let stats = client
                .get_video_library_transcribing_stats(library.id, date, date)
                .await?;

            let transcription_seconds = f64_to_u64(
                find_chart_value_for_date(&stats.transcription_seconds_chart, date)
                    .context(TRANSCRIPTION_SECONDS)?,
            );

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

    fn merge_latest(&mut self, snap: Self) {
        self.transcription_seconds = self.transcription_seconds.max(snap.transcription_seconds);
    }
}
