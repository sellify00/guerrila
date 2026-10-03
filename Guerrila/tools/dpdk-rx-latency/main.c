#define _GNU_SOURCE 1

#include <arpa/inet.h>
#include <errno.h>
#include <getopt.h>
#include <inttypes.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <rte_branch_prediction.h>
#include <rte_byteorder.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_errno.h>
#include <rte_ethdev.h>
#include <rte_ether.h>
#include <rte_ip.h>
#include <rte_mbuf.h>
#include <rte_mbuf_dyn.h>
#include <rte_mempool.h>
#include <rte_udp.h>

#define NS_PER_SEC 1000000000ULL
#define HIST_NS_MAX 1000000U
#define HIST_BUCKETS (HIST_NS_MAX + 2U)
#define HIST_OVERFLOW_BUCKET (HIST_BUCKETS - 1U)

#define DEFAULT_DURATION_SECONDS 120U
#define DEFAULT_PORT_ID 0U
#define DEFAULT_QUEUE_ID 0U
#define DEFAULT_UDP_MIN 8000U
#define DEFAULT_UDP_MAX 8025U
#define DEFAULT_BURST_SIZE 64U
#define DEFAULT_RX_DESC 2048U
#define DEFAULT_TX_DESC 512U
#define DEFAULT_MBUF_COUNT 32768U
#define DEFAULT_MEMPOOL_CACHE 256U
#define DEFAULT_CALIBRATION_MS 100U

static volatile sig_atomic_t g_stop_requested;

struct histogram {
    uint64_t *buckets;
    uint64_t total;
    uint64_t max_ns;
};

struct options {
    uint16_t port_id;
    uint16_t queue_id;
    uint16_t udp_min;
    uint16_t udp_max;
    uint16_t burst_size;
    uint16_t rx_desc;
    uint16_t tx_desc;
    uint32_t mbuf_count;
    uint32_t duration_seconds;
    uint32_t calibration_ms;
    const char *json_path;
};

struct counters {
    uint64_t packets_seen;
    uint64_t bytes_seen;
    uint64_t udp_range_seen;
    uint64_t large_udp_range_seen;
    uint64_t rx_bursts;
    uint64_t rx_empty_polls;
    uint64_t drop_parse_l2;
    uint64_t drop_parse_l3;
    uint64_t drop_parse_l4;
    uint64_t drop_wrong_udp_port;
    uint64_t hw_timestamp_packets;
};

struct port_clock {
    bool field_registered;
    bool offload_configured;
    bool timesync_enabled;
    bool supported;
    bool calibrated;
    int field_offset;
    uint64_t rx_flag;
    uint64_t base_clock;
    uint64_t base_mono_ns;
    double ns_per_tick;
};

enum packet_classify_result {
    PACKET_MATCH = 0,
    PACKET_DROP_PARSE_L2,
    PACKET_DROP_PARSE_L3,
    PACKET_DROP_PARSE_L4,
    PACKET_DROP_WRONG_UDP_PORT,
};

static void
handle_signal(int signum)
{
    (void)signum;
    g_stop_requested = 1;
}

static void
usage(const char *prog)
{
    fprintf(stderr,
        "Usage: %s [DPDK EAL args] -- [options]\n"
        "\n"
        "Options:\n"
        "  --seconds N        Capture duration in seconds (default: %u)\n"
        "  --port N           DPDK port ID (default: %u)\n"
        "  --queue N          RX queue ID (default: %u)\n"
        "  --udp-min N        Minimum UDP port to accept (default: %u)\n"
        "  --udp-max N        Maximum UDP port to accept (default: %u)\n"
        "  --burst N          RX burst size (default: %u)\n"
        "  --rx-desc N        RX descriptor count (default: %u)\n"
        "  --tx-desc N        TX descriptor count (default: %u)\n"
        "  --mbufs N          Mbuf count for the mempool (default: %u)\n"
        "  --json PATH        Output JSON file path (required)\n"
        "  --calibration-ms N Device clock calibration window in ms (default: %u)\n"
        "  --help             Show this help text\n"
        "\n"
        "Example:\n"
        "  %s -l 0-3 -n 4 -a 0000:05:00.0 -- --seconds 120 --json /tmp/dpdk.json\n",
        prog,
        DEFAULT_DURATION_SECONDS,
        DEFAULT_PORT_ID,
        DEFAULT_QUEUE_ID,
        DEFAULT_UDP_MIN,
        DEFAULT_UDP_MAX,
        DEFAULT_BURST_SIZE,
        DEFAULT_RX_DESC,
        DEFAULT_TX_DESC,
        DEFAULT_MBUF_COUNT,
        DEFAULT_CALIBRATION_MS,
        prog);
}

static int
parse_u16(const char *value, uint16_t *out)
{
    char *end = NULL;
    unsigned long parsed = strtoul(value, &end, 10);
    if (value == end || *end != '\0' || parsed > UINT16_MAX) {
        return -1;
    }
    *out = (uint16_t)parsed;
    return 0;
}

static int
parse_u32(const char *value, uint32_t *out)
{
    char *end = NULL;
    unsigned long parsed = strtoul(value, &end, 10);
    if (value == end || *end != '\0' || parsed > UINT32_MAX) {
        return -1;
    }
    *out = (uint32_t)parsed;
    return 0;
}

static int
parse_options(int argc, char **argv, struct options *opts)
{
    static const struct option long_options[] = {
        {"seconds", required_argument, NULL, 's'},
        {"port", required_argument, NULL, 'p'},
        {"queue", required_argument, NULL, 'q'},
        {"udp-min", required_argument, NULL, 'm'},
        {"udp-max", required_argument, NULL, 'M'},
        {"burst", required_argument, NULL, 'b'},
        {"rx-desc", required_argument, NULL, 'r'},
        {"tx-desc", required_argument, NULL, 't'},
        {"mbufs", required_argument, NULL, 'n'},
        {"json", required_argument, NULL, 'j'},
        {"calibration-ms", required_argument, NULL, 'c'},
        {"help", no_argument, NULL, 'h'},
        {NULL, 0, NULL, 0},
    };

    opts->port_id = DEFAULT_PORT_ID;
    opts->queue_id = DEFAULT_QUEUE_ID;
    opts->udp_min = DEFAULT_UDP_MIN;
    opts->udp_max = DEFAULT_UDP_MAX;
    opts->burst_size = DEFAULT_BURST_SIZE;
    opts->rx_desc = DEFAULT_RX_DESC;
    opts->tx_desc = DEFAULT_TX_DESC;
    opts->mbuf_count = DEFAULT_MBUF_COUNT;
    opts->duration_seconds = DEFAULT_DURATION_SECONDS;
    opts->calibration_ms = DEFAULT_CALIBRATION_MS;
    opts->json_path = NULL;

    optind = 1;

    for (;;) {
        int option_index = 0;
        int c = getopt_long(argc, argv, "s:p:q:m:M:b:r:t:n:j:c:h", long_options, &option_index);
        if (c == -1) {
            break;
        }

        switch (c) {
        case 's':
            if (parse_u32(optarg, &opts->duration_seconds) != 0 || opts->duration_seconds == 0) {
                fprintf(stderr, "Invalid --seconds value: %s\n", optarg);
                return -1;
            }
            break;
        case 'p':
            if (parse_u16(optarg, &opts->port_id) != 0) {
                fprintf(stderr, "Invalid --port value: %s\n", optarg);
                return -1;
            }
            break;
        case 'q':
            if (parse_u16(optarg, &opts->queue_id) != 0) {
                fprintf(stderr, "Invalid --queue value: %s\n", optarg);
                return -1;
            }
            break;
        case 'm':
            if (parse_u16(optarg, &opts->udp_min) != 0) {
                fprintf(stderr, "Invalid --udp-min value: %s\n", optarg);
                return -1;
            }
            break;
        case 'M':
            if (parse_u16(optarg, &opts->udp_max) != 0) {
                fprintf(stderr, "Invalid --udp-max value: %s\n", optarg);
                return -1;
            }
            break;
        case 'b':
            if (parse_u16(optarg, &opts->burst_size) != 0 || opts->burst_size == 0) {
                fprintf(stderr, "Invalid --burst value: %s\n", optarg);
                return -1;
            }
            break;
        case 'r':
            if (parse_u16(optarg, &opts->rx_desc) != 0 || opts->rx_desc == 0) {
                fprintf(stderr, "Invalid --rx-desc value: %s\n", optarg);
                return -1;
            }
            break;
        case 't':
            if (parse_u16(optarg, &opts->tx_desc) != 0 || opts->tx_desc == 0) {
                fprintf(stderr, "Invalid --tx-desc value: %s\n", optarg);
                return -1;
            }
            break;
        case 'n':
            if (parse_u32(optarg, &opts->mbuf_count) != 0 || opts->mbuf_count == 0) {
                fprintf(stderr, "Invalid --mbufs value: %s\n", optarg);
                return -1;
            }
            break;
        case 'j':
            opts->json_path = optarg;
            break;
        case 'c':
            if (parse_u32(optarg, &opts->calibration_ms) != 0 || opts->calibration_ms == 0) {
                fprintf(stderr, "Invalid --calibration-ms value: %s\n", optarg);
                return -1;
            }
            break;
        case 'h':
            usage(argv[0]);
            exit(0);
        default:
            usage(argv[0]);
            return -1;
        }
    }

    if (opts->udp_min > opts->udp_max) {
        fprintf(stderr, "UDP port range is invalid: %u > %u\n", opts->udp_min, opts->udp_max);
        return -1;
    }

    if (opts->json_path == NULL) {
        fprintf(stderr, "--json is required\n");
        return -1;
    }

    return 0;
}

static uint64_t
monotonic_ns(void)
{
    struct timespec ts;
#ifdef CLOCK_MONOTONIC_RAW
    const clockid_t clock_id = CLOCK_MONOTONIC_RAW;
#else
    const clockid_t clock_id = CLOCK_MONOTONIC;
#endif
    if (clock_gettime(clock_id, &ts) != 0) {
        return 0;
    }
    return ((uint64_t)ts.tv_sec * NS_PER_SEC) + (uint64_t)ts.tv_nsec;
}

static int
histogram_init(struct histogram *hist)
{
    hist->buckets = calloc(HIST_BUCKETS, sizeof(*hist->buckets));
    if (hist->buckets == NULL) {
        return -1;
    }
    hist->total = 0;
    hist->max_ns = 0;
    return 0;
}

static void
histogram_record(struct histogram *hist, uint64_t ns)
{
    size_t bucket = ns <= HIST_NS_MAX ? (size_t)ns : HIST_OVERFLOW_BUCKET;
    hist->buckets[bucket]++;
    hist->total++;
    if (ns > hist->max_ns) {
        hist->max_ns = ns;
    }
}

static uint64_t
histogram_quantile(const struct histogram *hist, uint64_t numerator, uint64_t denominator)
{
    uint64_t cumulative = 0;
    uint64_t target;

    if (hist->total == 0) {
        return 0;
    }

    if (hist->total == 1) {
        return hist->max_ns;
    }

    target = ((hist->total - 1) * numerator) / denominator;

    for (size_t bucket = 0; bucket < HIST_BUCKETS; bucket++) {
        cumulative += hist->buckets[bucket];
        if (cumulative > target) {
            if (bucket == HIST_OVERFLOW_BUCKET) {
                return hist->max_ns;
            }
            return (uint64_t)bucket;
        }
    }

    return hist->max_ns;
}

static void
histogram_free(struct histogram *hist)
{
    free(hist->buckets);
    hist->buckets = NULL;
}

static const void *
packet_read(const struct rte_mbuf *mbuf, uint32_t offset, uint32_t len, void *scratch)
{
    return rte_pktmbuf_read(mbuf, offset, len, scratch);
}

static enum packet_classify_result
classify_udp_range(const struct rte_mbuf *mbuf, uint16_t udp_min, uint16_t udp_max)
{
    uint32_t offset = 0;
    uint16_t ether_type;
    struct rte_ether_hdr eth_hdr;
    const struct rte_ether_hdr *eth;
    struct rte_vlan_hdr vlan_hdr;
    struct rte_ipv4_hdr ipv4_hdr;
    const struct rte_ipv4_hdr *ipv4;
    struct rte_udp_hdr udp_hdr;
    const struct rte_udp_hdr *udp;

    eth = packet_read(mbuf, offset, sizeof(eth_hdr), &eth_hdr);
    if (eth == NULL) {
        return PACKET_DROP_PARSE_L2;
    }
    offset += sizeof(*eth);
    ether_type = rte_be_to_cpu_16(eth->ether_type);

    for (int i = 0; i < 2; i++) {
        if (ether_type != RTE_ETHER_TYPE_VLAN && ether_type != RTE_ETHER_TYPE_QINQ) {
            break;
        }
        const struct rte_vlan_hdr *vlan = packet_read(mbuf, offset, sizeof(vlan_hdr), &vlan_hdr);
        if (vlan == NULL) {
            return PACKET_DROP_PARSE_L2;
        }
        offset += sizeof(*vlan);
        ether_type = rte_be_to_cpu_16(vlan->eth_proto);
    }

    if (ether_type != RTE_ETHER_TYPE_IPV4) {
        return PACKET_DROP_PARSE_L3;
    }

    ipv4 = packet_read(mbuf, offset, sizeof(ipv4_hdr), &ipv4_hdr);
    if (ipv4 == NULL) {
        return PACKET_DROP_PARSE_L3;
    }
    if ((ipv4->version_ihl >> 4) != 4) {
        return PACKET_DROP_PARSE_L3;
    }
    uint8_t ihl = (uint8_t)((ipv4->version_ihl & 0x0f) * 4);
    if (ihl < sizeof(struct rte_ipv4_hdr)) {
        return PACKET_DROP_PARSE_L3;
    }
    if (ipv4->next_proto_id != IPPROTO_UDP) {
        return PACKET_DROP_PARSE_L4;
    }
    offset += ihl;

    udp = packet_read(mbuf, offset, sizeof(udp_hdr), &udp_hdr);
    if (udp == NULL) {
        return PACKET_DROP_PARSE_L4;
    }

    uint16_t src_port = rte_be_to_cpu_16(udp->src_port);
    uint16_t dst_port = rte_be_to_cpu_16(udp->dst_port);
    if ((src_port >= udp_min && src_port <= udp_max) ||
        (dst_port >= udp_min && dst_port <= udp_max)) {
        return PACKET_MATCH;
    }

    return PACKET_DROP_WRONG_UDP_PORT;
}

static int
calibrate_port_clock(uint16_t port_id, uint32_t calibration_ms, struct port_clock *clock_state)
{
    uint64_t start_clock = 0;
    uint64_t end_clock = 0;
    uint64_t start_ns = 0;
    uint64_t end_ns = 0;
    int rc;

    rc = rte_eth_read_clock(port_id, &start_clock);
    if (rc != 0) {
        return rc;
    }
    start_ns = monotonic_ns();
    usleep(calibration_ms * 1000U);
    rc = rte_eth_read_clock(port_id, &end_clock);
    if (rc != 0) {
        return rc;
    }
    end_ns = monotonic_ns();
    if (end_clock <= start_clock || end_ns <= start_ns) {
        return -EINVAL;
    }

    clock_state->supported = true;
    clock_state->calibrated = true;
    clock_state->base_clock = end_clock;
    clock_state->base_mono_ns = end_ns;
    clock_state->ns_per_tick =
        (double)(end_ns - start_ns) / (double)(end_clock - start_clock);
    return 0;
}

static int
wait_for_link_up(uint16_t port_id, uint32_t timeout_ms, struct rte_eth_link *link)
{
    const useconds_t sleep_us = 100000;
    const uint32_t max_attempts = timeout_ms / 100U;

    for (uint32_t attempt = 0; attempt < max_attempts; attempt++) {
        memset(link, 0, sizeof(*link));
        rte_eth_link_get_nowait(port_id, link);
        if (link->link_status == RTE_ETH_LINK_UP) {
            return 0;
        }
        usleep(sleep_us);
    }

    memset(link, 0, sizeof(*link));
    rte_eth_link_get_nowait(port_id, link);
    return link->link_status == RTE_ETH_LINK_UP ? 0 : -ETIMEDOUT;
}

static int
write_json(FILE *out,
    const struct options *opts,
    const struct counters *counts,
    const struct histogram *rx_hist,
    const struct histogram *hw_hist,
    const struct port_clock *clock_state,
    const struct rte_eth_stats *stats,
    uint64_t duration_ns,
    uint64_t tsc_hz,
    const struct rte_eth_dev_info *dev_info)
{
    double duration_seconds = (double)duration_ns / (double)NS_PER_SEC;
    return fprintf(out,
        "{\n"
        "  \"port_id\": %u,\n"
        "  \"queue_id\": %u,\n"
        "  \"driver_name\": \"%s\",\n"
        "  \"duration_seconds\": %.6f,\n"
        "  \"dpdk_rx_packets_total\": %" PRIu64 ",\n"
        "  \"dpdk_rx_bytes_total\": %" PRIu64 ",\n"
        "  \"dpdk_large_udp_8000_8025_seen\": %" PRIu64 ",\n"
        "  \"dpdk_packets_seen\": %" PRIu64 ",\n"
        "  \"dpdk_udp_8000_8025_seen\": %" PRIu64 ",\n"
        "  \"dpdk_rx_bursts\": %" PRIu64 ",\n"
        "  \"dpdk_rx_empty_polls\": %" PRIu64 ",\n"
        "  \"dpdk_drop_parse_l2\": %" PRIu64 ",\n"
        "  \"dpdk_drop_parse_l3\": %" PRIu64 ",\n"
        "  \"dpdk_drop_parse_l4\": %" PRIu64 ",\n"
        "  \"dpdk_drop_wrong_udp_port\": %" PRIu64 ",\n"
        "  \"dpdk_rx_missed_errors\": %" PRIu64 ",\n"
        "  \"dpdk_rx_errors\": %" PRIu64 ",\n"
        "  \"dpdk_rx_nombuf\": %" PRIu64 ",\n"
        "  \"dpdk_rx_to_classify_p50_ns\": %" PRIu64 ",\n"
        "  \"dpdk_rx_to_classify_p95_ns\": %" PRIu64 ",\n"
        "  \"dpdk_rx_to_classify_p99_ns\": %" PRIu64 ",\n"
        "  \"dpdk_rx_to_classify_p999_ns\": %" PRIu64 ",\n"
        "  \"dpdk_rx_to_classify_max_ns\": %" PRIu64 ",\n"
        "  \"dpdk_hw_rx_timestamp_supported\": %s,\n"
        "  \"dpdk_hw_rx_timestamp_packets\": %" PRIu64 ",\n"
        "  \"dpdk_hw_rx_to_classify_p50_ns\": %" PRIu64 ",\n"
        "  \"dpdk_hw_rx_to_classify_p95_ns\": %" PRIu64 ",\n"
        "  \"dpdk_hw_rx_to_classify_p99_ns\": %" PRIu64 ",\n"
        "  \"dpdk_hw_rx_to_classify_p999_ns\": %" PRIu64 ",\n"
        "  \"dpdk_hw_rx_to_classify_max_ns\": %" PRIu64 ",\n"
        "  \"dpdk_clock_calibrated\": %s,\n"
        "  \"dpdk_tsc_hz\": %" PRIu64 "\n"
        "}\n",
        opts->port_id,
        opts->queue_id,
        dev_info->driver_name != NULL ? dev_info->driver_name : "unknown",
        duration_seconds,
        counts->packets_seen,
        counts->bytes_seen,
        counts->large_udp_range_seen,
        counts->packets_seen,
        counts->udp_range_seen,
        counts->rx_bursts,
        counts->rx_empty_polls,
        counts->drop_parse_l2,
        counts->drop_parse_l3,
        counts->drop_parse_l4,
        counts->drop_wrong_udp_port,
        stats->imissed,
        stats->ierrors,
        stats->rx_nombuf,
        histogram_quantile(rx_hist, 50, 100),
        histogram_quantile(rx_hist, 95, 100),
        histogram_quantile(rx_hist, 99, 100),
        histogram_quantile(rx_hist, 999, 1000),
        rx_hist->max_ns,
        clock_state->supported ? "true" : "false",
        counts->hw_timestamp_packets,
        histogram_quantile(hw_hist, 50, 100),
        histogram_quantile(hw_hist, 95, 100),
        histogram_quantile(hw_hist, 99, 100),
        histogram_quantile(hw_hist, 999, 1000),
        hw_hist->max_ns,
        clock_state->calibrated ? "true" : "false",
        tsc_hz);
}

int
main(int argc, char **argv)
{
    struct options opts;
    struct counters counts = {0};
    struct histogram rx_hist = {0};
    struct histogram hw_hist = {0};
    struct port_clock clock_state = {
        .field_registered = false,
        .offload_configured = false,
        .timesync_enabled = false,
        .supported = false,
        .calibrated = false,
        .field_offset = -1,
        .rx_flag = 0,
        .base_clock = 0,
        .base_mono_ns = 0,
        .ns_per_tick = 0.0,
    };
    struct rte_eth_conf port_conf;
    struct rte_eth_dev_info dev_info;
    struct rte_eth_rxconf rx_conf;
    struct rte_eth_txconf tx_conf;
    struct rte_eth_link link;
    struct rte_eth_stats stats;
    struct rte_mempool *pool = NULL;
    struct rte_mbuf **burst = NULL;
    uint16_t nb_rx = 0;
    uint64_t tsc_hz = 0;
    uint64_t start_ns = 0;
    uint64_t deadline_ns = 0;
    uint64_t end_ns = 0;
    uint16_t port_id;
    int ret;
    FILE *json_file = NULL;

    memset(&port_conf, 0, sizeof(port_conf));
    memset(&dev_info, 0, sizeof(dev_info));
    memset(&rx_conf, 0, sizeof(rx_conf));
    memset(&tx_conf, 0, sizeof(tx_conf));
    memset(&link, 0, sizeof(link));
    memset(&stats, 0, sizeof(stats));

    signal(SIGINT, handle_signal);
    signal(SIGTERM, handle_signal);

    ret = rte_eal_init(argc, argv);
    if (ret < 0) {
        rte_panic("rte_eal_init failed: %s\n", rte_strerror(rte_errno));
    }

    argc -= ret;
    argv += ret;

    if (parse_options(argc, argv, &opts) != 0) {
        usage(argv[0]);
        return 1;
    }

    port_id = opts.port_id;
    if (!rte_eth_dev_is_valid_port(port_id)) {
        fprintf(stderr, "DPDK port %u is invalid\n", port_id);
        return 1;
    }

    if (histogram_init(&rx_hist) != 0 || histogram_init(&hw_hist) != 0) {
        fprintf(stderr, "Failed to allocate histograms\n");
        return 1;
    }

    burst = calloc(opts.burst_size, sizeof(*burst));
    if (burst == NULL) {
        fprintf(stderr, "Failed to allocate burst array\n");
        return 1;
    }

    tsc_hz = rte_get_tsc_hz();
    if (tsc_hz == 0) {
        fprintf(stderr, "Failed to query TSC frequency\n");
        return 1;
    }

    if (rte_mbuf_dyn_rx_timestamp_register(&clock_state.field_offset, &clock_state.rx_flag) == 0) {
        clock_state.field_registered = true;
    }

    rte_eth_dev_info_get(port_id, &dev_info);

    memset(&port_conf, 0, sizeof(port_conf));
    port_conf.rxmode.mq_mode = RTE_ETH_MQ_RX_NONE;
    if (clock_state.field_registered &&
        (dev_info.rx_offload_capa & RTE_ETH_RX_OFFLOAD_TIMESTAMP) != 0) {
        port_conf.rxmode.offloads |= RTE_ETH_RX_OFFLOAD_TIMESTAMP;
        clock_state.offload_configured = true;
        clock_state.supported = true;
    }

    pool = rte_pktmbuf_pool_create("dpdk_rx_latency_pool",
        opts.mbuf_count,
        DEFAULT_MEMPOOL_CACHE,
        0,
        RTE_MBUF_DEFAULT_BUF_SIZE,
        rte_socket_id());
    if (pool == NULL) {
        fprintf(stderr, "rte_pktmbuf_pool_create failed: %s\n", rte_strerror(rte_errno));
        return 1;
    }

    ret = rte_eth_dev_configure(port_id, 1, 1, &port_conf);
    if (ret != 0) {
        fprintf(stderr, "rte_eth_dev_configure failed: %s\n", rte_strerror(-ret));
        return 1;
    }

    rx_conf = dev_info.default_rxconf;
    rx_conf.offloads = port_conf.rxmode.offloads;
    ret = rte_eth_rx_queue_setup(port_id,
        opts.queue_id,
        opts.rx_desc,
        rte_eth_dev_socket_id(port_id),
        &rx_conf,
        pool);
    if (ret != 0) {
        fprintf(stderr, "rte_eth_rx_queue_setup failed: %s\n", rte_strerror(-ret));
        return 1;
    }

    tx_conf = dev_info.default_txconf;
    tx_conf.offloads = port_conf.txmode.offloads;
    ret = rte_eth_tx_queue_setup(port_id,
        opts.queue_id,
        opts.tx_desc,
        rte_eth_dev_socket_id(port_id),
        &tx_conf);
    if (ret != 0) {
        fprintf(stderr, "rte_eth_tx_queue_setup failed: %s\n", rte_strerror(-ret));
        return 1;
    }

    ret = rte_eth_dev_start(port_id);
    if (ret != 0) {
        fprintf(stderr, "rte_eth_dev_start failed: %s\n", rte_strerror(-ret));
        return 1;
    }

    rte_eth_promiscuous_enable(port_id);

    if (clock_state.offload_configured) {
        ret = rte_eth_timesync_enable(port_id);
        if (ret == 0) {
            clock_state.timesync_enabled = true;
            if (calibrate_port_clock(port_id, opts.calibration_ms, &clock_state) != 0) {
                clock_state.calibrated = false;
            }
        } else {
            clock_state.offload_configured = false;
            clock_state.timesync_enabled = false;
        }
    }

    if (wait_for_link_up(port_id, 10000U, &link) != 0) {
        fprintf(stderr, "Port %u link is down\n", port_id);
        return 1;
    }

    start_ns = monotonic_ns();
    deadline_ns = start_ns + ((uint64_t)opts.duration_seconds * NS_PER_SEC);

    while (!g_stop_requested) {
        uint64_t now_ns = monotonic_ns();
        uint64_t burst_return_tsc;
        if (now_ns >= deadline_ns) {
            break;
        }

        nb_rx = rte_eth_rx_burst(port_id, opts.queue_id, burst, opts.burst_size);
        burst_return_tsc = rte_rdtsc_precise();
        counts.rx_bursts++;

        if (nb_rx == 0) {
            counts.rx_empty_polls++;
            continue;
        }

        for (uint16_t i = 0; i < nb_rx; i++) {
            struct rte_mbuf *mbuf = burst[i];
            uint64_t classify_end_tsc;
            uint64_t classify_end_ns = 0;
            uint64_t classify_latency_ns;
            enum packet_classify_result result;

            counts.packets_seen++;
            counts.bytes_seen += rte_pktmbuf_pkt_len(mbuf);
            result = classify_udp_range(mbuf, opts.udp_min, opts.udp_max);

            classify_end_tsc = rte_rdtsc_precise();
            classify_latency_ns = ((classify_end_tsc - burst_return_tsc) * NS_PER_SEC) / tsc_hz;
            histogram_record(&rx_hist, classify_latency_ns);

            switch (result) {
            case PACKET_MATCH:
                counts.udp_range_seen++;
                if (rte_pktmbuf_pkt_len(mbuf) > 1000U) {
                    counts.large_udp_range_seen++;
                }
                break;
            case PACKET_DROP_PARSE_L2:
                counts.drop_parse_l2++;
                break;
            case PACKET_DROP_PARSE_L3:
                counts.drop_parse_l3++;
                break;
            case PACKET_DROP_PARSE_L4:
                counts.drop_parse_l4++;
                break;
            case PACKET_DROP_WRONG_UDP_PORT:
                counts.drop_wrong_udp_port++;
                break;
            }

            if (clock_state.calibrated && clock_state.field_offset >= 0 &&
                (mbuf->ol_flags & clock_state.rx_flag) != 0) {
                rte_mbuf_timestamp_t raw_hw_ticks =
                    *RTE_MBUF_DYNFIELD(mbuf, clock_state.field_offset, rte_mbuf_timestamp_t *);
                classify_end_ns = monotonic_ns();
                double diff_ticks = (double)((int64_t)(raw_hw_ticks - clock_state.base_clock));
                uint64_t hw_packet_ns = clock_state.base_mono_ns;
                if (diff_ticks >= 0.0) {
                    hw_packet_ns += (uint64_t)(diff_ticks * clock_state.ns_per_tick);
                }
                if (classify_end_ns >= hw_packet_ns) {
                    histogram_record(&hw_hist, classify_end_ns - hw_packet_ns);
                    counts.hw_timestamp_packets++;
                }
            }

            rte_pktmbuf_free(mbuf);
        }
    }

    end_ns = monotonic_ns();
    rte_eth_stats_get(port_id, &stats);

    json_file = fopen(opts.json_path, "w");
    if (json_file == NULL) {
        fprintf(stderr, "Failed to open JSON output %s: %s\n",
            opts.json_path,
            strerror(errno));
        return 1;
    }

    if (write_json(stdout,
            &opts,
            &counts,
            &rx_hist,
            &hw_hist,
            &clock_state,
            &stats,
            end_ns - start_ns,
            tsc_hz,
            &dev_info) < 0 ||
        write_json(json_file,
            &opts,
            &counts,
            &rx_hist,
            &hw_hist,
            &clock_state,
            &stats,
            end_ns - start_ns,
            tsc_hz,
            &dev_info) < 0) {
        fprintf(stderr, "Failed to write JSON output\n");
        fclose(json_file);
        return 1;
    }

    fclose(json_file);
    if (clock_state.timesync_enabled) {
        rte_eth_timesync_disable(port_id);
    }
    rte_eth_dev_stop(port_id);
    rte_eth_dev_close(port_id);
    histogram_free(&rx_hist);
    histogram_free(&hw_hist);
    free(burst);
    return 0;
}
