# Shadow Engine V6.4-Guerrila — Handoff Status Memo

## Current conclusion

Guerrila remains the active project.

Full Shadow Engine should **not** be treated as the near-term deployment target for a 150 SOL bankroll. Guerrila is still the proof-and-survival profile: private-lane only, public execution amputated, hard treasury corset, and progressive rollout.

The current milestone picture is materially better than before:

- **G0** done
- **G2** done
- **G3** done
- **G6** done
- **G5** partial
- **G4** not started and is the next major build block
- **G7+** still open

The new additive quant directives are **not** a strategic reset. They are bounded additions to Guerrila, not permission to broaden the battlefield early.

## What Guerrila is right now

Guerrila is no longer just architecture or intent.

It now has three substantial pieces in place:

### 1. Shared-memory and state foundation

This is materially real now:

- versioned `/dev/shm` segments,
- wait-free RCU / double-buffer substrate,
- leader schedule + leader-class tags,
- NACK ring,
- pool and mint tag maps,
- pool and mint arenas,
- ALT arena and ALT tag map,
- health and egress-health segments.

That means the worker-side state substrate is no longer hypothetical.

### 2. `shadow-state` is real

`shadow-state` is no longer a scaffold.

Built or materially proven:

- premium shared Geyser/RPC source path,
- RPC warm-start + Yellowstone live stream,
- fork-aware canonical store,
- strict Raydium V4 pool decode,
- authoritative vault reserve tracking,
- SPL Token + Token-2022 mint decode,
- active transfer-fee publication,
- live fee-update freshness enforcement,
- ALT snapshot publication,
- leader schedule / leader-class publication,
- monotonic `snapshot_slot`,
- 1-hour read-only mainnet soak and live dynamic-append proof.

In plain terms: state, freshness, fees, ALTs, and routing tags now have a real off-core publisher.

### 3. The Jito private lane is real

The Jito side is materially complete:

- real `/dev/shm/shadow_egress_ring_jito_*` path,
- startup bootstrap for per-worker rings,
- dedicated Jito proxy crate,
- persistent Searcher API session,
- auth / refresh / reconnect path,
- dequeue -> one-envelope-one-bundle -> send,
- exact post-sign byte preservation,
- exactly-one NACK on local failure,
- coarse Jito lane health publication with hysteresis,
- fail-closed startup wiring from `shadow-tier0`.

So the private Jito egress lane is not a placeholder anymore.

## Practical completion view

Think of Guerrila in four layers.

### 1. Core doctrine and firewalls

This is strong.

Implemented or locked in:

- public lane amputated,
- private-lane-only routing doctrine,
- hard 15 SOL live-input corset,
- 10 / 15 / 30 SOL reserve/position/entity envelope,
- stale-state Harmonic reroute-to-Jito logic,
- pre-sign egress admission gate,
- class-scoped Harmonic quarantine doctrine,
- thread-local Pool-Slot Dominance Gate in observe-first posture,
- additive tactics constrained to fixed-layout / Jito-only / shadow-only forms.

### 2. Worker inner brain

This is materially underway, but not sealed.

Built or materially done:

- route hydration,
- pool/mint lookup,
- exact quote path,
- Token-2022 fee handling,
- execution policy,
- template assembly,
- signing,
- egress enqueue,
- dominance logic,
- fee clamp,
- banked leader-class LUT selection.

Still open in G5:

- no-alloc proof,
- no-syscall strace proof,
- thread-local overlay completion,
- Taylor pre-filter integration,
- microbench / p99 proof,
- some final live-loop integration gates.

So the worker brain is strong, but not yet fully certified.

### 3. State and private Jito plumbing

This is the most improved area.

- G2 is done.
- G3 is done.
- G6 is done.

That means the project now has a real shared-memory substrate, a real state daemon, and a real Jito private-lane proxy.

### 4. Outer live body

This is still incomplete.

The main open blocks are:

- **G1** commodity host determinism + kernel-bypass host bring-up,
- **G4** AF_XDP ingress, shred matrix, strict decode, and feed-ring production,
- **G7** Harmonic egress proxy,
- **G8** `shadow-pm` inventory truth and quarantine release paths,
- **G9** `shadow-oracle` dual LUTs + target filter,
- **G9A** additive quant tactics milestone,
- **G10** on-chain `ShadowGuard` + exact template audits,
- **G11** `shadow-control` Rosetta Stone auto-heal,
- **G12** observe-only / tiny-live rollout ladder.

So Guerrila is **not** end-to-end runnable yet.

## What is already strong

### Private-lane doctrine is coherent

The system still correctly behaves like Guerrila, not like a half-reverted public-war build:

- no public target execution,
- no TPU rescue logic,
- no accidental public fallback,
- Harmonic is conditional and fail-closed,
- risky tactics stay Jito-only,
- additive tactics remain constrained.

### Shared-state realism is much better now

The biggest change versus earlier handoffs is that state is no longer just “planned.”

The engine now has a real state daemon with:

- rooted canonical store,
- live mint fee freshness,
- ALT publication,
- leader-class tags,
- real soak proof,
- dynamic append behavior.

### Jito lane is no longer hypothetical

The Jito egress side has crossed from scaffold to real subsystem.

That matters because it means one private lane is already genuinely attached to the worker/IPC architecture.

## What is *not* true yet

Do **not** describe Guerrila as:

- complete,
- deploy-ready,
- observe-only ready,
- tiny-live ready,
- or fully integrated end-to-end.

That would still be false.

The best current description is:

> **The core doctrine, shared-state spine, and Jito private-lane arm are now real, but the system still lacks its AF_XDP ingress body, Harmonic arm, PM/oracle/control organs, and rollout proof.**

## Most important strategic interpretation

The project is in a better place technically, but the business truth has not changed:

Guerrila’s job is still to:

1. preserve capital,
2. measure the real private-lane slice,
3. prove whether a 150 SOL private-lane-only sniper can survive and retain EV,
4. earn the right to scale only if the evidence supports it.

The new additive directives do **not** change that.

They approve bounded future tactics such as:

- Taylor pre-filter only,
- Ghost Snipe cache prefetch only,
- minimum-empirical-safe Jito density optimization,
- deterministic Jito-only exhaust scavenging,
- tick-boundary sabotage in shadow-observe only.

Those are useful additions, but they are still subordinate to Guerrila’s redlines:

- do not hinder speed,
- do not create overhead,
- do not bleed capital,
- do not sacrifice major alpha.

## What to look out for

### 1. G4 is now the next real bottleneck

The next major missing organ is AF_XDP ingress.

Without G4, the system still lacks the real primary Tier-0 ingestion path:

- shred capture,
- fragment window,
- broad-spectrum matrix / FEC path,
- strict decode,
- allowlist prefilter,
- pool-sharded feed-ring production.

That is the next big milestone.

### 2. G5 is ahead in logic but behind in certification

The worker core has a lot of real logic already, but the remaining proof gates still matter.

Do not confuse “implemented” with “sealed.”

The main blocker there is not feature absence; it is hot-path proof:

- no alloc,
- no syscall,
- microbench,
- integration hardening.

### 3. G3/G6 progress can create checklist drift again

The project just moved from a world where G3 and G6 were genuinely open to a world where they are materially done.

Future handoffs must keep the checklist synced so later engineers do not think Guerrila is earlier than it really is.

### 4. Additive directives should not derail milestone order

The new additive quant directives are bounded and real, but they should mostly land in later places:

- worker-side gates/hardening in G5,
- oracle / replay / audit work in G9/G9A/G10,
- rollout proof in G12.

They should **not** cause the team to skip G4 or reopen public-lane complexity.

### 5. G1 still matters

Even with strong code progress, host determinism and kernel-bypass bring-up remain open.

So there is still a real risk of “good architecture on the wrong box.”

## Best current summary for another chat

Use this:

> Guerrila remains the active proving profile, not the full near-term deployment target. The project is now materially stronger than before: G0, G2, G3, and G6 are done; the shared-memory foundation, `shadow-state`, and the Jito private lane are real. The worker core is largely built but not yet fully sealed; G5 still needs no-alloc/no-syscall/perf proof. The system is still not end-to-end runnable because G1 host bring-up, G4 AF_XDP ingress, G7 Harmonic proxy, G8 PM, G9 oracle, G10 ShadowGuard audit, G11 Rosetta control, and G12 rollout proof remain open. The additive quant directives are not a reset; they are bounded future scope and should not interrupt G4.

## Immediate next build direction

The next major block should be:

1. **G4 — AF_XDP ingress, broad-spectrum shred matrix, and decode**
2. then continue closing the remaining **G5 certification gates** against the live ingress path
3. then **G7 / G8 / G9** in the normal private-lane completion order
4. only later pull forward **G9A** additive tactics beyond what is already structurally allowed

If forced to summarize in one sentence:

> **Build the missing ingress body now, not more optional cleverness.**

## Final business posture

Keep Guerrila alive.

Do **not** shelve it now.
Do **not** promote full Shadow Engine to near-term deployment target.
Do **not** reopen the public war machine.
Do **not** let additive tactics derail the proving sequence.

The correct posture is still:

- **Full Shadow Engine** = future expansion machine
- **Guerrila** = current proving weapon
- **Kill decision later** = only after observe-only + tiny-live evidence says the slice is not positive EV
