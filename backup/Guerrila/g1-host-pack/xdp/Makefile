# shadow_rx_meta.bpf.o — XDP BPF program for Shadow Engine AF_XDP ingress
#
# Requires: clang, llvm-strip, linux headers (libbpf-dev or kernel-headers)
#
# Usage:
#   make                     # build shadow_rx_meta.bpf.o
#   make clean               # remove build artifacts
#
# Load onto NIC:
#   ip link set dev <IFACE> xdp obj shadow_rx_meta.bpf.o sec xdp

CLANG      ?= clang
LLVM_STRIP ?= llvm-strip
CFLAGS     := -O2 -g -target bpf -Wall -Werror

# Detect kernel headers
KERNEL_HEADERS ?= /usr/include
ARCH_HEADERS   ?= /usr/include/$(shell uname -m)-linux-gnu
CFLAGS += -I$(KERNEL_HEADERS)
ifneq ("$(wildcard $(ARCH_HEADERS)/asm/types.h)","")
CFLAGS += -I$(ARCH_HEADERS)
endif

.PHONY: all clean

all: shadow_rx_meta.bpf.o

shadow_rx_meta.bpf.o: shadow_rx_meta.bpf.c
	$(CLANG) $(CFLAGS) -c $< -o $@
	$(LLVM_STRIP) -g $@

clean:
	rm -f shadow_rx_meta.bpf.o
