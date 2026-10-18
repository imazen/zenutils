# Causal profiling coverage inventory

Observed 2026-10-18 UTC from tracked files at the revisions below. These are local
checkout observations, not timing results or proof of working instrumentation.
The package count includes test tools and excluded packages; it is not a count
of production crates.

Reproduce using `just perf-inventory <repository-roots>` in zenutils.
See [the workflow](causal-profiling.md) for profiling and correctness gates.

| Repository | Revision | Package manifests | Harness sources | Textual Coz markers |
|---|---|---:|---:|---:|
| zenjpeg | `c30bf9388fd8` | 8 | 174 | 0 |
| zenwebp | `018753cf6df5` | 5 | 34 | 0 |
| zenpng | `fdaa8de687cc` | 3 | 32 | 0 |
| zengif | `874e59fecf71` | 3 | 14 | 0 |
| zenavif | `85dd0d2b609b` | 10 | 69 | 0 |
| zenrav1e | `a079b3cdee78` | 4 | 14 | 0 |
| zenjxl | `4a2c021b4589` | 3 | 4 | 0 |
| zenjxl-decoder | `426f179be58f` | 7 | 5 | 0 |
| jxl-encoder | `8c1ddd667ba1` | 7 | 407 | 0 |
| heic | `e5ad6684bcb3` | 10 | 25 | 0 |
| mozjpeg-rs | `45d36a7170da` | 3 | 65 | 0 |
| zenflate | `9658d5251180` | 5 | 18 | 0 |
| zenzop | `885dcfa37785` | 2 | 3 | 0 |
| zenzstd | `4caa66a5bfef` | 2 | 7 | 0 |
| zenquant | `ea47d33002a2` | 2 | 17 | 0 |
| zenblend | `636c847989a4` | 2 | 2 | 0 |
| zentone | `f32e091c3c26` | 2 | 13 | 0 |
| zenpixels | `640dcedf1ee3` | 5 | 19 | 0 |
| linear-srgb | `6bae33ee657d` | 3 | 17 | 0 |
| zenpipe | `3c517197e195` | 13 | 31 | 0 |
| zensim | `5a4d5fa3c213` | 11 | 98 | 0 |
| fast-ssim2 | `aaf7705be0b9` | 5 | 20 | 0 |
| zenanalyze | `14dbe93d6af2` | 9 | 35 | 0 |
| zenbitmaps | `1d82056c6613` | 3 | 1 | 0 |
| zenextras | `1058208c9eb4` | 7 | 2 | 0 |
| zenraw | `047af1d3974e` | 3 | 4 | 0 |
| ultrahdr | `73ab3d27dff7` | 5 | 4 | 0 |
| garb | `37c9bd6b5739` | 2 | 2 | 0 |
| zencodec | `5b253be5eaaa` | 4 | 2 | 0 |
| zennode | `414e3a677ba1` | 3 | 0 | 0 |
| zenwasm | `6e003dcff999` | 3 | 1 | 0 |
| zentract | `ec15c080c781` | 4 | 1 | 0 |
| ravif | `680ca56d84b1` | 2 | 1 | 0 |
| enough | `bd674e80a615` | 15 | 3 | 0 |
| whereat | `9945a61ff4f7` | 3 | 16 | 0 |

## Work remaining

No optimization or verified causal profile is claimed by this inventory.
The first profiling targets are resize (zenresize), compression (zenflate),
and PNG (zenpng). zenresize was assigned separately and is absent from this
snapshot. Subsequent targets include the other codecs, pixel conversions,
pipeline operations, and metrics listed above.

This snapshot does not cover rav1d-safe, zenav1-svt, zenav1-aom, zenmetrics,
butteraugli, zensally, super-resolution, or every external workspace dependency.
Inventory and profile them before claiming complete ecosystem coverage.
