use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
    bunny::{ApiClient, DnsZone, QueriesByTypeChart},
    state::{PollFuture, State},
};

type DnsZoneStateMap = HashMap<u64, DnsZoneState>;

#[derive(Serialize, Deserialize, Default)]
pub struct DnsZoneStatsState {
    zones: DnsZoneStateMap,
}

impl DnsZoneStatsState {
    async fn backfill_until(
        state: &mut DnsZoneState,
        zone: &DnsZone,
        until_date: NaiveDate,
        client: &ApiClient,
    ) -> Result<()> {
        if let Some(last_complete_day) = state.last_complete_day {
            let mut date_cursor = last_complete_day + chrono::TimeDelta::days(1);
            while date_cursor <= until_date {
                debug!(date = %date_cursor, zone = %zone, "Backfilling DNS zone stats");
                let day_data = Self::get_single_day_state(zone, date_cursor, client).await?;

                state.last_complete_day_state.normal_queries_served +=
                    day_data.normal_queries_served;
                state.last_complete_day_state.smart_queries_served += day_data.smart_queries_served;

                let queries_by_type = &mut state.last_complete_day_state.queries_served_per_type;
                for (type_str, value) in &day_data.queries_served_per_type {
                    *queries_by_type.entry(type_str.clone()).or_default() += value;
                }

                state.last_complete_day = Some(date_cursor);
                date_cursor += chrono::TimeDelta::days(1);
            }
        }

        state.domain.clone_from(&zone.domain);

        Ok(())
    }

    async fn refresh_current_day(
        state: &mut DnsZoneState,
        zone: &DnsZone,
        today_date: NaiveDate,
        client: &ApiClient,
    ) -> Result<()> {
        debug!(date = %today_date, zone = %zone, "Refreshing DNS zone stats for current day");
        let new_state = Self::get_single_day_state(zone, today_date, client).await?;

        state.current_day_state.normal_queries_served = state
            .current_day_state
            .normal_queries_served
            .max(new_state.normal_queries_served);
        state.current_day_state.smart_queries_served = state
            .current_day_state
            .smart_queries_served
            .max(new_state.smart_queries_served);

        let queries_by_type = &mut state.current_day_state.queries_served_per_type;
        for (type_str, value) in &new_state.queries_served_per_type {
            let entry = queries_by_type.entry(type_str.clone()).or_default();
            *entry = (*entry).max(*value);
        }

        state.domain.clone_from(&zone.domain);

        Ok(())
    }

    async fn get_single_day_state(
        zone: &DnsZone,
        date: NaiveDate,
        client: &ApiClient,
    ) -> Result<StateData> {
        let mut state = StateData::default();
        let date_str = date.format("%Y-%m-%d").to_string();

        let zone_stats = client.get_dns_zone_stats(zone.id, date, date).await?;

        state.normal_queries_served = f64_to_u64(
            find_chart_value_for_date(&zone_stats.normal_queries_served_chart, &date_str)
                .context("Normal queries served")?,
        );

        state.smart_queries_served = f64_to_u64(
            find_chart_value_for_date(&zone_stats.smart_queries_served_chart, &date_str)
                .context("Smart queries served")?,
        );

        let mut queries_by_type = QueriesByTypeChart::new();

        for (type_num, value) in &zone_stats.queries_by_type_chart {
            let type_str = get_dns_type_name(type_num);
            queries_by_type.insert(type_str.to_string(), *value);
        }

        state.queries_served_per_type = queries_by_type;

        Ok(state)
    }

    fn update_metrics(&self) {
        for (zone_id, state) in &self.zones {
            state.emit_metrics(*zone_id);
        }
    }
}

impl State for DnsZoneStatsState {
    fn poll<'a>(&'a mut self, client: &'a ApiClient) -> PollFuture<'a> {
        Box::pin(async move {
            debug!("Polling DNS zone stats");
            let today = chrono::Utc::now().date_naive();
            let yesterday = today - chrono::Days::new(1);

            let zones = client.list_dns_zones().await?;
            for zone in &zones {
                let state = self.zones.entry(zone.id).or_default();

                if let Some(last_complete_day) = state.last_complete_day
                    && last_complete_day < yesterday
                {
                    Self::backfill_until(state, zone, yesterday, client).await?;
                    state.current_day_state = StateData::default();
                } else {
                    state.last_complete_day = Some(yesterday);
                }

                Self::refresh_current_day(state, zone, today, client).await?;
            }

            self.update_metrics();

            Ok(())
        })
    }

    fn serialize(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Serialize, Deserialize, Default)]
struct DnsZoneState {
    domain: String,
    last_complete_day: Option<chrono::NaiveDate>,
    last_complete_day_state: StateData,
    current_day_state: StateData,
}

impl DnsZoneState {
    fn emit_metrics(&self, zone_id: u64) {
        let zone_id_str = zone_id.to_string();
        let last = &self.last_complete_day_state;
        let current = &self.current_day_state;
        let labels = [
            ("zone_id", zone_id_str.clone()),
            ("domain", self.domain.clone()),
        ];

        counter!("bunnynet.dns_zone.normal_queries", &labels)
            .absolute(last.normal_queries_served + current.normal_queries_served);
        counter!("bunnynet.dns_zone.smart_queries", &labels)
            .absolute(last.smart_queries_served + current.smart_queries_served);

        let unique_types: HashSet<&String> = last
            .queries_served_per_type
            .keys()
            .chain(current.queries_served_per_type.keys())
            .collect();

        for type_str in unique_types {
            let total = last
                .queries_served_per_type
                .get(type_str)
                .copied()
                .unwrap_or(0)
                + current
                    .queries_served_per_type
                    .get(type_str)
                    .copied()
                    .unwrap_or(0);

            counter!(
                "bunnynet.dns_zone.queries_by_type",
                "zone_id" => zone_id_str.clone(),
                "domain" => self.domain.clone(),
                "type" => type_str.clone(),
            )
            .absolute(total);
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct StateData {
    pub normal_queries_served: u64,
    pub smart_queries_served: u64,
    pub queries_served_per_type: QueriesByTypeChart,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
const fn f64_to_u64(v: f64) -> u64 {
    if v < 0.0 { 0 } else { v as u64 }
}

fn find_chart_value_for_date<V: Copy>(chart: &HashMap<String, V>, date_prefix: &str) -> Result<V> {
    let mut iter = chart.iter();
    match (iter.next(), iter.next()) {
        (Some((key, value)), None) if key.starts_with(date_prefix) => Ok(*value),
        _ => bail!(
            "Expected exactly one entry starting with {date_prefix}, got {} entries",
            chart.len()
        ),
    }
}

// DNS resource record types
// https://www.iana.org/assignments/dns-parameters/dns-parameters.xhtml#dns-parameters-4
#[allow(clippy::too_many_lines)]
fn get_dns_type_name(type_num: &str) -> &str {
    match type_num {
        "1" => "A",
        "2" => "NS",
        "3" => "MD",
        "4" => "MF",
        "5" => "CNAME",
        "6" => "SOA",
        "7" => "MB",
        "8" => "MG",
        "9" => "MR",
        "10" => "NULL",
        "11" => "WKS",
        "12" => "PTR",
        "13" => "HINFO",
        "14" => "MINFO",
        "15" => "MX",
        "16" => "TXT",
        "17" => "RP",
        "18" => "AFSDB",
        "19" => "X25",
        "20" => "ISDN",
        "21" => "RT",
        "22" => "NSAP",
        "23" => "NSAP-PTR",
        "24" => "SIG",
        "25" => "KEY",
        "26" => "PX",
        "27" => "GPOS",
        "28" => "AAAA",
        "29" => "LOC",
        "30" => "NXT",
        "31" => "EID",
        "32" => "NIMLOC",
        "33" => "SRV",
        "34" => "ATMA",
        "35" => "NAPTR",
        "36" => "KX",
        "37" => "CERT",
        "38" => "A6",
        "39" => "DNAME",
        "40" => "SINK",
        "41" => "OPT",
        "42" => "APL",
        "43" => "DS",
        "44" => "SSHFP",
        "45" => "IPSECKEY",
        "46" => "RRSIG",
        "47" => "NSEC",
        "48" => "DNSKEY",
        "49" => "DHCID",
        "50" => "NSEC3",
        "51" => "NSEC3PARAM",
        "52" => "TLSA",
        "53" => "SMIMEA",
        "54" => "Unassigned",
        "55" => "HIP",
        "56" => "NINFO",
        "57" => "RKEY",
        "58" => "TALINK",
        "59" => "CDS",
        "60" => "CDNSKEY",
        "61" => "OPENPGPKEY",
        "62" => "CSYNC",
        "63" => "ZONEMD",
        "64" => "SVCB",
        "65" => "HTTPS",
        "66" => "DSYNC",
        "67" => "HHIT",
        "68" => "BRID",
        "99" => "SPF",
        "100" => "UINFO",
        "101" => "UID",
        "102" => "GID",
        "103" => "UNSPEC",
        "104" => "NID",
        "105" => "L32",
        "106" => "L64",
        "107" => "LP",
        "108" => "EUI48",
        "109" => "EUI64",
        "128" => "NXNAME",
        "249" => "TKEY",
        "250" => "TSIG",
        "251" => "IXFR",
        "252" => "AXFR",
        "253" => "MAILB",
        "254" => "MAILA",
        "255" => "*",
        "256" => "URI",
        "257" => "CAA",
        "258" => "AVC",
        "259" => "DOA",
        "260" => "AMTRELAY",
        "261" => "RESINFO",
        "262" => "WALLET",
        "263" => "CLA",
        "264" => "IPN",
        "32768" => "TA",
        "32769" => "DLV",
        _ => "UNKNOWN",
    }
}
