"""Developer-only: export Real-ESRGAN ``realesr-general-x4v3`` to ONNX (spec section 5.4).

Needs ``torch`` (see ``tools/requirements-export.txt``); never bundled with the app.

    python tools/export_onnx.py [--out models/realesr-general-x4v3.onnx]

The network definition (SRVGGNetCompact) is copied from the Real-ESRGAN repository
(https://github.com/xinntao/Real-ESRGAN, BSD-3-Clause, Copyright (c) 2021 Xintao Wang).
"""

from __future__ import annotations

import argparse
import hashlib
import logging
import urllib.request
from pathlib import Path

import torch
from torch import nn
from torch.nn import functional as F

log = logging.getLogger('export_onnx')

WEIGHTS_URL = 'https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesr-general-x4v3.pth'
ROOT = Path(__file__).resolve().parent.parent


class SRVGGNetCompact(nn.Module):
    """Compact VGG-style network with pixel-shuffle upsampling (Real-ESRGAN)."""

    def __init__(
        self,
        num_in_ch: int = 3,
        num_out_ch: int = 3,
        num_feat: int = 64,
        num_conv: int = 32,
        upscale: int = 4,
    ) -> None:
        super().__init__()
        self.upscale = upscale
        self.body = nn.ModuleList()
        self.body.append(nn.Conv2d(num_in_ch, num_feat, 3, 1, 1))
        self.body.append(nn.PReLU(num_parameters=num_feat))
        for _ in range(num_conv):
            self.body.append(nn.Conv2d(num_feat, num_feat, 3, 1, 1))
            self.body.append(nn.PReLU(num_parameters=num_feat))
        self.body.append(nn.Conv2d(num_feat, num_out_ch * upscale * upscale, 3, 1, 1))
        self.upsampler = nn.PixelShuffle(upscale)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        out = x
        for layer in self.body:
            out = layer(out)
        out = self.upsampler(out)
        base = F.interpolate(x, scale_factor=self.upscale, mode='nearest')
        return out + base


def download(url: str, dest: Path) -> Path:
    if not dest.exists():
        log.info('Downloading %s', url)
        dest.parent.mkdir(parents=True, exist_ok=True)
        urllib.request.urlretrieve(url, dest)  # noqa: S310 - fixed https URL
    log.info('Weights sha256: %s', hashlib.sha256(dest.read_bytes()).hexdigest())
    return dest


def export(out_path: Path, weights: Path) -> None:
    net = SRVGGNetCompact()
    state = torch.load(weights, map_location='cpu', weights_only=True)
    net.load_state_dict(state.get('params', state), strict=True)
    net.eval()
    dummy = torch.rand(1, 3, 64, 64)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    kwargs = dict(
        input_names=['input'],
        output_names=['output'],
        opset_version=17,
        dynamic_axes={'input': {2: 'height', 3: 'width'}, 'output': {2: 'height_x4', 3: 'width_x4'}},
    )
    with torch.no_grad():
        try:
            torch.onnx.export(net, (dummy,), str(out_path), dynamo=False, **kwargs)
        except TypeError:  # older torch without the ``dynamo`` argument
            torch.onnx.export(net, (dummy,), str(out_path), **kwargs)
    log.info('Wrote %s (%.1f MB)', out_path, out_path.stat().st_size / 2**20)


def main() -> None:
    logging.basicConfig(level=logging.INFO, format='%(message)s')
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, default=ROOT / 'models' / 'realesr-general-x4v3.onnx')
    p.add_argument('--weights', type=Path, default=ROOT / 'tools' / '_cache' / 'realesr-general-x4v3.pth')
    args = p.parse_args()
    export(args.out, download(WEIGHTS_URL, args.weights))


if __name__ == '__main__':
    main()
