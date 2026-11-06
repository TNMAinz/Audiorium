"""Fit a ten-band parametric EQ to a measured response, in AutoEq's file format.

Why this exists: AutoEq has no profile for the Moondrop LAN 2, so its two tunings are
fitted here from the published squig.link measurement, against the Harman IE 2019 target
those graphs are drawn to. Every candidate is evaluated with this app's own biquads (RBJ
cookbook, 48 kHz, shelves at S = 1), so the numbers written to disk are the numbers the
engine runs.

  python tools/fit_correction.py left.txt right.txt target.txt "Moondrop Lan 2 (Ref)" out.txt

The measurement is a REW text export (frequency, SPL, phase); the target is
(frequency, dB). Left and right are averaged, the measurement is smoothed to 1/12
octave, and the correction is target - measured, centred at 1 kHz. The fit is a low
shelf, a high shelf and eight peaking bands, refined one band at a time.
"""

import math
import sys

RATE = 48_000.0
MIN_HZ = 20.0
MAX_HZ = 20_000.0
GAIN_LIMIT = 6.0
PEAK_MAX_HZ = 9_000.0
REPORT_HZ = 6_000.0
SHELF_LOW = (60.0, 200.0)
SHELF_HIGH = (5_000.0, 12_000.0)
SPACING = 1.15
GRID = 300
BANDS = 10
PEAKS = BANDS - 2
QS = [0.4, 0.5, 0.6, 0.7, 0.85, 1.0, 1.2, 1.4, 1.7, 2.0, 2.4, 3.0, 3.6, 4.5]


def biquad(kind, frequency, gain_db, q):
    """The coefficients eq.rs builds, before the divide by a0."""
    f0 = min(max(frequency, MIN_HZ), RATE * 0.49)
    w0 = 2.0 * math.pi * f0 / RATE
    sin, cos = math.sin(w0), math.cos(w0)
    alpha = sin / (2.0 * q)
    a = 10.0 ** (gain_db / 40.0)
    shelf_alpha = sin / 2.0 * math.sqrt(2.0)
    if kind == "PK":
        return (1 + alpha * a, -2 * cos, 1 - alpha * a, 1 + alpha / a, -2 * cos, 1 - alpha / a)
    if kind == "LSC":
        return (
            a * ((a + 1) - (a - 1) * cos + 2 * math.sqrt(a) * shelf_alpha),
            2 * a * ((a - 1) - (a + 1) * cos),
            a * ((a + 1) - (a - 1) * cos - 2 * math.sqrt(a) * shelf_alpha),
            (a + 1) + (a - 1) * cos + 2 * math.sqrt(a) * shelf_alpha,
            -2 * ((a - 1) + (a + 1) * cos),
            (a + 1) + (a - 1) * cos - 2 * math.sqrt(a) * shelf_alpha,
        )
    if kind == "HSC":
        return (
            a * ((a + 1) + (a - 1) * cos + 2 * math.sqrt(a) * shelf_alpha),
            -2 * a * ((a - 1) + (a + 1) * cos),
            a * ((a + 1) + (a - 1) * cos - 2 * math.sqrt(a) * shelf_alpha),
            (a + 1) - (a - 1) * cos + 2 * math.sqrt(a) * shelf_alpha,
            2 * ((a - 1) - (a + 1) * cos),
            (a + 1) - (a - 1) * cos - 2 * math.sqrt(a) * shelf_alpha,
        )
    raise ValueError(kind)


def magnitude_db(coefficients, frequency):
    b0, b1, b2, a0, a1, a2 = coefficients
    b0, b1, b2, a1, a2 = b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0
    w = 2.0 * math.pi * min(max(frequency, 1.0), RATE * 0.4999) / RATE
    sin1, cos1 = math.sin(w), math.cos(w)
    sin2, cos2 = math.sin(2 * w), math.cos(2 * w)
    num = math.hypot(b0 + b1 * cos1 + b2 * cos2, -(b1 * sin1 + b2 * sin2))
    den = math.hypot(1.0 + a1 * cos1 + a2 * cos2, -(a1 * sin1 + a2 * sin2))
    if den <= 1e-12 or not math.isfinite(num):
        return 0.0
    return 20.0 * math.log10(max(num / den, 1e-12))


def read_pairs(path, columns=2):
    points = []
    with open(path, encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("*") or line.startswith("#"):
                continue
            parts = line.replace(",", "\t").split()
            if len(parts) < columns:
                continue
            try:
                points.append((float(parts[0]), float(parts[1])))
            except ValueError:
                continue
    return points


def interpolate(points, frequency):
    """Log-frequency linear interpolation, clamped at both ends."""
    if frequency <= points[0][0]:
        return points[0][1]
    if frequency >= points[-1][0]:
        return points[-1][1]
    low, high = 0, len(points) - 1
    while high - low > 1:
        middle = (low + high) // 2
        if points[middle][0] <= frequency:
            low = middle
        else:
            high = middle
    x0, y0 = points[low]
    x1, y1 = points[high]
    if x0 <= 0 or x1 == x0:
        return y0
    share = (math.log(frequency) - math.log(x0)) / (math.log(x1) - math.log(x0))
    return y0 + (y1 - y0) * share


def smooth(points, width=1 / 12):
    """Fractional-octave smoothing on the measurement's own grid."""
    smoothed = []
    for frequency, _ in points:
        low = frequency * 2 ** (-width / 2)
        high = frequency * 2 ** (width / 2)
        window = [value for f, value in points if low <= f <= high]
        smoothed.append((frequency, sum(window) / len(window) if window else 0.0))
    return smoothed


class Grid:
    """The shared trig for every candidate, so a fit takes seconds instead of minutes."""

    def __init__(self, wanted, weights):
        self.frequencies = [MIN_HZ * (MAX_HZ / MIN_HZ) ** (index / (GRID - 1)) for index in range(GRID)]
        self.wanted = wanted
        self.weights = weights
        self.trig = []
        for frequency in self.frequencies:
            w = 2.0 * math.pi * min(max(frequency, 1.0), RATE * 0.4999) / RATE
            self.trig.append((math.sin(w), math.cos(w), math.sin(2 * w), math.cos(2 * w)))

    def array(self, kind, fc, gain, q):
        b0, b1, b2, a0, a1, a2 = biquad(kind, fc, gain, q)
        b0, b1, b2, a1, a2 = b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0
        values = []
        for sin1, cos1, sin2, cos2 in self.trig:
            num = math.hypot(b0 + b1 * cos1 + b2 * cos2, -(b1 * sin1 + b2 * sin2))
            den = math.hypot(1.0 + a1 * cos1 + a2 * cos2, -(a1 * sin1 + a2 * sin2))
            values.append(20.0 * math.log10(max(num / den, 1e-12)) if den > 1e-12 else 0.0)
        return values

    def error(self, curve):
        return sum(w * (c - t) ** 2 for w, c, t in zip(self.weights, curve, self.wanted))

    def residual(self, curve):
        return [t - c for c, t in zip(curve, self.wanted)]

    def added(self, curve, values):
        return [c + v for c, v in zip(curve, values)]


def tame_treble(frequencies, wanted, width=1 / 6):
    """Fold the curve above 2 kHz into third-octave bands before the fit sees it."""
    smoothed = list(wanted)
    for index, frequency in enumerate(frequencies):
        if frequency < 2_000:
            continue
        low = frequency * 2 ** (-width / 2)
        high = frequency * 2 ** (width / 2)
        window = [wanted[other] for other, f in enumerate(frequencies) if low <= f <= high]
        smoothed[index] = sum(window) / len(window) if window else wanted[index]
    return smoothed


def clamp_gain(fc, gain):
    """No band above 6 kHz asks for more than 3 dB: a coupler peak is not a headphone peak."""
    limit = GAIN_LIMIT if fc <= 6_000 else 3.0
    return max(-limit, min(limit, gain))


def fit(measured, target_points):
    frequencies = [MIN_HZ * (MAX_HZ / MIN_HZ) ** (index / (GRID - 1)) for index in range(GRID)]
    target = [interpolate(target_points, f) for f in frequencies]
    measured_at = [interpolate(measured, f) for f in frequencies]
    # Centre the correction at 1 kHz: the midrange is the reference, so a shelf has to earn
    # its gain instead of every band being shifted by the overall measurement level.
    wanted = [t - m for t, m in zip(target, measured_at)]
    reference = interpolate(list(zip(frequencies, wanted)), 1_000.0)
    wanted = [value - reference for value in wanted]
    # The rig is trustworthy to about 6 kHz. Between 6 and 9 kHz a band may still take the
    # edge off a real peak, but at a third of the weight, so a coupler artefact cannot drag
    # the correction with it; above 9 kHz only the shelf's overall tilt is fitted.
    wanted = tame_treble(frequencies, wanted)
    weights = [1.0 if f <= 6_000 else 0.35 if f <= 9_000 else 0.02 for f in frequencies]
    grid = Grid(wanted, weights)

    filters = []
    curve = [0.0] * GRID

    def add(kind, candidates):
        """Add the candidate that leaves the smallest weighted error."""
        best = None
        for fc, gain, q in candidates:
            gain = clamp_gain(fc, gain)
            values = grid.array(kind, fc, gain, q)
            error = grid.error(grid.added(curve, values))
            if best is None or error < best[0]:
                best = (error, (kind, fc, round(gain * 2) / 2, q), values)
        filters.append(best[1])
        for index, value in enumerate(best[2]):
            curve[index] += value

    # The shelves take their gain from the region they cover, then search corner and Q.
    low_gain = sum(wanted[index] for index, f in enumerate(frequencies) if f <= 200) / max(
        1, sum(1 for f in frequencies if f <= 200)
    )
    treble = sorted(wanted[index] for index, f in enumerate(frequencies) if 6_000 <= f <= 12_000)
    high_gain = max(-3.0, min(3.0, treble[len(treble) // 2] if treble else 0.0))
    add("LSC", [(fc, low_gain, q) for fc in [MIN_HZ * (MAX_HZ / MIN_HZ) ** (i / 80) for i in range(81)] if SHELF_LOW[0] <= fc <= SHELF_LOW[1] for q in QS[:5]])
    add("HSC", [(fc, high_gain, q) for fc in [MIN_HZ * (MAX_HZ / MIN_HZ) ** (i / 80) for i in range(81)] if SHELF_HIGH[0] <= fc <= SHELF_HIGH[1] for q in QS[:5]])

    # Peaking bands: each one takes the worst remaining point as its centre.
    for _ in range(PEAKS):
        residual = grid.residual(curve)
        taken = [fc for kind, fc, _, _ in filters if kind == "PK"]
        # A band near one already placed would only repeat it, so the next band goes to the
        # worst point that no existing band covers.
        zone = 1.08
        usable = [
            index
            for index, f in enumerate(frequencies)
            if MIN_HZ <= f <= PEAK_MAX_HZ
            and not any(max(f, other) / min(f, other) < zone for other in taken)
        ]
        if not usable:
            continue
        worst = max(usable, key=lambda index: abs(residual[index]) * weights[index])
        centre_hz = frequencies[worst]
        gain_hint = residual[worst] / 2.0

        def candidates_for(spacing):
            options = []
            for ratio in [0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.25, 1.4, 1.6]:
                fc = min(max(centre_hz * ratio, 25.0), PEAK_MAX_HZ)
                if any(max(fc, other) / min(fc, other) < spacing for other in taken):
                    continue
                for q in QS:
                    for delta in (-1.0, -0.5, 0.0, 0.5, 1.0):
                        gain = gain_hint + delta
                        if abs(gain) < 0.5:
                            continue
                        options.append((fc, gain, q))
            return options

        options = candidates_for(SPACING) or candidates_for(1.0)
        if options:
            add("PK", options)

    # One band at a time, nudged, until nothing improves.
    for _ in range(8):
        improved = False
        for index in range(len(filters)):
            kind, fc, gain, q = filters[index]
            removed = grid.array(kind, fc, gain, q)
            without = [c - v for c, v in zip(curve, removed)]
            low, high = {
                "LSC": SHELF_LOW,
                "HSC": SHELF_HIGH,
            }.get(kind, (25.0, PEAK_MAX_HZ))
            best = (grid.error(curve), (kind, fc, gain, q), removed)
            for new_fc in [fc * factor for factor in (0.85, 0.92, 0.96, 1.0, 1.04, 1.08, 1.18)]:
                new_fc = min(max(new_fc, low), high)
                for new_q in QS:
                    for step in (-1.0, -0.5, -0.25, 0.0, 0.25, 0.5, 1.0):
                        new_gain = clamp_gain(new_fc, gain + step)
                        if abs(new_gain) > GAIN_LIMIT:
                            continue
                        values = grid.array(kind, new_fc, new_gain, new_q)
                        error = grid.error(grid.added(without, values))
                        if error < best[0] - 1e-9:
                            best = (error, (kind, new_fc, new_gain, new_q), values)
            filters[index] = best[1]
            curve = grid.added(without, best[2])
            if best[1] != (kind, fc, gain, q):
                improved = True
        if not improved:
            break
    # Every slot earns its place: a band that was refined down to nothing is replaced by
    # the band the remaining curve asks for most.
    def refill():
        nonlocal curve
        if len(filters) >= BANDS:
            return False
        residual = grid.residual(curve)
        taken = [fc for kind, fc, _, _ in filters if kind == "PK"]
        for zone in (SPACING,):
            usable = [
                index
                for index, f in enumerate(frequencies)
                if 25.0 <= f <= PEAK_MAX_HZ
                and not any(max(f, other) / min(f, other) < zone for other in taken)
            ]
            if not usable:
                continue
            worst = max(usable, key=lambda index: abs(residual[index]) * weights[index])
            fc = frequencies[worst]
            best = None
            for q in QS:
                for delta in (-0.5, 0.0, 0.5):
                    gain = clamp_gain(fc, residual[worst] / 2.0 + delta)
                    if abs(gain) < 0.5 or abs(gain) > GAIN_LIMIT:
                        continue
                    values = grid.array("PK", fc, gain, q)
                    error = grid.error(grid.added(curve, values))
                    if best is None or error < best[0]:
                        best = (error, ("PK", fc, gain, q), values)
            if best is None:
                continue
            filters.append(best[1])
            curve = grid.added(curve, best[2])
            return True
        return False

    # Two bands a hair apart only double the same move, and a cap on their gain means
    # nothing if a second band can ignore it.
    merged = True
    while merged:
        merged = False
        for index in range(len(filters)):
            for other in range(index):
                a, b = filters[index], filters[other]
                if a[0] != "PK" or b[0] != "PK":
                    continue
                if max(a[1], b[1]) / min(a[1], b[1]) >= 1.06:
                    continue
                weaker = index if abs(a[2]) < abs(b[2]) else other
                kind, fc, gain, q = filters.pop(weaker)
                removed = grid.array(kind, fc, gain, q)
                curve = [c - v for c, v in zip(curve, removed)]
                merged = True
                break
            if merged:
                break

    for _ in range(BANDS * 2):
        weak = [index for index, (kind, _, gain, _) in enumerate(filters) if kind == "PK" and abs(gain) < 0.5]
        if not weak:
            break
        kind, fc, gain, q = filters.pop(weak[-1])
        removed = grid.array(kind, fc, gain, q)
        curve = [c - v for c, v in zip(curve, removed)]
        if not refill():
            break
    while len(filters) < BANDS and refill():
        pass

    return filters, frequencies, wanted


def rounded(filters):
    return [(kind, int(round(fc)), round(gain * 2) / 2, round(q, 2)) for kind, fc, gain, q in filters]


def main():
    left_path, right_path, target_path, name, output_path = sys.argv[1:6]
    left = read_pairs(left_path, columns=3)
    right = read_pairs(right_path, columns=3)
    if len(right) != len(left):
        raise SystemExit("the two channels are not the same length")
    averaged = [(a[0], (a[1] + b[1]) / 2.0) for a, b in zip(left, right)]
    measured = smooth(averaged)
    target_points = read_pairs(target_path)
    filters, frequencies, wanted = fit(measured, target_points)
    filters = rounded(filters)
    gains = [0.0] * len(frequencies)
    for kind, fc, gain, q in filters:
        for index, frequency in enumerate(frequencies):
            gains[index] += magnitude_db(biquad(kind, fc, gain, q), frequency)
    residual = [g - w for g, w in zip(gains, wanted)]
    inside = [r for r, f in zip(residual, frequencies) if f <= REPORT_HZ]
    rms = math.sqrt(sum(r * r for r in inside) / len(inside))
    worst = max(abs(r) for r in inside)
    rms_all = math.sqrt(sum(r * r for r in residual) / len(residual))
    boost = max(
        sum(magnitude_db(biquad(kind, fc, gain, q), MIN_HZ * (MAX_HZ / MIN_HZ) ** (i / 400)) for kind, fc, gain, q in filters)
        for i in range(401)
    )
    preamp = round(-boost * 10) / 10
    lines = [f"Preamp: {preamp:.1f} dB"]
    for index, (kind, fc, gain, q) in enumerate(filters, start=1):
        lines.append(f"Filter {index}: ON {kind} Fc {fc} Hz Gain {gain:+.1f} dB Q {q:.2f}")
    with open(output_path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write("\n".join(lines) + "\n")
    print(
        f"{name}: {len(filters)} bands, preamp {preamp:+.1f} dB, "
        f"rms residual {rms:.2f} dB to {REPORT_HZ / 1000:.0f} kHz ({rms_all:.2f} dB full range), worst {worst:.2f} dB"
    )
    print("\n".join(lines))


if __name__ == "__main__":
    main()
