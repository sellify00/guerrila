use solana_connection_cache::{
    client_connection::ClientConnection as BlockingClientConnection,
    connection_cache_stats::ConnectionCacheStats,
};
use solana_quic_client::{
    nonblocking::quic_client::QuicLazyInitializedEndpoint, quic_client::QuicClientConnection,
};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderContact {
    pub leader_pubkey: String,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpuSenderMode {
    Quic,
    Udp,
    Fake,
}

impl TpuSenderMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "quic" => Ok(Self::Quic),
            "udp" => Ok(Self::Udp),
            "fake" => Ok(Self::Fake),
            other => Err(format!(
                "unsupported tiny_live sender mode '{other}' (expected quic, udp, or fake)"
            )),
        }
    }

    pub const fn is_fake(self) -> bool {
        matches!(self, Self::Fake)
    }

    pub const fn requires_quic_contact(self) -> bool {
        matches!(self, Self::Quic | Self::Fake)
    }
}

pub trait TpuWireSender {
    fn send(&mut self, tx_bytes: &[u8], targets: &[LeaderContact]) -> Result<u64, String>;
}

#[derive(Clone)]
pub struct QuicTpuWireSender {
    endpoint: Arc<QuicLazyInitializedEndpoint>,
    connection_stats: Arc<ConnectionCacheStats>,
}

impl QuicTpuWireSender {
    pub fn new() -> Self {
        Self {
            endpoint: Arc::new(QuicLazyInitializedEndpoint::default()),
            connection_stats: Arc::new(ConnectionCacheStats::default()),
        }
    }
}

impl Default for QuicTpuWireSender {
    fn default() -> Self {
        Self::new()
    }
}

impl TpuWireSender for QuicTpuWireSender {
    fn send(&mut self, tx_bytes: &[u8], targets: &[LeaderContact]) -> Result<u64, String> {
        if tx_bytes.is_empty() {
            return Err("empty transaction bytes".to_owned());
        }
        if targets.is_empty() {
            return Err("no TPU leader targets resolved".to_owned());
        }

        let mut success_count = 0u64;
        let mut failures = Vec::new();
        for target in targets {
            let address = resolve_socket_addr(&target.address)?;
            let connection = QuicClientConnection::new(
                self.endpoint.clone(),
                address,
                self.connection_stats.clone(),
            );
            match connection.send_data(tx_bytes) {
                Ok(()) => {
                    success_count = success_count.saturating_add(1);
                }
                Err(err) => failures.push(format!(
                    "quic send to {} (leader {}) failed: {}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use solana_keypair::Keypair;
    use solana_net_utils::bind_to_localhost;
    use solana_streamer::{
        quic::{QuicServerParams, SpawnServerResult},
        streamer::StakedNodes,
    };
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        time::{Duration, Instant},
    };

    #[test]
    fn sender_mode_parse_accepts_quic_udp_and_fake() {
        assert_eq!(TpuSenderMode::parse("quic").unwrap(), TpuSenderMode::Quic);
        assert_eq!(TpuSenderMode::parse("udp").unwrap(), TpuSenderMode::Udp);
        assert_eq!(TpuSenderMode::parse("fake").unwrap(), TpuSenderMode::Fake);
        assert!(TpuSenderMode::parse("bogus").is_err());
    }

    #[test]
    fn quic_sender_delivers_bytes_to_a_quic_tpu_listener() {
        let (packet_sender, packet_receiver) = unbounded();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let server_socket = bind_to_localhost().unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let keypair = Keypair::new();
        let SpawnServerResult {
            endpoints: _,
            thread,
            key_updater: _,
        } = solana_streamer::quic::spawn_server(
            "shadow-egress-tpu-test",
            "shadow_egress_tpu_test",
            server_socket.try_clone().unwrap(),
            &keypair,
            packet_sender,
            exit.clone(),
            staked_nodes,
            QuicServerParams::default(),
        )
        .unwrap();

        let mut sender = QuicTpuWireSender::new();
        let targets = vec![LeaderContact {
            leader_pubkey: "leader-1".to_owned(),
            address: server_socket.local_addr().unwrap().to_string(),
        }];
        assert_eq!(sender.send(&[1, 2, 3, 4], &targets).unwrap(), 1);

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut received = false;
        while Instant::now() < deadline {
            if let Ok(batch) = packet_receiver.recv_timeout(Duration::from_millis(250)) {
                if !batch.is_empty() {
                    received = true;
                    break;
                }
            }
        }

        drop(sender);
        exit.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        assert!(
            received,
            "expected the QUIC TPU listener to receive one packet"
        );
    }
}
