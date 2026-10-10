# fast-spektrafilm

Implement the rewrite in Rust in this directory. Treat
the sibling `../spektrafilm` checkout as read-only reference. The pinned baseline is
experimental commit `28bf883e1672e884307edc75852549376e13644e`, not main/dev.
See BASELINE.json for original asset hashes. Copy measurement data and preserve
licenses; do not recreate profiles or LUTs by hand.

Prioritize the library and CLI. Preserve the reference processing model and
verify every supported stage using float64 differential tests. Report measured
errors, timing, and memory with identical settings. Do not substitute faster
approximate models or silently ignore unsupported parameters. Python under
tools/ is reference extraction, validation, and benchmark tooling only; the
processing implementation must remain Rust. native/raw.c only adapts LibRaw's ABI.

Use crops from samples/ for tests and benchmarks; cache crops after RAW decoding.
Do not run the reference simulation on full-size sample images by default.
Build the GUI using https://github.com/slint-ui/slint after core completion.
Ask the user before installing packages into the base system. User-local tools
may live under .tools/. No implementation changes belong in the original repo.
