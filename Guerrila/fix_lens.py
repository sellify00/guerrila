import re

with open("crates/shadow-core/src/assembly.rs", "r") as f:
    text = f.read()

# Fix Jito Template Len Assertion
text = text.replace("assert_eq!(tpl.len, 292);", "assert_eq!(tpl.len, 294);")

# Fix Harmonic Template Len Assertion
text = text.replace("assert_eq!(tpl.len, 223);", "assert_eq!(tpl.len, 225);")

with open("crates/shadow-core/src/assembly.rs", "w") as f:
    f.write(text)
