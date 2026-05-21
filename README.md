# bunnynet_prometheus

Expose Bunny.net statistics as a scrapable prometheus endpoint.

_bunnynet_prometheus_ is a daemon that polls the Bunny.net API for statistics and exposes them on a HTTP endpoint in prometheus format.

## Installation

TODO!

## Usage

TODO!

## Collectors

### dns_zone

| Name                                | Type    | Tags                  | Description |
| ----------------------------------- | ------- | --------------------- | ----------- |
| _bunnynet.dns_zone.normal_queries_  | Counter | zone_id, domain       |             |
| _bunnynet.dns_zone.smart_queries_   | Counter | zone_id, domain       |             |
| _bunnynet.dns_zone.queries_by_type_ | Counter | zone_id, domain, type |             |

### storage_zone

| Name                                 | Type    | Tags          | Description |
| ------------------------------------ | ------- | ------------- | ----------- |
| _bunnynet.storage_zone.storage_used_ | Counter | zone_id, name |             |
| _bunnynet.storage_zone.file_count_   | Counter | zone_id, name |             |

### video_library

| Name                                          | Type    | Tags                       | Description |
| --------------------------------------------- | ------- | -------------------------- | ----------- |
| _bunnynet.video_library.views_                | Counter | library_id, name           |             |
| _bunnynet.video_library.watch_time_           | Counter | library_id, name           |             |
| _bunnynet.video_library.country_views_        | Counter | library_id, name, country  |             |
| _bunnynet.video_library.country_watch_time_   | Counter | library_id, name, country  |             |

### video_library_transcribing

| Name                                          | Type             | Tags | Description |
| --------------------------------------------- | ---------------- | ---- | ----------- |
| _bunnynet.video_library_transcribing.seconds_ | library_id, name |      |

### video_library_drm

| Name                                         | Type    | Tags             | Description |
| -------------------------------------------- | ------- | ---------------- | ----------- |
| _bunnynet.video_library_drm.licenses_issued_ | Counter | library_id, name |             |

### pull_zone

| Name                                                          | Type    | Tags                  | Description |
| ------------------------------------------------------------- | ------- | --------------------- | ----------- |
| _bunnynet.pull_zone.bandwidth_used_                           | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.bandwidth_cached_                         | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.requests_served_                          | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.pull_requests_pulled_                     | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.origin_shield_bandwidth_used_             | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.origin_shield_internal_bandwidth_used_    | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.origin_traffic_                           | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.errors_3xx_                               | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.errors_4xx_                               | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.errors_5xx_                               | Counter | zone_id, name         |             |
| _bunnynet.pull_zone.origin_response_time_                     | Gauge   | zone_id, name         |             |
| _bunnynet.pull_zone.cache_hit_rate_                           | Gauge   | zone_id, name         |             |
| _bunnynet.pull_zone.geo_traffic_                              | Counter | zone_id, name, region |             |

### pull_zone_optimizer

| Name                                                   | Type    | Tags          | Description |
| ------------------------------------------------------ | ------- | ------------- | ----------- |
| _bunnynet.pull_zone_optimizer.requests_optimized_      | Counter | zone_id, name |             |
| _bunnynet.pull_zone_optimizer.traffic_saved_           | Counter | zone_id, name |             |
| _bunnynet.pull_zone_optimizer.average_compression_     | Gauge   | zone_id, name |             |
| _bunnynet.pull_zone_optimizer.average_processing_time_ | Gauge   | zone_id, name |             |

### pull_zone_origin_shield_queue

| Name                                                         | Type  | Tags          | Description |
| ------------------------------------------------------------ | ----- | ------------- | ----------- |
| _bunnynet.pull_zone_origin_shield_queue.concurrent_requests_ | Gauge | zone_id, name |             |
| _bunnynet.pull_zone_origin_shield_queue.queued_requests_     | Gauge | zone_id, name |             |

### pull_zone_safehop

| Name                                          | Type    | Tags          | Description |
| --------------------------------------------- | ------- | ------------- | ----------- |
| _bunnynet.pull_zone_safehop.requests_retried_ | Counter | zone_id, name |             |
| _bunnynet.pull_zone_safehop.requests_saved_   | Counter | zone_id, name |             |
