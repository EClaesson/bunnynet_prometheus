use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;
use serde::de::DeserializeOwned;

use crate::application::ApplicationStatsState;
use crate::dns_zone::DnsZoneStatsState;
use crate::edge_script::EdgeScriptStatsState;
use crate::pull_zone::PullZoneStatsState;
use crate::pull_zone_optimizer::PullZoneOptimizerStatsState;
use crate::pull_zone_origin_shield_queue::PullZoneOriginShieldQueueStatsState;
use crate::pull_zone_safehop::PullZoneSafeHopStatsState;
use crate::shield_zone::ShieldZoneStatsState;
use crate::state::{State, read_state_from_file, write_state_to_file};
use crate::storage_zone::StorageZoneStatsState;
use crate::video_library::VideoLibraryStatsState;
use crate::video_library_drm::VideoLibraryDrmStatsState;
use crate::video_library_transcribing::VideoLibraryTranscribingStatsState;

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum Collector {
    Application,
    DnsZone,
    EdgeScript,
    StorageZone,
    VideoLibrary,
    VideoLibraryTranscribing,
    VideoLibraryDrm,
    PullZone,
    PullZoneOptimizer,
    PullZoneOriginShieldQueue,
    PullZoneSafehop,
    ShieldZone,
}

impl Collector {
    pub fn state_file_name(self) -> String {
        let name = self.name();
        format!("{name}.json")
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::DnsZone => "dns_zone",
            Self::EdgeScript => "edge_script",
            Self::StorageZone => "storage_zone",
            Self::VideoLibrary => "video_library",
            Self::VideoLibraryTranscribing => "video_library_transcribing",
            Self::VideoLibraryDrm => "video_library_drm",
            Self::PullZone => "pull_zone",
            Self::PullZoneOptimizer => "pull_zone_optimizer",
            Self::PullZoneOriginShieldQueue => "pull_zone_origin_shield_queue",
            Self::PullZoneSafehop => "pull_zone_safehop",
            Self::ShieldZone => "shield_zone",
        }
    }

    pub fn load_state(self, state_dir: &Path) -> Result<Box<dyn State>> {
        let file = self.state_file_name();
        match self {
            Self::Application => read::<ApplicationStatsState>(state_dir, &file),
            Self::DnsZone => read::<DnsZoneStatsState>(state_dir, &file),
            Self::EdgeScript => read::<EdgeScriptStatsState>(state_dir, &file),
            Self::StorageZone => read::<StorageZoneStatsState>(state_dir, &file),
            Self::VideoLibrary => read::<VideoLibraryStatsState>(state_dir, &file),
            Self::VideoLibraryTranscribing => {
                read::<VideoLibraryTranscribingStatsState>(state_dir, &file)
            }
            Self::VideoLibraryDrm => read::<VideoLibraryDrmStatsState>(state_dir, &file),
            Self::PullZone => read::<PullZoneStatsState>(state_dir, &file),
            Self::PullZoneOptimizer => read::<PullZoneOptimizerStatsState>(state_dir, &file),
            Self::PullZoneOriginShieldQueue => {
                read::<PullZoneOriginShieldQueueStatsState>(state_dir, &file)
            }
            Self::PullZoneSafehop => read::<PullZoneSafeHopStatsState>(state_dir, &file),
            Self::ShieldZone => read::<ShieldZoneStatsState>(state_dir, &file),
        }
    }

    pub fn save_state(self, state: &dyn State, state_dir: &Path) -> Result<()> {
        write_state_to_file(state_dir, &self.state_file_name(), &state.serialize()?)
    }
}

fn read<T>(state_dir: &Path, file: &str) -> Result<Box<dyn State>>
where
    T: State + Default + DeserializeOwned + 'static,
{
    Ok(Box::new(read_state_from_file::<T>(state_dir, file)?))
}
