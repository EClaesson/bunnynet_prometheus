# bunnynet_prometheus

Expose [Bunny.net](https://bunny.net) statistics as a scrapable prometheus endpoint.

_bunnynet_prometheus_ is a daemon that periodically polls the [Bunny.net API](https://docs.bunny.net/api-reference) for usage and performance statistics and re-exposes it as Prometheus metrics on a HTTP endpoint.

All available statistics endpoints in the Bunny.net API are covered. Magic container applications, DNS zones, Edge scripts, Storage zones, Video libraries, Video transcribing, Video DRM, Pull zones, Pull zone optimizers, Pull zone origin shields, Pull zone safehops and Shield zones.
Each category of statistics are implemented as separate collectors that can be individually enabled. Details about available collectors and the metrics they emit can be found at [Collectors](#collectors).

Counter values are persisted to disk between the poll cycles, so restarts doesn't reset totals and day rollovers are correctly handled. If the daemon has been stopped a longer time (max 30 days), missed days will be backfilled.

## Installation

_bunnynet_prometheus_ can be compiled with cargo or you can download prebuilt [releases](https://github.com/EClaesson/bunnynet_prometheus/releases).

It is also available on crates.io.

```
cargo install bunnynet_prometheus
```

## Usage

```
bunnynet_prometheus [OPTIONS] --collectors <COLLECTORS>...

Options:
  -v, --verbose
          Enable verbose output
  -q, --quiet
          Only output warnings and errors
  -k, --access-key <ACCESS_KEY>
          Bunny.net API access key (Can also be set by environment variable BUNNYNET_ACCESS_KEY)
  -f, --access-key-file <ACCESS_KEY_FILE>
          Path to a file containing a Bunny.net API access key
  -r, --api-request-timeout <API_REQUEST_TIMEOUT>
          Timeout in seconds for Bunny.net API requests [default: 10]
  -i, --poll-interval <POLL_INTERVAL>
          Update interval in seconds [default: 300]
  -s, --state-dir <STATE_DIR>
          Path to a directory to store persistent state files in [default: ~/.local/share/bunnynet_prometheus/state]
  -a, --bind-addr <BIND_ADDR>
          HTTP server bind address [default: 0.0.0.0]
  -p, --bind-port <BIND_PORT>
          HTTP server bind port [default: 9000]
  -c, --collectors <COLLECTORS>...
          Comma-separated list of categories of statistics to poll [possible values: application, dns_zone, edge_script, storage_zone, video_library, video_library_transcribing, video_library_drm, pull_zone, pull_zone_optimizer, pull_zone_origin_shield_queue, pull_zone_safehop, shield_zone]
  -h, --help
          Print help
  -V, --version
          Print version
```

You can create your API access key in the [Bunny.net dashboard](https://dash.bunny.net/account/api-key).

The HTTP endpoint will respond on any path.

### Example

Load access key from file and poll for dns_zone and storage_zone statistics and expose on default port (9000).

```
bunnynet_prometheus -f ~/.bunnynet.key -c dns_zone,storage_zone
```

## Internal metrics

These metrics are always enabled and expose the health and age of the collectors.

| Name                                                | Type  | Tags      |
| --------------------------------------------------- | ----- | --------- |
| _bunnynet_last_update_attempt_timestamp_seconds_    | Gauge |           |
| _bunnynet_last_successful_update_timestamp_seconds_ | Gauge |           |
| _bunnynet_last_collector_update_timestamp_seconds_  | Gauge | collector |

## Collectors

### application

| Name                                   | Type    | Tags                 |
| -------------------------------------- | ------- | -------------------- |
| _bunnynet_application_target_latency_  | Gauge   | app_id, name         |
| _bunnynet_application_active_regions_  | Gauge   | app_id, name         |
| _bunnynet_application_latency_         | Gauge   | app_id, name         |
| _bunnynet_application_instances_       | Gauge   | app_id, name         |
| _bunnynet_application_cpu_usage_       | Gauge   | app_id, name         |
| _bunnynet_application_ram_usage_       | Gauge   | app_id, name         |
| _bunnynet_application_traffic_         | Counter | app_id, name         |
| _bunnynet_application_volume_usage_    | Gauge   | app_id, name, volume |
| _bunnynet_application_volume_capacity_ | Gauge   | app_id, name, volume |

### dns_zone

| Name                                | Type    | Tags                  |
| ----------------------------------- | ------- | --------------------- |
| _bunnynet_dns_zone_normal_queries_  | Counter | zone_id, domain       |
| _bunnynet_dns_zone_smart_queries_   | Counter | zone_id, domain       |
| _bunnynet_dns_zone_queries_by_type_ | Counter | zone_id, domain, type |

### edge_script

| Name                                    | Type    | Tags            |
| --------------------------------------- | ------- | --------------- |
| _bunnynet_edge_script_requests_served_  | Counter | script_id, name |
| _bunnynet_edge_script_cpu_time_         | Counter | script_id, name |
| _bunnynet_edge_script_average_cpu_time_ | Gauge   | script_id, name |

### storage_zone

| Name                                 | Type    | Tags          |
| ------------------------------------ | ------- | ------------- |
| _bunnynet_storage_zone_storage_used_ | Counter | zone_id, name |
| _bunnynet_storage_zone_file_count_   | Counter | zone_id, name |

### video_library

| Name                                        | Type    | Tags                      |
| ------------------------------------------- | ------- | ------------------------- |
| _bunnynet_video_library_views_              | Counter | library_id, name          |
| _bunnynet_video_library_watch_time_         | Counter | library_id, name          |
| _bunnynet_video_library_country_views_      | Counter | library_id, name, country |
| _bunnynet_video_library_country_watch_time_ | Counter | library_id, name, country |

### video_library_drm

| Name                                         | Type    | Tags             |
| -------------------------------------------- | ------- | ---------------- |
| _bunnynet_video_library_drm_licenses_issued_ | Counter | library_id, name |

### video_library_transcribing

| Name                                          | Type    | Tags             |
| --------------------------------------------- | ------- | ---------------- |
| _bunnynet_video_library_transcribing_seconds_ | Counter | library_id, name |

### pull_zone

| Name                                                       | Type    | Tags                  |
| ---------------------------------------------------------- | ------- | --------------------- |
| _bunnynet_pull_zone_bandwidth_used_                        | Counter | zone_id, name         |
| _bunnynet_pull_zone_bandwidth_cached_                      | Counter | zone_id, name         |
| _bunnynet_pull_zone_requests_served_                       | Counter | zone_id, name         |
| _bunnynet_pull_zone_pull_requests_pulled_                  | Counter | zone_id, name         |
| _bunnynet_pull_zone_origin_shield_bandwidth_used_          | Counter | zone_id, name         |
| _bunnynet_pull_zone_origin_shield_internal_bandwidth_used_ | Counter | zone_id, name         |
| _bunnynet_pull_zone_origin_traffic_                        | Counter | zone_id, name         |
| _bunnynet_pull_zone_errors_3xx_                            | Counter | zone_id, name         |
| _bunnynet_pull_zone_errors_4xx_                            | Counter | zone_id, name         |
| _bunnynet_pull_zone_errors_5xx_                            | Counter | zone_id, name         |
| _bunnynet_pull_zone_origin_response_time_                  | Gauge   | zone_id, name         |
| _bunnynet_pull_zone_cache_hit_rate_                        | Gauge   | zone_id, name         |
| _bunnynet_pull_zone_geo_traffic_                           | Counter | zone_id, name, region |

### pull_zone_optimizer

| Name                                                   | Type    | Tags          |
| ------------------------------------------------------ | ------- | ------------- |
| _bunnynet_pull_zone_optimizer_requests_optimized_      | Counter | zone_id, name |
| _bunnynet_pull_zone_optimizer_traffic_saved_           | Counter | zone_id, name |
| _bunnynet_pull_zone_optimizer_average_compression_     | Gauge   | zone_id, name |
| _bunnynet_pull_zone_optimizer_average_processing_time_ | Gauge   | zone_id, name |

### pull_zone_origin_shield_queue

| Name                                                         | Type  | Tags          |
| ------------------------------------------------------------ | ----- | ------------- |
| _bunnynet_pull_zone_origin_shield_queue_concurrent_requests_ | Gauge | zone_id, name |
| _bunnynet_pull_zone_origin_shield_queue_queued_requests_     | Gauge | zone_id, name |

### pull_zone_safehop

| Name                                          | Type    | Tags          |
| --------------------------------------------- | ------- | ------------- |
| _bunnynet_pull_zone_safehop_requests_retried_ | Counter | zone_id, name |
| _bunnynet_pull_zone_safehop_requests_saved_   | Counter | zone_id, name |

### shield_zone

| Name                                                | Type    | Tags                                           |
| --------------------------------------------------- | ------- | ---------------------------------------------- |
| _bunnynet_shield_zone_requests_                     | Counter | shield_zone_id, pull_zone_id, category, action |
| _bunnynet_shield_zone_clean_requests_limit_         | Gauge   | shield_zone_id, pull_zone_id                   |
| _bunnynet_shield_zone_billable_requests_this_month_ | Gauge   | shield_zone_id, pull_zone_id                   |
