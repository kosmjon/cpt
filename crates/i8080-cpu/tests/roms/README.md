# 8080 CP/M diagnostic test ROMs

Public-domain 8080 CPU test programs used by the conformance suite
(`tests/conformance.rs`). These are the de-facto standard for validating 8080 emulators.

| File | Purpose |
|------|---------|
| `8080PRE.COM` | Preliminary quick test |
| `TST8080.COM` | Microcosm Associates 8080/8085 CPU diagnostic |
| `CPUTEST.COM` | Supersoft Associates diagnostic (more thorough) |
| `8080EXM.COM` | 8080 instruction exerciser (exhaustive; CRC-checks every group) |

Obtained from the `cpu_tests/` directory of the `superzazu/8080` project
(https://github.com/superzazu/8080), a widely-referenced source for these binaries.
They are vintage public-domain diagnostics originally distributed for CP/M.
