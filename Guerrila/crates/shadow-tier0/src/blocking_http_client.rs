use core::time::Duration;
use reqwest::blocking::Client;

pub fn build_blocking_json_rpc_client(timeout_ms: u64) -> Client {
    let timeout = Duration::from_millis(timeout_ms);
    Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        // Avoid reusing idle sockets across native-XDP transition flaps on the
        // management NIC; fresh connections have proven reliable on-host.
        .pool_max_idle_per_host(0)
        .build()
        .expect("failed to build reqwest blocking client")
}
