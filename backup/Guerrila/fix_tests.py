import re

with open("crates/shadow-core/src/assembly.rs", "r") as f:
    text = f.read()

# Fix overlay delta tests
text = text.replace("overlay_delta: OverlayDelta {},", "overlay_delta: OverlayDelta { pool_index: 0, reserve_in_delta: 0, reserve_out_delta: 0 },")

with open("crates/shadow-core/src/assembly.rs", "w") as f:
    f.write(text)
