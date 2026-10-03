"""The 15 Camera Raw–style adjustments (spec section 5.3).

Everything here uses only ``backend.xp`` (NumPy or CuPy) plus the backend's blur,
box-filter and down/up-scale helpers, so the same code runs on CPU and GPU.

Two layers:

* **Reference functions** (``temperature`` … ``vignette``): one adjustment each, written
  exactly as the formulas of section 5.3. A value of 0 returns the input unchanged.
* **Fused group functions** (``pointwise_lut``, ``tone_local``, ``color``, ``detail``,
  ``effects``): mathematically the same pipeline, but with far fewer full-image passes
  (per-channel lookup table, combined luminance gains). ``core.pipeline`` uses these;
  ``tests/test_adjustments.py`` checks they match the reference functions.
"""

from __future__ import annotations

from typing import Any

from core.backend import Backend

LUMA = (0.2126, 0.7152, 0.0722)
GAIN_MAX = 8.0
NR_SUBSAMPLE_RADIUS = 3  # fast guided filter: radius on the downscaled grid


def luminance(img: Any) -> Any:
    return img[..., 0] * LUMA[0] + img[..., 1] * LUMA[1] + img[..., 2] * LUMA[2]


def smoothstep(xp: Any, a: float, b: float, x: Any) -> Any:
    t = xp.clip((x - a) / (b - a), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def luma_gain(xp: Any, lum: Any, lum_new: Any) -> Any:
    """Gain that turns ``lum`` into ``lum_new`` (section 5.1)."""
    return xp.clip(lum_new / xp.maximum(lum, 1e-4), 0.0, GAIN_MAX)


def apply_luma_gain(xp: Any, img: Any, lum: Any, lum_new: Any) -> Any:
    return img * luma_gain(xp, lum, lum_new)[..., None]


def long_side_of(img: Any) -> int:
    return int(max(img.shape[0], img.shape[1]))


# Reference functions ---------------------------------------------------------------------
# 1–2 White Balance


def temperature(img: Any, t: float, backend: Backend) -> Any:
    if t == 0:
        return img
    xp = backend.xp
    gains = xp.asarray([1 + 0.15 * t / 100, 1.0, 1 - 0.15 * t / 100], dtype=xp.float32)
    return img * gains


def tint(img: Any, m: float, backend: Backend) -> Any:
    if m == 0:
        return img
    xp = backend.xp
    gains = xp.asarray([1.0, 1 - 0.12 * m / 100, 1.0], dtype=xp.float32)
    return img * gains


# 3–9 Tone


def exposure(img: Any, e: float, backend: Backend) -> Any:
    if e == 0:
        return img
    xp = backend.xp
    x = xp.maximum(img, 0.0)
    lin = xp.where(x <= 0.04045, x / 12.92, ((x + 0.055) / 1.055) ** 2.4)
    lin = lin * xp.float32(2.0**e)
    out = xp.where(lin <= 0.0031308, 12.92 * lin, 1.055 * xp.maximum(lin, 0.0) ** (1 / 2.4) - 0.055)
    return out.astype(xp.float32)


def brightness(img: Any, b: float, backend: Backend) -> Any:
    if b == 0:
        return img
    xp = backend.xp
    return (xp.maximum(img, 0.0) ** xp.float32(2.0 ** (-b / 100))).astype(xp.float32)


def contrast(img: Any, c: float, backend: Backend) -> Any:
    if c == 0:
        return img
    return (img - 0.5) * backend.xp.float32(2.0 ** (c / 100)) + 0.5


def _hs_lum_new(xp: Any, lum: Any, h: float, s: float, backend: Backend, ls: int) -> Any:
    lb = backend.gaussian_blur(lum, 0.01 * ls)  # local brightness
    lum_new = lum
    if s != 0:
        lum_new = lum_new + 0.4 * (s / 100) * (1.0 - smoothstep(xp, 0.0, 0.5, lb))
    if h != 0:
        lum_new = lum_new + 0.4 * (h / 100) * smoothstep(xp, 0.5, 1.0, lb)
    return lum_new


def highlights_shadows(img: Any, h: float, s: float, backend: Backend, long_side: int | None = None) -> Any:
    """Highlights and Shadows, computed together from one local-brightness blur."""
    if h == 0 and s == 0:
        return img
    xp = backend.xp
    lum = luminance(img)
    return apply_luma_gain(xp, img, lum, _hs_lum_new(xp, lum, h, s, backend, long_side or long_side_of(img)))


def whites(img: Any, w: float, backend: Backend) -> Any:
    if w == 0:
        return img
    return img * backend.xp.float32(1.0 / (1 - 0.2 * w / 100))


def blacks(img: Any, k: float, backend: Backend) -> Any:
    if k == 0:
        return img
    bp = -0.1 * k / 100
    xp = backend.xp
    return (img - xp.float32(bp)) * xp.float32(1.0 / (1 - bp))


# 10–12 Presence


def _clarity_lum_new(xp: Any, lum: Any, c: float, backend: Backend, ls: int) -> Any:
    detail_ = lum - backend.gaussian_blur(lum, 0.015 * ls)
    mid = 1.0 - (2.0 * lum - 1.0) ** 2
    return lum + 0.8 * (c / 100) * detail_ * mid


def clarity(img: Any, c: float, backend: Backend, long_side: int | None = None) -> Any:
    if c == 0:
        return img
    xp = backend.xp
    lum = luminance(img)
    return apply_luma_gain(
        xp, img, lum, _clarity_lum_new(xp, lum, c, backend, long_side or long_side_of(img))
    )


def _channel_max_min(xp: Any, img: Any) -> tuple[Any, Any]:
    r, g, b = img[..., 0], img[..., 1], img[..., 2]
    return xp.maximum(xp.maximum(r, g), b), xp.minimum(xp.minimum(r, g), b)


def _vibrance_factor(xp: Any, img: Any, v: float) -> Any:
    mx, mn = _channel_max_min(xp, img)
    sat = (mx - mn) / xp.maximum(mx, 1e-4)
    return 1.0 + (v / 100) * (1.0 - sat)


def vibrance(img: Any, v: float, backend: Backend) -> Any:
    if v == 0:
        return img
    xp = backend.xp
    lum = luminance(img)[..., None]
    return lum + (img - lum) * _vibrance_factor(xp, img, v)[..., None]


def saturation(img: Any, s: float, backend: Backend) -> Any:
    if s == 0:
        return img
    lum = luminance(img)[..., None]
    return lum + (img - lum) * backend.xp.float32(1 + s / 100)


# 13–14 Detail


def _sharpen_lum_new(lum: Any, a: float, backend: Backend, ls: int) -> Any:
    sigma = max(0.5, ls / 4000)
    return lum + (a / 100) * (lum - backend.gaussian_blur(lum, sigma))


def sharpening(img: Any, a: float, backend: Backend, long_side: int | None = None) -> Any:
    if a == 0:
        return img
    lum = luminance(img)
    return apply_luma_gain(
        backend.xp, img, lum, _sharpen_lum_new(lum, a, backend, long_side or long_side_of(img))
    )


def guided_filter(backend: Backend, guide: Any, sources: list[Any], r: int, eps: float) -> list[Any]:
    """Guided filter of each source with one guide image (section 5.3 #14).

    For large radii the box means are computed on block-averaged copies and the
    coefficients ``a, b`` upscaled bilinearly (a "fast guided filter"), identically on
    both backends. The products ``I*I`` and ``I*p`` are formed at full resolution
    before averaging, so variance and covariance (and thus the strength) are preserved.
    """
    box = backend.box_filter
    f = max(1, r // NR_SUBSAMPLE_RADIUS)
    h, w = guide.shape
    if f > 1:
        down = lambda a: backend.downscale(a, f)  # noqa: E731
        r = max(1, int(round(r / f)))
    else:
        down = lambda a: a  # noqa: E731
    m_i = box(down(guide), r)
    var = box(down(guide * guide), r) - m_i * m_i
    out = []
    for p in sources:
        m_p = box(down(p), r)
        a = (box(down(guide * p), r) - m_i * m_p) / (var + eps)
        b = m_p - a * m_i
        a_m, b_m = box(a, r), box(b, r)
        if f > 1:
            a_m, b_m = backend.upscale(a_m, h, w, f), backend.upscale(b_m, h, w, f)
        out.append(a_m * guide + b_m)
    return out


def _nr_params(n: float, ls: int) -> tuple[int, float]:
    return max(1, int(round(0.002 * ls))), (0.03 * n / 100) ** 2


def noise_reduction(img: Any, n: float, backend: Backend, long_side: int | None = None) -> Any:
    """Guided-filter noise reduction: light on luminance, stronger on color noise."""
    if n == 0:
        return img
    xp = backend.xp
    r, eps = _nr_params(n, long_side or long_side_of(img))
    lum = luminance(img)
    [lum_dn] = guided_filter(backend, lum, [lum], r, eps)
    chroma = [img[..., k] - lum for k in range(3)]
    chroma_dn = guided_filter(backend, lum, chroma, 2 * r, 4 * eps)
    return xp.stack(chroma_dn, axis=2) + lum_dn[..., None]


# 15 Effects


def _vignette_gain(xp: Any, h: int, w: int, v: float) -> Any:
    y = (xp.arange(h, dtype=xp.float32) + 0.5) / h * 2 - 1
    x = (xp.arange(w, dtype=xp.float32) + 0.5) / w * 2 - 1
    r = xp.sqrt(y[:, None] ** 2 + x[None, :] ** 2) * xp.float32(2**-0.5)
    return 1.0 + 0.8 * (v / 100) * smoothstep(xp, 0.35, 1.0, r)


def vignette(img: Any, v: float, backend: Backend) -> Any:
    if v == 0:
        return img
    return img * _vignette_gain(backend.xp, img.shape[0], img.shape[1], v)[..., None]


# Fused group functions (used by the pipeline) ----------------------------------------------
# They work on a *planar* float32 image of shape (3, H, W): each channel is a contiguous
# plane, which makes every operation 5–10× faster on the CPU than on interleaved (H, W, 3)
# data. They take ownership of ``p`` and modify it in place; temporaries are reused where
# possible because every full-resolution pass costs memory bandwidth.

VIGNETTE_GRID = 8  # the smooth vignette gain is evaluated on a 1/8 grid and upscaled


def pointwise_active(s: Any) -> bool:
    return bool(s.temperature or s.tint or s.exposure or s.brightness or s.contrast)


def pointwise(img: Any, s: Any, backend: Backend) -> Any:
    """White Balance (+clip) → Exposure → Brightness → Contrast: all per-channel functions.

    Works on an (…, 3) array; used to build the lookup table and for float input.
    """
    xp = backend.xp
    if s.temperature or s.tint:
        img = tint(temperature(img, s.temperature, backend), s.tint, backend)
        img = xp.clip(img, 0.0, 1.0)
    img = exposure(img, s.exposure, backend)
    img = brightness(img, s.brightness, backend)
    return contrast(img, s.contrast, backend)


def pointwise_lut(s: Any, backend: Backend, levels: int) -> Any:
    """``(3, levels)`` table of :func:`pointwise` evaluated at every input code value."""
    xp = backend.xp
    grid = xp.linspace(0.0, 1.0, levels, dtype=xp.float32)
    grid = xp.repeat(grid[:, None], 3, axis=1)  # (levels, 3)
    return xp.ascontiguousarray(pointwise(grid, s, backend).T, dtype=xp.float32)


def to_planar(img: Any, s: Any, backend: Backend) -> Any:
    """(H, W, 3) input → planar float32 (3, H, W) with :func:`pointwise` applied.

    8/16-bit input goes through an exact per-channel lookup table.
    """
    xp = backend.xp
    h, w = img.shape[:2]
    levels = 256 if img.dtype == xp.uint8 else 65536 if img.dtype == xp.uint16 else None
    p = xp.empty((3, h, w), dtype=xp.float32)
    if levels is not None:
        lut = pointwise_lut(s, backend, levels)
        for c in range(3):
            p[c] = lut[c][xp.ascontiguousarray(img[..., c])]
        return p
    src = img.astype(xp.float32, copy=False)
    if pointwise_active(s):
        src = pointwise(src, s, backend)
    for c in range(3):
        p[c] = src[..., c]
    return p


def planar_to_hwc(p: Any, backend: Backend) -> Any:
    return backend.xp.ascontiguousarray(p.transpose(1, 2, 0))


def planar_to_uint8(p: Any, backend: Backend) -> Any:
    """Planar float in [0, 1] → (H, W, 3) uint8, rounded."""
    xp = backend.xp
    out = xp.empty((p.shape[1], p.shape[2], 3), dtype=xp.uint8)
    tmp = xp.empty(p.shape[1:], dtype=xp.float32)
    for c in range(3):
        xp.clip(p[c], 0.0, 1.0, out=tmp)
        tmp *= 255.0
        xp.rint(tmp, out=tmp)
        out[..., c] = tmp
    return out


def lum_planar(p: Any, backend: Backend) -> Any:
    xp = backend.xp
    lum = p[0] * xp.float32(LUMA[0])
    tmp = p[1] * xp.float32(LUMA[1])
    lum += tmp
    xp.multiply(p[2], xp.float32(LUMA[2]), out=tmp)
    lum += tmp
    return lum


def _smoothstep_(xp: Any, a: float, b: float, x: Any) -> Any:
    """In-place smoothstep (overwrites ``x``)."""
    x -= a
    x *= 1.0 / (b - a)
    xp.clip(x, 0.0, 1.0, out=x)
    u = x * -2.0
    u += 3.0
    x *= x
    x *= u
    return x


def _gain_(xp: Any, lum: Any, lum_new: Any) -> Any:
    """In-place :func:`luma_gain` (overwrites ``lum_new``)."""
    lum_new /= xp.maximum(lum, 1e-4)
    return xp.clip(lum_new, 0.0, GAIN_MAX, out=lum_new)


def _clip_planar(p: Any, backend: Backend) -> Any:
    return backend.xp.clip(p, 0.0, 1.0, out=p)


def _scale_planes(p: Any, gain: Any) -> None:
    for c in range(3):
        p[c] *= gain


def _hs_delta(xp: Any, lb: Any, h: float, s: float) -> Any:
    """``L_new - L`` of Highlights/Shadows as a function of the local brightness ``lb``."""
    delta = xp.zeros_like(lb)
    if s:
        k = 0.4 * (s / 100)
        shadow = _smoothstep_(xp, 0.0, 0.5, lb.copy())  # mask_shadow = 1 - shadow
        shadow *= -k
        shadow += k
        delta += shadow
    if h:
        highlight = _smoothstep_(xp, 0.5, 1.0, lb.copy())
        highlight *= 0.4 * (h / 100)
        delta += highlight
    return delta


def tone_local(p: Any, s: Any, backend: Backend, ls: int) -> Any:
    """Highlights + Shadows → Whites → Blacks → clip, as one gain and one offset."""
    xp = backend.xp
    scale, offset = 1.0, 0.0
    if s.whites:
        scale /= 1 - 0.2 * s.whites / 100
    if s.blacks:
        bp = -0.1 * s.blacks / 100
        scale /= 1 - bp
        offset = -bp / (1 - bp)
    if s.highlights or s.shadows:
        lum = lum_planar(p, backend)
        lum_new = backend.blur_map(lum, 0.01 * ls, lambda lb: _hs_delta(xp, lb, s.highlights, s.shadows))
        lum_new += lum
        gain = _gain_(xp, lum, lum_new)
        if scale != 1.0:
            gain *= xp.float32(scale)
        _scale_planes(p, gain)
    elif scale != 1.0:
        p *= xp.float32(scale)
    if offset:
        p += xp.float32(offset)
    return _clip_planar(p, backend)


def color(p: Any, s: Any, backend: Backend) -> Any:
    """Vibrance → Saturation → clip. Both keep luminance, so their factors multiply."""
    xp = backend.xp
    lum = lum_planar(p, backend)
    fs = 1 + s.saturation / 100
    factor: Any = xp.float32(fs)
    if s.vibrance:
        mx = xp.maximum(p[0], p[1])
        xp.maximum(mx, p[2], out=mx)
        mn = xp.minimum(p[0], p[1])
        xp.minimum(mn, p[2], out=mn)
        mn -= mx  # -(max - min)
        xp.maximum(mx, 1e-4, out=mx)
        mn /= mx  # -sat
        k = s.vibrance / 100
        mn *= k * fs
        mn += (1 + k) * fs  # (1 + k * (1 - sat)) * fs
        factor = mn
    for c in range(3):
        p[c] -= lum
        p[c] *= factor
        p[c] += lum
    return _clip_planar(p, backend)


def noise_reduction_planar(p: Any, n: float, backend: Backend, ls: int) -> Any:
    xp = backend.xp
    r, eps = _nr_params(n, ls)
    lum = lum_planar(p, backend)
    [lum_dn] = guided_filter(backend, lum, [lum], r, eps)
    chroma = [p[k] - lum for k in range(3)]
    chroma_dn = guided_filter(backend, lum, chroma, 2 * r, 4 * eps)
    for k in range(3):
        xp.add(chroma_dn[k], lum_dn, out=p[k])
    return p


def _clarity_gain(xp: Any, lum: Any, c: float, backend: Backend, ls: int) -> Any:
    detail_ = backend.gaussian_blur(lum, 0.015 * ls)
    xp.subtract(lum, detail_, out=detail_)  # L - blur(L)
    mid = lum * 2.0
    mid -= 1.0
    mid *= mid
    xp.subtract(1.0, mid, out=mid)  # 1 - (2L - 1)^2
    detail_ *= mid
    detail_ *= 0.8 * (c / 100)
    detail_ += lum
    return _gain_(xp, lum, detail_)


def _sharpen_gain(xp: Any, lum: Any, a: float, backend: Backend, ls: int) -> Any:
    blur = backend.gaussian_blur(lum, max(0.5, ls / 4000))
    blur = xp.subtract(lum, blur, out=blur if blur is not lum else None)
    blur *= a / 100
    blur += lum
    return _gain_(xp, lum, blur)


def detail(p: Any, s: Any, backend: Backend, ls: int) -> Any:
    """Noise Reduction → Clarity → Sharpening › Amount → clip.

    Clarity and Sharpening are luminance gains; luminance of ``rgb * g`` is ``L * g``,
    so both gains are computed on the luminance plane and applied to the color once.
    """
    xp = backend.xp
    if s.noise_reduction:
        p = noise_reduction_planar(p, s.noise_reduction, backend, ls)
    if s.clarity or s.sharpening_amount:
        lum = lum_planar(p, backend)
        gain = None
        if s.clarity:
            gain = _clarity_gain(xp, lum, s.clarity, backend, ls)
            lum *= gain
        if s.sharpening_amount:
            g2 = _sharpen_gain(xp, lum, s.sharpening_amount, backend, ls)
            gain = g2 if gain is None else xp.multiply(gain, g2, out=gain)
        _scale_planes(p, gain)
    return _clip_planar(p, backend)


def _vignette_gain_grid(xp: Any, h: int, w: int, v: float, f: int) -> Any:
    """Vignette gain sampled at the centers of ``f × f`` blocks (the grid of ``downscale``)."""
    gh, gw = -(-h // f), -(-w // f)
    y = ((xp.arange(gh, dtype=xp.float32) * f + (f - 1) / 2 + 0.5) / h) * 2 - 1
    x = ((xp.arange(gw, dtype=xp.float32) * f + (f - 1) / 2 + 0.5) / w) * 2 - 1
    r = xp.sqrt(y[:, None] ** 2 + x[None, :] ** 2) * xp.float32(2**-0.5)
    return (1.0 + 0.8 * (v / 100) * smoothstep(xp, 0.35, 1.0, r)).astype(xp.float32)


def effects(p: Any, s: Any, backend: Backend) -> Any:
    """Post-Crop Vignetting › Amount → clip."""
    h, w = p.shape[1:]
    f = max(1, min(VIGNETTE_GRID, min(h, w) // 400))  # grid of ≥ 400 px: error < 1e-4
    gain = _vignette_gain_grid(backend.xp, h, w, s.vignette_amount, f)
    if f > 1:
        gain = backend.upscale(gain, h, w, f)
    _scale_planes(p, gain)
    return _clip_planar(p, backend)
