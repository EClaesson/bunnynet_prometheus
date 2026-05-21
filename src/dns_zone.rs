use std::collections::HashSet;

use anyhow::Context;
use chrono::NaiveDate;
use metrics::counter;
use serde::{Deserialize, Serialize};

use crate::bunny::{ApiClient, DnsZone, QueriesByTypeChart};
use crate::entity_stats::{
    DayData, EntityStatsState, EntityType, FetchFuture, f64_to_u64, find_chart_value_for_date,
};

pub type DnsZoneStatsState = EntityStatsState<DnsZoneKind>;

const NORMAL_QUERIES_SERVED: &str = "normal_queries_served";
const SMART_QUERIES_SERVED: &str = "smart_queries_served";

pub struct DnsZoneKind;

impl EntityType for DnsZoneKind {
    type Entity = DnsZone;
    type DayData = DnsDayData;

    const LOG_LABEL: &'static str = "DNS zone";

    fn entity_id(entity: &DnsZone) -> String {
        entity.id.to_string()
    }

    fn entity_label(entity: &DnsZone) -> String {
        entity.domain.clone()
    }

    fn list(client: &ApiClient) -> FetchFuture<'_, Vec<DnsZone>> {
        Box::pin(async move { client.list_dns_zones().await })
    }

    fn fetch_day<'a>(
        client: &'a ApiClient,
        zone: &'a DnsZone,
        date: NaiveDate,
    ) -> FetchFuture<'a, DnsDayData> {
        Box::pin(async move {
            let stats = client.get_dns_zone_stats(zone.id, date, date).await?;

            let normal_queries_served = f64_to_u64(
                find_chart_value_for_date(&stats.normal_queries_served_chart, date)
                    .context(NORMAL_QUERIES_SERVED)?,
            );
            let smart_queries_served = f64_to_u64(
                find_chart_value_for_date(&stats.smart_queries_served_chart, date)
                    .context(SMART_QUERIES_SERVED)?,
            );

            let mut queries_served_per_type = QueriesByTypeChart::new();
            for (type_num, value) in &stats.queries_by_type_chart {
                queries_served_per_type.insert(dns_type_name(type_num).to_string(), *value);
            }

            Ok(DnsDayData {
                normal_queries_served,
                smart_queries_served,
                queries_served_per_type,
            })
        })
    }

    fn emit_metrics(id: &str, domain: &str, last: &DnsDayData, current: &DnsDayData) {
        let labels = [("zone_id", id.to_string()), ("domain", domain.to_string())];

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
                "zone_id" => id.to_string(),
                "domain" => domain.to_string(),
                "type" => type_str.clone(),
            )
            .absolute(total);
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct DnsDayData {
    pub normal_queries_served: u64,
    pub smart_queries_served: u64,
    pub queries_served_per_type: QueriesByTypeChart,
}

impl DayData for DnsDayData {
    fn accumulate(&mut self, day: Self) {
        self.normal_queries_served += day.normal_queries_served;
        self.smart_queries_served += day.smart_queries_served;
        for (type_str, value) in day.queries_served_per_type {
            *self.queries_served_per_type.entry(type_str).or_default() += value;
        }
    }

    fn merge_latest(&mut self, snap: Self) {
        self.normal_queries_served = self.normal_queries_served.max(snap.normal_queries_served);
        self.smart_queries_served = self.smart_queries_served.max(snap.smart_queries_served);
        for (type_str, value) in snap.queries_served_per_type {
            let entry = self.queries_served_per_type.entry(type_str).or_default();
            *entry = (*entry).max(value);
        }
    }
}

// DNS resource record types
// https://www.iana.org/assignments/dns-parameters/dns-parameters.xhtml#dns-parameters-4
#[allow(clippy::too_many_lines)]
fn dns_type_name(type_num: &str) -> &str {
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
