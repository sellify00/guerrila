use crate::hash_monitor::HashMonitorOutcome;
use crate::tripwire::TripwireOutcome;
use shadow_ipc::parsing_manifest::ParsingManifestEntryV1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosettaEventKind {
    G9ATickBoundary,
    TacWorker,
    Opso,
    OracleTelemetry,
    Tripwire,
    ManifestRepair,
    ProgramHashQuarantine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosettaEvent {
    pub kind: RosettaEventKind,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct RosettaSink {
    enabled: bool,
    events: Vec<RosettaEvent>,
}

impl RosettaSink {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            events: Vec::new(),
        }
    }

    pub fn ingest_log_line(&mut self, target: &str, message: impl Into<String>) -> bool {
        if !self.enabled {
            return false;
        }

        let normalized = normalize_target(target);
        let Some(kind) = classify_target(&normalized) else {
            return false;
        };

        self.events.push(RosettaEvent {
            kind,
            target: normalized,
            message: message.into(),
        });
        true
    }

    pub fn record_tripwire(&mut self, tripwire: &TripwireOutcome) {
        let _ = self.ingest_log_line(
            "[tripwire]",
            format!(
                "grpc_only={} af_xdp_blind={} affected_venues={:?}",
                tripwire.grpc_only, tripwire.af_xdp_blind, tripwire.affected_venues
            ),
        );
    }

    pub fn record_manifest_repair(
        &mut self,
        repair_mode: &str,
        entries: &[ParsingManifestEntryV1],
    ) {
        let _ = self.ingest_log_line(
            "[manifest_repair]",
            format!("mode={repair_mode} entry_count={}", entries.len()),
        );
    }

    pub fn record_hash_quarantine(&mut self, outcome: &HashMonitorOutcome) {
        for event in &outcome.events {
            let _ = self.ingest_log_line(
                "[program_hash_quarantine]",
                format!(
                    "venue_id={} reason={} known={:02x?} observed={:02x?}",
                    event.venue_id,
                    event.reason.as_str(),
                    event.known_hash,
                    event.observed_hash,
                ),
            );
        }
    }
}

fn normalize_target(target: &str) -> String {
    target.trim().to_ascii_lowercase()
}

fn classify_target(target: &str) -> Option<RosettaEventKind> {
    if target == "[g9a_tick_boundary]" || target == "g9a_tick_boundary" {
        return Some(RosettaEventKind::G9ATickBoundary);
    }
    if target.starts_with("[tac-w") || target.starts_with("tac-w") {
        return Some(RosettaEventKind::TacWorker);
    }
    if target == "[opso]" || target == "opso" {
        return Some(RosettaEventKind::Opso);
    }
    if target == "[oracle telemetry]" || target == "oracle telemetry" {
        return Some(RosettaEventKind::OracleTelemetry);
    }
    if target == "[tripwire]" || target == "tripwire" {
        return Some(RosettaEventKind::Tripwire);
    }
    if target == "[manifest_repair]" || target == "manifest_repair" {
        return Some(RosettaEventKind::ManifestRepair);
    }
    if target == "[program_hash_quarantine]" || target == "program_hash_quarantine" {
        return Some(RosettaEventKind::ProgramHashQuarantine);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rosetta_sink_ingests_requested_targets() {
        let mut sink = RosettaSink::new(true);
        assert!(sink.ingest_log_line("[g9a_tick_boundary]", "tick"));
        assert!(sink.ingest_log_line("[tac-w0]", "worker"));
        assert!(sink.ingest_log_line("[opso]", "observe"));
        assert!(sink.ingest_log_line("[oracle telemetry]", "telemetry"));
        assert!(sink.ingest_log_line("[tripwire]", "blind"));
        assert!(sink.ingest_log_line("[manifest_repair]", "repair"));
        assert!(sink.ingest_log_line("[program_hash_quarantine]", "hash"));
        assert_eq!(sink.events.len(), 7);
    }

    #[test]
    fn g11_no_money_worker_dependency() {
        let files = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/rosetta_sink.rs"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/hash_monitor.rs"),
        ];
        for file in files {
            let contents = std::fs::read_to_string(file).unwrap();
            let test_boundary = contents.find("#[cfg(test)]").unwrap_or(contents.len());
            let production = &contents[..test_boundary];
            assert!(
                !production.contains("money_worker"),
                "shadow-control must not depend on money_worker: {file}"
            );
        }
    }

    #[test]
    fn g11_no_public_route_branch_added() {
        let files = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/hash_monitor.rs"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/rosetta_sink.rs"),
        ];
        let banned = [
            "public_execution",
            "public_send",
            "fallback_to_public",
            "open_public",
            "reopen_public",
        ];
        for file in files {
            let contents = std::fs::read_to_string(file).unwrap();
            let test_boundary = contents.find("#[cfg(test)]").unwrap_or(contents.len());
            let production = &contents[..test_boundary];
            for pat in &banned {
                assert!(
                    !production.contains(pat),
                    "shadow-control must not add public route branches via '{pat}' in {file}"
                );
            }
        }
    }
}
