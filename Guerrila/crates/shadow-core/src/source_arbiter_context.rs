use std::cell::Cell;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceArbiterStageContext {
    pub source_recv_ns: u64,
    pub decode_ns: u64,
    pub normalized_record_ns: u64,
    pub worker_seen_ns: u64,
    pub route_ready_ns: u64,
    pub quote_ready_ns: u64,
    pub pm_ready_ns: u64,
    pub assembly_ready_ns: u64,
}

thread_local! {
    static SOURCE_ARBITER_CONTEXT: Cell<SourceArbiterStageContext> =
        Cell::new(SourceArbiterStageContext::default());
}

#[inline(always)]
fn observe_min(slot: &mut u64, value: u64) {
    if value == 0 {
        return;
    }
    if *slot == 0 || value < *slot {
        *slot = value;
    }
}

#[inline(always)]
fn update_context(update: impl FnOnce(&mut SourceArbiterStageContext)) {
    SOURCE_ARBITER_CONTEXT.with(|cell| {
        let mut current = cell.get();
        update(&mut current);
        cell.set(current);
    });
}

pub fn reset_source_arbiter_context() {
    SOURCE_ARBITER_CONTEXT.with(|cell| cell.set(SourceArbiterStageContext::default()));
}

pub fn begin_source_arbiter_context(
    source_recv_ns: u64,
    decode_ns: u64,
    normalized_record_ns: u64,
    worker_seen_ns: u64,
    route_ready_ns: u64,
) {
    SOURCE_ARBITER_CONTEXT.with(|cell| {
        cell.set(SourceArbiterStageContext {
            source_recv_ns,
            decode_ns,
            normalized_record_ns,
            worker_seen_ns,
            route_ready_ns,
            ..SourceArbiterStageContext::default()
        });
    });
}

pub fn current_source_arbiter_context() -> SourceArbiterStageContext {
    SOURCE_ARBITER_CONTEXT.with(Cell::get)
}

pub fn source_arbiter_route_anchor_ns() -> u64 {
    SOURCE_ARBITER_CONTEXT.with(|cell| cell.get().route_ready_ns)
}

pub fn mark_source_arbiter_quote_ready_ns(value: u64) {
    update_context(|current| observe_min(&mut current.quote_ready_ns, value));
}

pub fn mark_source_arbiter_pm_ready_ns(value: u64) {
    update_context(|current| observe_min(&mut current.pm_ready_ns, value));
}

pub fn mark_source_arbiter_assembly_ready_ns(value: u64) {
    update_context(|current| observe_min(&mut current.assembly_ready_ns, value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_clears_context() {
        reset_source_arbiter_context();
        begin_source_arbiter_context(10, 20, 30, 40, 50);
        mark_source_arbiter_quote_ready_ns(60);
        mark_source_arbiter_pm_ready_ns(70);
        mark_source_arbiter_assembly_ready_ns(80);
        assert_eq!(
            current_source_arbiter_context(),
            SourceArbiterStageContext {
                source_recv_ns: 10,
                decode_ns: 20,
                normalized_record_ns: 30,
                worker_seen_ns: 40,
                route_ready_ns: 50,
                quote_ready_ns: 60,
                pm_ready_ns: 70,
                assembly_ready_ns: 80,
            }
        );
        reset_source_arbiter_context();
        assert_eq!(
            current_source_arbiter_context(),
            SourceArbiterStageContext::default()
        );
    }

    #[test]
    fn retains_earliest_marked_stage() {
        reset_source_arbiter_context();
        begin_source_arbiter_context(0, 0, 0, 0, 100);
        mark_source_arbiter_quote_ready_ns(140);
        mark_source_arbiter_quote_ready_ns(130);
        mark_source_arbiter_pm_ready_ns(220);
        mark_source_arbiter_pm_ready_ns(225);
        let current = current_source_arbiter_context();
        assert_eq!(current.quote_ready_ns, 130);
        assert_eq!(current.pm_ready_ns, 220);
    }
}
