import re

with open("crates/shadow-core/src/pipeline.rs", "r") as f:
    pipe = f.read()

pipe = pipe.replace("pub struct OverlayDelta {}", "pub struct OverlayDelta {\n    pub pool_index: u32,\n    pub reserve_in_delta: i64,\n    pub reserve_out_delta: i64,\n}")

pipe = pipe.replace("overlay_delta: OverlayDelta {},", """overlay_delta: OverlayDelta {
            pool_index: lookup_bundle.pool.pool_index,
            reserve_in_delta: quote_res.our_effective_amount_in as i64,
            reserve_out_delta: -(quote_res.expected_out as i64),
        },""")

with open("crates/shadow-core/src/pipeline.rs", "w") as f:
    f.write(pipe)
