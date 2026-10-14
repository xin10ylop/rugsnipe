"""Weighted statistics, bootstrap CIs and multiple-testing corrections."""
from __future__ import annotations

import math

import numpy as np


def wmean(r: np.ndarray, w: np.ndarray) -> float:
    return float(np.sum(r * w) / np.sum(w)) if np.sum(w) > 0 else float("nan")


def boot(r: np.ndarray, w: np.ndarray, B: int = 2000, seed: int = 7, ci=(0.05, 0.95)):
    """Bootstrap the weighted mean (resample launches). Returns (lo, hi, p_le0)."""
    m = len(r)
    if m < 2:
        return float("nan"), float("nan"), float("nan")
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, m, size=(B, m))
    ww = w[idx]
    means = (ww * r[idx]).sum(1) / ww.sum(1)
    lo, hi = np.quantile(means, ci)
    p = (np.sum(means <= 0) + 1) / (B + 1)
    return float(lo), float(hi), float(p)


def norm_sf(z: np.ndarray) -> np.ndarray:
    from math import erfc
    z = np.asarray(z, dtype=float)
    return 0.5 * np.vectorize(lambda x: erfc(x / math.sqrt(2)))(z) if z.ndim else 0.5 * erfc(float(z) / math.sqrt(2))


def norm_sf_fast(z: np.ndarray) -> np.ndarray:
    """Upper-tail normal probability, vectorised (Abramowitz-Stegun 7.1.26 via erfc approx)."""
    z = np.asarray(z, dtype=np.float64)
    x = np.abs(z) / math.sqrt(2)
    t = 1.0 / (1.0 + 0.3275911 * x)
    y = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))))
    erfc = y * np.exp(-x * x)
    return np.where(z >= 0, 0.5 * erfc, 1 - 0.5 * erfc)


def bh_qvalues(p: np.ndarray, m_total: int | None = None) -> np.ndarray:
    """Benjamini-Hochberg q-values. m_total >= len(p) counts untested configurations as p=1."""
    p = np.asarray(p, dtype=np.float64)
    m = m_total or len(p)
    order = np.argsort(p)
    ranked = p[order] * m / np.arange(1, len(p) + 1)
    q = np.minimum.accumulate(ranked[::-1])[::-1]
    out = np.empty_like(q)
    out[order] = np.minimum(q, 1.0)
    return out


def wilson(k: float, n: float, z: float = 1.96):
    if n <= 0:
        return float("nan"), float("nan")
    ph = k / n
    d = 1 + z * z / n
    c = (ph + z * z / (2 * n)) / d
    h = z * math.sqrt(ph * (1 - ph) / n + z * z / (4 * n * n)) / d
    return c - h, c + h
