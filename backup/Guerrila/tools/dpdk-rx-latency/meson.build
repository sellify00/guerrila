project(
  'dpdk-rx-latency',
  'c',
  version: '0.1.0',
  default_options: [
    'c_std=c11',
    'warning_level=3',
  ],
)

dpdk = dependency('libdpdk', method: 'pkg-config', required: true)

executable(
  'dpdk-rx-latency',
  'main.c',
  dependencies: [dpdk],
  install: false,
)
