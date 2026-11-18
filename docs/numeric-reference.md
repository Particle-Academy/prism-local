# Preliminary scalar math references

The scalar `exp`, `erf` and `tanh` functions are experimental, bounded
approximations. Their tests compare the 1,264 values in `math-reference.csv` to
independent high-precision Decimal results. The fixture includes representative
values, points around approximation branches and 401 evenly spaced inputs per
function; it is not a proven global error bound or an embedding-quality result.
The `exp` input domain is finite,
normal f32 values in [-80, 80] plus zero; the other two reject nonfinite and
nonzero subnormal inputs. Inference must explicitly handle any wider domain.

The CSV was generated twice in a disposable Python environment using isolated
mode and no site packages. Python was 3.13.15, Windows AMD64, libmpdec 4.0.0,
with 120-digit Decimal precision. `exp` used Decimal's exponential, `tanh`
used `(exp(2x)-1)/(exp(2x)+1)`, and `erf` used the 320-term alternating
power series with a 100-digit pi constant. Inputs are rounded to binary32 first
and then represented exactly in Decimal; the decimal inputs are recorded in
the CSV and outputs have 45 significant digits. Both runs produced
LF-terminated bytes with SHA-256
`6aff402b1020a5f47bfcb08e90592478c54d3e461d429cde90311f0aa9ae3b42`.
The uncommitted generation script's SHA-256 was
`53f860bc61815a406cf973c9238ae19bb6badb28e9831c8aceab32b62f71f12f`.

The exact calculation components were pinned by SHA-256:

| Component | SHA-256 |
|---|---|
| venv python.exe | `72b29481593c5da37c99248c82777fbfb56217ea7809b771bc760d0a9ecb179b` |
| python313.dll | `e820bf024efd2b56bb2b82791e6b6ddc7303f070f8e72cba7637482a8a906238` |
| decimal.py | `66049e06703babdbb35e5a7e6de3dab6641d79b180b471a1acc64afab6a1ad6c` |
| struct.py | `eadbcc540c3b6496e52449e712eca3694e31e1d935af0f1e26cff0e3cc370945` |
| _decimal.pyd | `1e6a5707163d3c103e0eb4695eefedbea4b855fb227c61cc265b168970d6c6f0` |
| ucrtbase.dll | `dedb7611e1abc1ff3852dcac3d4c93a048d729e25b061973b9969f22698f14ca` |
| vcruntime140.dll | `d1f4225df2cd877dbf130d5668a021dce3f94118455ff5ec952061c30afc9ce7` |

The OS was Windows `10.0.26200.0`; the virtual environment used the base
installation's standard library, so this is not a fully hermetic image. A
fully pinned disposable environment is still required for model-reference
generation. The current CI compares math output bytes for all 1,264 fixture
inputs, plus signed zero and the smallest normal inputs, between its Windows
and Linux debug and release builds. This does not establish identical
results for every CPU, ISA, thread count, floating-point environment or input.
