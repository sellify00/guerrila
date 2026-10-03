use crate::rpc::RpcProbeError;

#[derive(Debug, thiserror::Error)]
pub enum TpuProxyError {
    #[error("failed to open TPU egress ring for worker {worker_id}: {source}")]
    RingOpen {
        worker_id: u32,
        source: shadow_ipc::ring::RingError,
    },
    #[error("failed to open NACK ring: {0}")]
    NackRingOpen(shadow_ipc::ring::RingError),
    #[error("failed to open TPU result ring: {0}")]
    ResultRingOpen(shadow_ipc::ring::RingError),
    #[error("failed to publish TPU no-send result: result ring full")]
    ResultRingFull,
    #[error("failed to publish TPU NACK: nack ring full")]
    NackRingFull,
    #[error("failed to write TPU tiny-live artifact '{path}': {source}")]
    ArtifactWrite {
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Rpc(#[from] RpcProbeError),
}
