use std::path::Path;

use anyhow::Result;
use clap::ValueEnum;

use crate::dns_zone::DnsZoneStatsState;
use crate::state::{State, read_state_from_file, write_state_to_file};
use crate::storage_zone::StorageZoneStatsState;

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum Target {
    DnsZone,
    StorageZone,
}

impl Target {
    pub fn state_file_name(self) -> String {
        let name = self.name();
        format!("{name}.json")
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::DnsZone => "dns_zone",
            Self::StorageZone => "storage_zone",
        }
    }

    pub fn load_state(self, state_dir: &Path) -> Result<Box<dyn State>> {
        match self {
            Self::DnsZone => Ok(Box::new(read_state_from_file::<DnsZoneStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
            Self::StorageZone => Ok(Box::new(read_state_from_file::<StorageZoneStatsState>(
                state_dir,
                &self.state_file_name(),
            )?)),
        }
    }

    pub fn save_state(self, state: &dyn State, state_dir: &Path) -> Result<()> {
        write_state_to_file(state_dir, &self.state_file_name(), &state.serialize()?)
    }
}
