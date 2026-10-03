// shadow_rx_meta.bpf.c — XDP program for Shadow Engine AF_XDP ingress
//
// Writes a 16-byte ShadowRxMetaV1 header into the XDP metadata area
// (between data_meta and data), then redirects into the AF_XDP socket.
// The kernel's AF_XDP path places the metadata in the UMEM headroom
// immediately before the packet data, so the receiver can read it at
// (packet_offset - 16) without touching the packet bytes.
//
//   Offset  Size  Field
//   ──────  ────  ─────────────────
//   0       4     magic            (0x314d5853 = "SXM1" LE)
//   4       2     version          (1)
//   6       2     header_len       (16)
//   8       8     rx_timestamp_ns  (bpf_ktime_get_ns)
//
// Must match crates/shadow-tier0/src/xsk.rs ShadowRxMetaV1 layout exactly.
//
// Build:  clang -O2 -g -target bpf -c shadow_rx_meta.bpf.c -o shadow_rx_meta.bpf.o

#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/ipv6.h>
#include <linux/udp.h>
#include <bpf/bpf_endian.h>
#include <bpf/bpf_helpers.h>

#define SHADOW_RX_META_MAGIC   0x314d5853  /* "SXM1" little-endian */
#define SHADOW_RX_META_VERSION 1
#define SHADOW_RX_META_LEN     16
#define SHADOW_SOLANA_PORT_MIN 8000
#define SHADOW_SOLANA_PORT_MAX 8025
#define SHADOW_SHRED_COMMON_HDR_LEN 83
#define SHADOW_SHRED_VARIANT_OFFSET 64
#define SHADOW_SHRED_VERSION_OFFSET 77
#define SHADOW_MAINNET_SHRED_VERSION 50093
#define SHADOW_MAX_VLAN_DEPTH 2

enum shadow_xdp_diag_counter {
    SHADOW_XDP_DIAG_TOTAL_PACKETS = 0,
    SHADOW_XDP_DIAG_VLAN_TAGS,
    SHADOW_XDP_DIAG_IPV4_UDP,
    SHADOW_XDP_DIAG_IPV6_UDP,
    SHADOW_XDP_DIAG_PORT_RANGE,
    SHADOW_XDP_DIAG_SHRED_VARIANT,
    SHADOW_XDP_DIAG_SHRED_VERSION,
    SHADOW_XDP_DIAG_REDIRECT,
    SHADOW_XDP_DIAG_COUNTER_COUNT,
};

struct shadow_rx_meta_v1 {
    __u32 magic;
    __u16 version;
    __u16 header_len;
    __u64 rx_timestamp_ns;
} __attribute__((packed));

_Static_assert(sizeof(struct shadow_rx_meta_v1) == SHADOW_RX_META_LEN,
               "ShadowRxMetaV1 must be exactly 16 bytes");

struct shadow_xdp_diag_v1 {
    __u64 counters[SHADOW_XDP_DIAG_COUNTER_COUNT];
};

struct shadow_vlan_hdr {
    __be16 tci;
    __be16 encapsulated_proto;
} __attribute__((packed));

/* XSKMAP — userspace populates entry [queue_id] = xsk_fd after bind.
 * The kernel only needs the map to *exist* at bind time for zero-copy;
 * actual redirect happens per-packet via bpf_redirect_map(). */
struct {
    __uint(type, BPF_MAP_TYPE_XSKMAP);
    __type(key, __u32);
    __type(value, __u32);
    __uint(max_entries, 64);
} xsks_map SEC(".maps");

struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __type(key, __u32);
    __type(value, struct shadow_xdp_diag_v1);
    __uint(max_entries, 1);
} shadow_xdp_diag SEC(".maps");

static __always_inline void shadow_diag_inc(__u32 counter)
{
    __u32 key = 0;
    struct shadow_xdp_diag_v1 *diag;

    if (counter >= SHADOW_XDP_DIAG_COUNTER_COUNT)
        return;

    diag = bpf_map_lookup_elem(&shadow_xdp_diag, &key);
    if (!diag)
        return;

    diag->counters[counter]++;
}

static __always_inline int shadow_port_in_range(__u16 port_be)
{
    __u16 port = bpf_ntohs(port_be);

    return port >= SHADOW_SOLANA_PORT_MIN && port <= SHADOW_SOLANA_PORT_MAX;
}

static __always_inline int shadow_shred_variant(__u8 variant)
{
    __u8 merkle_tag = variant & 0xf0;

    return variant == 0xa5 || variant == 0x5a ||
           merkle_tag == 0x90 || merkle_tag == 0xb0 ||
           merkle_tag == 0x60 || merkle_tag == 0x70;
}

static __always_inline int shadow_udp_payload_is_mainnet_shred(void *payload, void *data_end)
{
    __u8 *bytes = payload;
    __u16 version;

    if ((void *)(bytes + SHADOW_SHRED_COMMON_HDR_LEN) > data_end)
        return 0;

    if (!shadow_shred_variant(bytes[SHADOW_SHRED_VARIANT_OFFSET]))
        return 0;
    shadow_diag_inc(SHADOW_XDP_DIAG_SHRED_VARIANT);

    version = (__u16)bytes[SHADOW_SHRED_VERSION_OFFSET] |
              ((__u16)bytes[SHADOW_SHRED_VERSION_OFFSET + 1] << 8);
    if (version != SHADOW_MAINNET_SHRED_VERSION)
        return 0;

    shadow_diag_inc(SHADOW_XDP_DIAG_SHRED_VERSION);
    return 1;
}

static __always_inline int shadow_is_solana_udp(struct xdp_md *ctx)
{
    void *data_end = (void *)(long)ctx->data_end;
    void *data = (void *)(long)ctx->data;
    struct ethhdr *eth = data;
    __u16 proto;
    __u64 offset = sizeof(*eth);
    int vlan_depth;

    if ((void *)(eth + 1) > data_end)
        return 0;

    shadow_diag_inc(SHADOW_XDP_DIAG_TOTAL_PACKETS);
    proto = eth->h_proto;

    for (vlan_depth = 0; vlan_depth < SHADOW_MAX_VLAN_DEPTH; vlan_depth++) {
        if (proto == bpf_htons(ETH_P_8021Q) || proto == bpf_htons(ETH_P_8021AD)) {
            struct shadow_vlan_hdr *vlan = data + offset;

            if ((void *)(vlan + 1) > data_end)
                return 0;

            proto = vlan->encapsulated_proto;
            offset += sizeof(*vlan);
            shadow_diag_inc(SHADOW_XDP_DIAG_VLAN_TAGS);
            continue;
        }

        break;
    }

    if (proto == bpf_htons(ETH_P_IP)) {
        struct iphdr *ip = data + offset;
        __u32 ip_len;
        struct udphdr *udp;

        if ((void *)(ip + 1) > data_end)
            return 0;
        if (ip->protocol != IPPROTO_UDP)
            return 0;
        shadow_diag_inc(SHADOW_XDP_DIAG_IPV4_UDP);

        ip_len = ip->ihl * 4;
        if (ip_len < sizeof(*ip))
            return 0;

        udp = data + offset + ip_len;
        if ((void *)(udp + 1) > data_end)
            return 0;

        /* Only redirect raw shreds destined to our local Agave dynamic ports.
         * Do not redirect gossip/QUIC/bootstrap packets just because they use
         * the same dynamic port range. */
        if (!shadow_port_in_range(udp->dest))
            return 0;
        shadow_diag_inc(SHADOW_XDP_DIAG_PORT_RANGE);
        return shadow_udp_payload_is_mainnet_shred((void *)(udp + 1), data_end);
    }

    if (proto == bpf_htons(ETH_P_IPV6)) {
        struct ipv6hdr *ip6 = data + offset;
        struct udphdr *udp;

        if ((void *)(ip6 + 1) > data_end)
            return 0;
        if (ip6->nexthdr != IPPROTO_UDP)
            return 0;
        shadow_diag_inc(SHADOW_XDP_DIAG_IPV6_UDP);

        udp = data + offset + sizeof(*ip6);
        if ((void *)(udp + 1) > data_end)
            return 0;

        /* Only redirect raw shreds destined to our local Agave dynamic ports.
         * Do not redirect gossip/QUIC/bootstrap packets just because they use
         * the same dynamic port range. */
        if (!shadow_port_in_range(udp->dest))
            return 0;
        shadow_diag_inc(SHADOW_XDP_DIAG_PORT_RANGE);
        return shadow_udp_payload_is_mainnet_shred((void *)(udp + 1), data_end);
    }

    return 0;
}

SEC("xdp")
int shadow_rx_meta_prog(struct xdp_md *ctx)
{
    if (!shadow_is_solana_udp(ctx))
        return XDP_PASS;

    /* Grow metadata area by 16 bytes (between data_meta and data).
     * This places our header in the headroom that AF_XDP delivers
     * to userspace at (packet_offset - 16) in the UMEM frame. */
    if (bpf_xdp_adjust_meta(ctx, -(int)SHADOW_RX_META_LEN))
        return XDP_PASS;

    void *data_meta = (void *)(long)ctx->data_meta;
    void *data      = (void *)(long)ctx->data;

    /* Bounds check: ensure metadata area is large enough */
    if (data_meta + SHADOW_RX_META_LEN > data)
        return bpf_redirect_map(&xsks_map, ctx->rx_queue_index, XDP_PASS);

    struct shadow_rx_meta_v1 *meta = data_meta;

    meta->magic          = SHADOW_RX_META_MAGIC;
    meta->version        = SHADOW_RX_META_VERSION;
    meta->header_len     = SHADOW_RX_META_LEN;
    meta->rx_timestamp_ns = bpf_ktime_get_ns();

    /* Redirect into the AF_XDP socket for this queue.
     * XDP_PASS is the fallback if no socket is bound to this queue yet. */
    shadow_diag_inc(SHADOW_XDP_DIAG_REDIRECT);
    return bpf_redirect_map(&xsks_map, ctx->rx_queue_index, XDP_PASS);
}

char _license[] SEC("license") = "GPL";
