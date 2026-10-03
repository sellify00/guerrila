#define _GNU_SOURCE

#include <pthread.h>
#include <rte_branch_prediction.h>
#include <rte_cycles.h>
#include <rte_eal.h>
#include <rte_errno.h>
#include <rte_ethdev.h>
#include <rte_launch.h>
#include <rte_lcore.h>
#include <rte_malloc.h>
#include <rte_mbuf.h>
#include <rte_mempool.h>
#include <sched.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

struct shadow_dpdk_config {
    uint16_t port_id;
    uint16_t queue_count;
    uint16_t burst_size;
    uint16_t rx_desc;
    uint32_t mbuf_count;
    uint32_t mbuf_cache_size;
    uint8_t promiscuous;
};

struct shadow_dpdk_packet {
    uint16_t queue_id;
    const uint8_t *data;
    uint32_t len;
    uint64_t rx_timestamp_ns;
};

typedef int (*shadow_dpdk_packet_cb)(void *user, const struct shadow_dpdk_packet *packet);

struct queue_worker_args {
    const struct shadow_dpdk_config *cfg;
    uint16_t queue_id;
    shadow_dpdk_packet_cb callback;
    void *user;
    volatile int *stop_flag;
};

static uint64_t shadow_monotonic_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        return 0;
    }
    return ((uint64_t)ts.tv_sec * 1000000000ULL) + (uint64_t)ts.tv_nsec;
}

static void shadow_set_error(char *errbuf, size_t errbuf_len, const char *fmt, ...) {
    if (errbuf == NULL || errbuf_len == 0) {
        return;
    }
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(errbuf, errbuf_len, fmt, ap);
    va_end(ap);
}

static bool shadow_wait_link_up(uint16_t port_id, char *errbuf, size_t errbuf_len) {
    struct rte_eth_link link;
    for (int attempt = 0; attempt < 300; ++attempt) {
        memset(&link, 0, sizeof(link));
        if (rte_eth_link_get_nowait(port_id, &link) == 0 && link.link_status) {
            return true;
        }
        usleep(100000);
    }
    shadow_set_error(errbuf, errbuf_len, "DPDK port %u link did not come up", port_id);
    return false;
}

static void *shadow_dpdk_queue_worker(void *arg) {
    struct queue_worker_args *worker = (struct queue_worker_args *)arg;
    const uint16_t port_id = worker->cfg->port_id;
    const uint16_t queue_id = worker->queue_id;
    const uint16_t burst_size = worker->cfg->burst_size == 0 ? 32 : worker->cfg->burst_size;
    struct rte_mbuf *bufs[256];

    while (*(worker->stop_flag) == 0) {
        const uint16_t nb = rte_eth_rx_burst(port_id, queue_id, bufs, burst_size);
        if (nb == 0) {
            rte_pause();
            continue;
        }

        for (uint16_t i = 0; i < nb; ++i) {
            struct rte_mbuf *m = bufs[i];
            struct shadow_dpdk_packet packet;
            packet.queue_id = queue_id;
            packet.data = rte_pktmbuf_mtod(m, const uint8_t *);
            packet.len = rte_pktmbuf_pkt_len(m);
            packet.rx_timestamp_ns = shadow_monotonic_ns();
            worker->callback(worker->user, &packet);
            rte_pktmbuf_free(m);
        }
    }

    return NULL;
}

int shadow_dpdk_run(const struct shadow_dpdk_config *cfg,
                    shadow_dpdk_packet_cb callback,
                    void **queue_users,
                    volatile int *stop_flag,
                    char *errbuf,
                    size_t errbuf_len) {
    if (cfg == NULL || callback == NULL || queue_users == NULL || stop_flag == NULL) {
        shadow_set_error(errbuf, errbuf_len, "invalid shadow_dpdk_run arguments");
        return -1;
    }
    if (cfg->queue_count == 0) {
        shadow_set_error(errbuf, errbuf_len, "queue_count must be > 0");
        return -1;
    }
    if (cfg->burst_size == 0 || cfg->burst_size > 256) {
        shadow_set_error(errbuf, errbuf_len, "burst_size must be in 1..=256");
        return -1;
    }

    char *argv[] = {"shadow-tier0", "--proc-type=auto"};
    int argc = (int)(sizeof(argv) / sizeof(argv[0]));
    int eal_rc = rte_eal_init(argc, argv);
    if (eal_rc < 0) {
        shadow_set_error(errbuf, errbuf_len, "rte_eal_init failed: %s", rte_strerror(rte_errno));
        return -1;
    }

    struct rte_mempool *pool = rte_pktmbuf_pool_create(
        "shadow_dpdk_pool",
        cfg->mbuf_count == 0 ? 8192u : cfg->mbuf_count,
        cfg->mbuf_cache_size == 0 ? 256u : cfg->mbuf_cache_size,
        0,
        RTE_MBUF_DEFAULT_BUF_SIZE,
        rte_socket_id());
    if (pool == NULL) {
        shadow_set_error(errbuf, errbuf_len, "rte_pktmbuf_pool_create failed: %s", rte_strerror(rte_errno));
        return -1;
    }

    struct rte_eth_conf port_conf;
    memset(&port_conf, 0, sizeof(port_conf));

    if (rte_eth_dev_configure(cfg->port_id, cfg->queue_count, 0, &port_conf) < 0) {
        shadow_set_error(errbuf, errbuf_len, "rte_eth_dev_configure failed for port %u", cfg->port_id);
        return -1;
    }

    uint16_t rx_desc = cfg->rx_desc == 0 ? 1024 : cfg->rx_desc;
    for (uint16_t queue_id = 0; queue_id < cfg->queue_count; ++queue_id) {
        if (rte_eth_rx_queue_setup(
                cfg->port_id,
                queue_id,
                rx_desc,
                rte_eth_dev_socket_id(cfg->port_id),
                NULL,
                pool) < 0) {
            shadow_set_error(errbuf, errbuf_len, "rte_eth_rx_queue_setup failed for port %u queue %u",
                             cfg->port_id,
                             queue_id);
            return -1;
        }
    }

    if (rte_eth_dev_start(cfg->port_id) < 0) {
        shadow_set_error(errbuf, errbuf_len, "rte_eth_dev_start failed for port %u", cfg->port_id);
        return -1;
    }

    if (cfg->promiscuous) {
        rte_eth_promiscuous_enable(cfg->port_id);
    }

    if (!shadow_wait_link_up(cfg->port_id, errbuf, errbuf_len)) {
        return -1;
    }

    pthread_t *threads = calloc(cfg->queue_count, sizeof(pthread_t));
    struct queue_worker_args *workers = calloc(cfg->queue_count, sizeof(struct queue_worker_args));
    if (threads == NULL || workers == NULL) {
        shadow_set_error(errbuf, errbuf_len, "failed to allocate DPDK worker thread state");
        free(threads);
        free(workers);
        return -1;
    }

    for (uint16_t queue_id = 0; queue_id < cfg->queue_count; ++queue_id) {
        workers[queue_id].cfg = cfg;
        workers[queue_id].queue_id = queue_id;
        workers[queue_id].callback = callback;
        workers[queue_id].user = queue_users[queue_id];
        workers[queue_id].stop_flag = stop_flag;
        if (pthread_create(&threads[queue_id], NULL, shadow_dpdk_queue_worker, &workers[queue_id]) != 0) {
            *stop_flag = 1;
            shadow_set_error(errbuf, errbuf_len, "failed to start DPDK worker thread for queue %u", queue_id);
            for (uint16_t j = 0; j < queue_id; ++j) {
                pthread_join(threads[j], NULL);
            }
            free(threads);
            free(workers);
            return -1;
        }
    }

    for (uint16_t queue_id = 0; queue_id < cfg->queue_count; ++queue_id) {
        pthread_join(threads[queue_id], NULL);
    }

    rte_eth_dev_stop(cfg->port_id);
    rte_eth_dev_close(cfg->port_id);
    rte_eal_cleanup();
    free(threads);
    free(workers);
    return 0;
}
