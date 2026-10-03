use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderContact {
    pub leader_pubkey: String,
    pub address: String,
}

pub trait TpuWireSender {
    fn send(&mut self, tx_bytes: &[u8], targets: &[LeaderContact]) -> Result<u64, String>;
}

#[derive(Debug, Clone, Copy)]
pub struct UdpTpuWireSender {
    timeout: Duration,
}

impl UdpTpuWireSender {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout: Duration::from_millis(timeout_ms.max(1)),
        }
    }
}

impl TpuWireSender for UdpTpuWireSender {
    fn send(&mut self, tx_bytes: &[u8], targets: &[LeaderContact]) -> Result<u64, String> {
        if tx_bytes.is_empty() {
            return Err("empty transaction bytes".to_owned());
        }
        if targets.is_empty() {
            return Err("no TPU leader targets resolved".to_owned());
        }

        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|err| format!("failed to bind UDP sender socket: {err}"))?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(|err| format!("failed to set write timeout: {err}"))?;

        let mut success_count = 0u64;
        let mut failures = Vec::new();
        for target in targets {
            let address = resolve_socket_addr(&target.address)?;
            match socket.send_to(tx_bytes, address) {
                Ok(sent) if sent == tx_bytes.len() => {
                    success_count = success_count.saturating_add(1);
                }
                Ok(sent) => failures.push(format!(
                    "partial send to {} (leader {}): {} of {} bytes",
                    target.address,
                    target.leader_pubkey,
                    sent,
                    tx_bytes.len()
                )),
                Err(err) => failures.push(format!(
                    "send to {} (leader {}) failed: {}",
                    target.address, target.leader_pubkey, err
                )),
            }
        }

        if success_count > 0 {
            Ok(success_count)
        } else {
            Err(failures.join("; "))
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FakeTpuWireSender;

impl TpuWireSender for FakeTpuWireSender {
    fn send(&mut self, tx_bytes: &[u8], targets: &[LeaderContact]) -> Result<u64, String> {
        if tx_bytes.is_empty() {
            return Err("empty transaction bytes".to_owned());
        }
        if targets.is_empty() {
            return Err("no TPU leader targets resolved".to_owned());
        }
        Ok(1)
    }
}

fn resolve_socket_addr(value: &str) -> Result<SocketAddr, String> {
    value
        .to_socket_addrs()
        .map_err(|err| format!("failed to resolve '{value}': {err}"))?
        .next()
        .ok_or_else(|| format!("no socket address resolved for '{value}'"))
}
