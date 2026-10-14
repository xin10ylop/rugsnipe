"""Hit test for a 0.5 SOL snipe on a SOL-quoted pump.fun curve (constant product)."""
import math

K = 30.0 * 1.073e9          # vSOL * vTOK at creation (SOL x tokens), SOL-quoted coins
FEE = 0.0125                # 0.95 % protocol + 0.30 % creator fee, each side
INIT_MCAP = 27.959          # SOL, curve floor


def reserves(mcap):
    p = mcap / 1e9
    return math.sqrt(K * p), math.sqrt(K / p)


def fill(entry_mcap, size=0.5):
    """Tokens bought for `size` SOL starting at spot `entry_mcap`."""
    v, t = reserves(entry_mcap)
    s = size * (1 - FEE)
    return t - K / (v + s)


def proceeds(mcap, q):
    v, t = reserves(mcap)
    return (v - K / (t + q)) * (1 - FEE)


def hit(entry_mcap, max_mcap, x, size=0.5):
    """True if selling the whole fill at the post-entry max nets >= (1+x)*size."""
    if not entry_mcap or not max_mcap or max_mcap <= entry_mcap:
        return False
    q = fill(entry_mcap, size)
    return proceeds(max_mcap, q) >= (1 + x) * size


def floor_loss(entry_mcap, size=0.5):
    q = fill(entry_mcap, size)
    return 1 - proceeds(INIT_MCAP, q) / size


def required_mult(entry_mcap, x, size=0.5):
    """Spot multiple over entry needed for a net +x (binary search)."""
    lo, hi = 1.0, 20.0
    for _ in range(50):
        mid = (lo + hi) / 2
        if hit(entry_mcap, entry_mcap * mid, x, size):
            hi = mid
        else:
            lo = mid
    return hi


def wilson(k, n, z=1.96):
    if n == 0:
        return (0.0, 0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, max(0.0, c - h), min(1.0, c + h))


if __name__ == "__main__":
    for e in (27.96, 40, 60, 100):
        print(e, [round(required_mult(e, x), 3) for x in (1.0, 1.5, 2.0)], round(floor_loss(e), 3))
