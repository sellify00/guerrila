use shadow_ipc::ingress_record::{self, IngressRecord, FEED_RING_DEFAULT_CAPACITY};
use shadow_ipc::ring::{MappedSpscRing, RingError};
use std::path::{Path, PathBuf};

use crate::grpc_producer::ValidatedTopology;

#[derive(Debug)]
pub enum DpdkProducerError {
    InvalidProducerId,
    RingCreateFailed { worker_id: u32, source: RingError },
}

impl std::fmt::Display for DpdkProducerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProducerId => write!(f, "invalid DPDK producer ID"),
            Self::RingCreateFailed { worker_id, source } => {
                write!(
                    f,
                    "feed ring create failed for worker {worker_id}: {source}"
                )
            }
        }
    }
}

impl std::error::Error for DpdkProducerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RingCreateFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn dpdk_producer_id(shred_capture_index: u32, shred_capture_workers: u32) -> Option<u32> {
    if shred_capture_workers == 0 || shred_capture_index >= shred_capture_workers {
        return None;
    }
    Some(shred_capture_index)
}

fn feed_ring_path_in(base_dir: &Path, worker_id: u32, producer_id: u32) -> PathBuf {
    base_dir.join(format!("shadow_feed_ring_w{worker_id}_p{producer_id}"))
}

fn bootstrap_dpdk_feed_rings(
    topology: &ValidatedTopology,
    shred_capture_index: u32,
    capacity: u32,
    base_dir: &Path,
) -> Result<Vec<MappedSpscRing<IngressRecord>>, DpdkProducerError> {
    let producer_id = dpdk_producer_id(shred_capture_index, topology.shred_capture_workers)
        .ok_or(DpdkProducerError::InvalidProducerId)?;
    let mut rings = Vec::with_capacity(topology.money_workers as usize);
    for worker_id in 0..topology.money_workers {
        let path = feed_ring_path_in(base_dir, worker_id, producer_id);
        let ring = ingress_record::create_feed_ring(&path, capacity)
            .map_err(|source| DpdkProducerError::RingCreateFailed { worker_id, source })?;
        rings.push(ring);
    }
    Ok(rings)
}

pub struct DpdkFeedEmitter {
    pub producer_id: u32,
    pub queue_id: u32,
    pub rings: Vec<MappedSpscRing<IngressRecord>>,
}

impl DpdkFeedEmitter {
    pub fn new(
        topology: &ValidatedTopology,
        shred_capture_index: u32,
        capacity: u32,
        base_dir: &Path,
    ) -> Result<Self, DpdkProducerError> {
        let producer_id = dpdk_producer_id(shred_capture_index, topology.shred_capture_workers)
            .ok_or(DpdkProducerError::InvalidProducerId)?;
        let rings = bootstrap_dpdk_feed_rings(topology, shred_capture_index, capacity, base_dir)?;
        Ok(Self {
            producer_id,
            queue_id: shred_capture_index,
            rings,
        })
    }
}

pub struct DpdkProducerRegistry {
    emitters: Vec<DpdkFeedEmitter>,
}

impl DpdkProducerRegistry {
    pub fn bootstrap(
        topology: &ValidatedTopology,
        base_dir: &Path,
    ) -> Result<Self, DpdkProducerError> {
        let mut emitters = Vec::with_capacity(topology.shred_capture_workers as usize);
        for idx in 0..topology.shred_capture_workers {
            emitters.push(DpdkFeedEmitter::new(
                topology,
                idx,
                FEED_RING_DEFAULT_CAPACITY,
                base_dir,
            )?);
        }
        Ok(Self { emitters })
    }

    pub fn len(&self) -> usize {
        self.emitters.len()
    }

    pub fn into_emitters(self) -> Vec<DpdkFeedEmitter> {
        self.emitters
    }
}
