# Windows `scroll_until` Motion Step Type Validation (Brief 5)

Date: 2026-10-12  
Workspace: `F:\auv`  
Branch: `feat/windows-scroll-until-parity`  
Raw evidence: [`docs/ai/references/driver/2026-10-12-windows-scroll-until-motion.jsonl`](docs/ai/references/driver/2026-10-12-windows-scroll-until-motion.jsonl) (4 runs, 93 tick records)

---

## 1. Executive Summary & Goal

Brief 5 validates `ScrollUntilStep::Motion` (timed wheel scroll) on Windows via the shared default implementation (`run_window_scroll_motion` + `CumulativeQuantizer`), proving that:
1. Timed wheel motions deliver their quantized total without drift or truncation.
2. Fractional sub-notch remainders are properly preserved across sample ticks in `emitted_units`.
3. Dispatched sub-step counts and timing match the requested duration and easing curve at 60 Hz.
4. Viewport pixel displacement reflects the delivered delta on live DirectComposition surfaces under `InputPolicy::BackgroundOnly`.

---

## 2. Test Runs & Quantitative Results

Target: QQ Music (`QQMusic.exe`, HWND `1639628`, Process ID `21772`), Playlist View (`[playlist]`), Scroll Point `(500.0, 500.0)`, Region `--region 0.20,0.30,0.70,0.65`, Settle `50ms`, Policy `BackgroundOnly`.

| Run | Duration & Easing | Requested $\Delta y$ | Delivered $\Delta y$ | Quantization Error | Emitted Wheel Units | Sub-Steps Dispatched / Planned | Before Frame SHA256 | After Frame SHA256 | Normalized Diff | Viewport Verified |
| :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **1** | 300ms `linear` | $+120.0\text{ px}$ | $+120.000000\text{ px}$ | $0.000000\text{ px}$ | $+144\text{ units}$ (1.20 notches) | 18 / 19 | `40352d61...` | `0c10427d...` | $0.038129$ ($3.81\%$) | **YES** |
| **2** | 500ms `ease-out` | $+240.0\text{ px}$ | $+240.000000\text{ px}$ | $0.000000\text{ px}$ | $+288\text{ units}$ (2.40 notches) | 26 / 31 | `0c10427d...` | `70ac3caa...` | $0.033277$ ($3.33\%$) | **YES** |
| **3** | 400ms `ease-in-out` | $-120.0\text{ px}$ | $-120.000000\text{ px}$ | $0.000000\text{ px}$ | $-144\text{ units}$ (-1.20 notches) | 20 / 25 | `70ac3caa...` | `bdad928c...` | $0.040353$ ($4.04\%$) | **YES** |
| **4** | 350ms `ease-in` | $+121.0\text{ px}$ | $+120.833333\text{ px}$ | $0.166667\text{ px}$ | $+145\text{ units}$ (1.208 notches) | 18 / 22 | `bdad928c...` | `32156ead...` | $0.038198$ ($3.82\%$) | **YES** |

---

## 3. Mathematical Quantization Proof

### 3.1 Win32 Wheel Quantum
In `auv-driver-windows`:
- `SCROLL_PIXELS_PER_WHEEL_NOTCH = 100.0`
- `WHEEL_DELTA_PER_NOTCH = 120.0`
- `scroll_quantum = SCROLL_PIXELS_PER_WHEEL_UNIT = 100.0 / 120.0 = 5/6 \approx 0.833333\text{ px}`.

### 3.2 Error Bound
For any requested total $Y \in \mathbb{R}$:
$$\text{final\_units} = \left\lfloor Y \times \frac{120}{100} + 0.5 \right\rfloor = \text{round}(Y \times 1.2)$$
$$\text{delivered} = \text{final\_units} \times \frac{5}{6}\text{ px}$$
$$|Y - \text{delivered}| \le \frac{1}{2} \times \text{quantum} = \frac{5}{12}\text{ px} \approx 0.416667\text{ px} < 1\text{ quantum} (0.833333\text{ px})$$

- When $Y$ is a multiple of $5/6\text{ px}$ (such as $100.0$, $120.0$, $240.0$), the delivered total is **mathematically exact** ($0.0\text{ px}$ error), as demonstrated by Runs 1, 2, and 3.
- When $Y$ is an arbitrary non-multiple (such as $121.0\text{ px}$ in Run 4):
  $$\text{round}(121.0 \times 1.2) = \text{round}(145.2) = 145\text{ units}$$
  $$\text{delivered} = 145 \times \frac{5}{6} = 120.833333\text{ px}$$
  $$\text{error} = |121.0 - 120.833333| = 0.166667\text{ px} = \frac{1}{6}\text{ px} \le \frac{5}{12}\text{ px}$$
  The delivered total matches within quantization exactly as dictated by the formula.

### 3.3 Sub-Step Preservation
In `CumulativeQuantizer`:
- Intermediate ticks compute $target\_units_k = \text{round}(Y \times f(t_k) \times 1.2)$.
- Step delta is $\Delta units_k = target\_units_k - emitted\_units_{k-1}$.
- `emitted_units` preserves the cumulative integer total across all samples.
- Dispatched sub-steps dispatch $\Delta units_k$ via `WM_MOUSEWHEEL`.
- Total delivered across all sub-steps equals $\sum \Delta units_k = final\_units$. Zero truncation or remainder drift occurs.

---

## 4. Sub-Step Dispatched Profile by Easing

- **Linear (Run 1, 300ms, 19 ticks)**: Constant step of 8 wheel units ($6.67\text{ px}$) per sample from tick 1 to 18 ($18 \times 8 = 144$ units).
- **Ease-Out (Run 2, 500ms, 31 ticks)**: Front-loaded dispatch starting at 28 units ($23.33\text{ px}$) per sample, smoothly decaying to 1 unit ($0.83\text{ px}$) at tick 25, tapering to 0 as progress hits $1.0$.
- **Ease-In-Out (Run 3, 400ms, 25 ticks)**: Symmetric S-curve starting at 1 unit, peaking at 14 units at midpoint ($t = 200\text{ ms}$), smoothly tapering back to 1 unit.
- **Ease-In (Run 4, 350ms, 22 ticks)**: Quadratic/cubic progression; ticks 0–3 emit 0 units as $t^3$ progress remains below $1\text{ unit}$, accelerating to 20 units at the final sample.

---

## 5. Viewport Displacement Verification

All 4 runs were verified against live WGC frames:
- **Run 1**: Frame SHA changed from `40352d61...` to `0c10427d...`, normalized diff $3.81\%$ (displaced playlist downwards by ~1 song row).
- **Run 2**: Frame SHA changed from `0c10427d...` to `70ac3caa...`, normalized diff $3.33\%$ (displaced playlist downwards by ~2 song rows).
- **Run 3**: Frame SHA changed from `70ac3caa...` to `bdad928c...`, normalized diff $4.04\%$ (displaced playlist upwards by ~1 song row).
- **Run 4**: Frame SHA changed from `bdad928c...` to `32156ead...`, normalized diff $3.82\%$ (displaced playlist downwards by ~1 song row).

All runs passed the visual displacement verification check (`displacement_verified: true`).

---

## 6. Handoff Note for Brief 6

1. **Quantization Behavior**:
   - On Windows, `WHEEL_DELTA = 120` produces a quantum of `100.0 / 120.0 = 5/6 px` ($\approx 0.8333\text{ px}$).
   - `CumulativeQuantizer` discretizes directly in whole integer Win32 wheel units, guaranteeing that the delivered total matches the requested total within $\le 5/12\text{ px} \approx 0.4167\text{ px} < 1\text{ quantum}$.
   - Remainder tracking in `emitted_units` ensures zero cumulative drift regardless of duration or sample rate.

2. **Discretization Comparison (Windows vs macOS)**:
   - On macOS, `CGEventCreateScrollWheelEvent2` accepts continuous line/pixel values, but discrete notch wheels behave similarly.
   - On Windows, applications supporting high-resolution scrolling (such as DirectComposition in QQ Music) accept and smoothly render sub-notch wheel messages (e.g. 8 or 14 units) without waiting for 120 units to accumulate.
   - There are **no surprises**: Windows wheel discretization delivers smooth, predictable timed gestures with exact total preservation.

3. **Input Delivery Constraint**:
   - 100% of runs operated strictly under `InputPolicy::BackgroundOnly` with zero cursor movement, zero focus stealing, and zero window restoration.
