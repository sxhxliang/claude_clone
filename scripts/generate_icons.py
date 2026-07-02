#!/usr/bin/env python3
from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "assets" / "icons"
PNG_DIR = ICON_DIR / "png"
BASE_SIZE = 1024


def lerp(a: int, b: int, t: float) -> int:
    return round(a + (b - a) * t)


def gradient(size: int, stops: list[tuple[float, tuple[int, int, int]]]) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = image.load()
    for y in range(size):
        for x in range(size):
            t = (x * 0.35 + y * 0.65) / (size - 1)
            for i, (stop, color) in enumerate(stops[1:], start=1):
                prev_stop, prev_color = stops[i - 1]
                if t <= stop:
                    local = 0.0 if stop == prev_stop else (t - prev_stop) / (stop - prev_stop)
                    px[x, y] = (
                        lerp(prev_color[0], color[0], local),
                        lerp(prev_color[1], color[1], local),
                        lerp(prev_color[2], color[2], local),
                        255,
                    )
                    break
            else:
                px[x, y] = (*stops[-1][1], 255)
    return image


def rounded_mask(size: int, radius: int, inset: int = 0) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle(
        (inset, inset, size - inset, size - inset),
        radius=radius,
        fill=255,
    )
    return mask


def paste_masked(base: Image.Image, layer: Image.Image, mask: Image.Image) -> None:
    base.alpha_composite(Image.composite(layer, Image.new("RGBA", layer.size, (0, 0, 0, 0)), mask))


def draw_icon() -> Image.Image:
    image = Image.new("RGBA", (BASE_SIZE, BASE_SIZE), (0, 0, 0, 0))

    bg = gradient(
        BASE_SIZE,
        [
            (0.0, (39, 50, 58)),
            (0.55, (27, 34, 40)),
            (1.0, (18, 23, 27)),
        ],
    )
    paste_masked(image, bg, rounded_mask(BASE_SIZE, 214, 56))

    border = Image.new("RGBA", (BASE_SIZE, BASE_SIZE), (0, 0, 0, 0))
    border_draw = ImageDraw.Draw(border)
    border_draw.rounded_rectangle((88, 88, 936, 936), radius=186, outline=(242, 221, 173, 36), width=8)
    image.alpha_composite(border)

    shadow = Image.new("RGBA", (BASE_SIZE, BASE_SIZE), (0, 0, 0, 0))
    sdraw = ImageDraw.Draw(shadow)
    bubble = [(132, 254), (892, 744)]
    sdraw.rounded_rectangle((132, 254, 892, 744), radius=116, fill=(0, 0, 0, 112))
    sdraw.polygon([(346, 704), (346, 833), (548, 704)], fill=(0, 0, 0, 112))
    shadow = shadow.filter(ImageFilter.GaussianBlur(34))
    image.alpha_composite(shadow, (0, 28))

    panel = Image.new("RGBA", (BASE_SIZE, BASE_SIZE), (0, 0, 0, 0))
    pdraw = ImageDraw.Draw(panel)
    panel_fill = gradient(
        BASE_SIZE,
        [
            (0.0, (255, 244, 220)),
            (1.0, (233, 220, 193)),
        ],
    )
    panel_mask = Image.new("L", (BASE_SIZE, BASE_SIZE), 0)
    mdraw = ImageDraw.Draw(panel_mask)
    mdraw.rounded_rectangle((132, 254, 892, 744), radius=116, fill=255)
    mdraw.polygon([(346, 704), (346, 833), (548, 704)], fill=255)
    panel.alpha_composite(Image.composite(panel_fill, Image.new("RGBA", panel_fill.size), panel_mask))
    image.alpha_composite(panel)

    dark = (43, 53, 59, 255)
    pdraw = ImageDraw.Draw(image)
    pdraw.line([(361, 449), (281, 512), (361, 575)], fill=dark, width=58, joint="curve")
    pdraw.line([(663, 449), (743, 512), (663, 575)], fill=dark, width=58, joint="curve")

    accent_layer = Image.new("RGBA", (BASE_SIZE, BASE_SIZE), (0, 0, 0, 0))
    adraw = ImageDraw.Draw(accent_layer)
    accent_grad = gradient(
        BASE_SIZE,
        [
            (0.0, (65, 184, 166)),
            (1.0, (217, 143, 69)),
        ],
    )
    arc_mask = Image.new("L", (BASE_SIZE, BASE_SIZE), 0)
    arc_draw = ImageDraw.Draw(arc_mask)
    arc_box = (307, 352, 635, 672)
    arc_draw.arc(arc_box, start=105, end=255, fill=255, width=78)
    for deg in (105, 255):
        radians = math.radians(deg)
        cx = (arc_box[0] + arc_box[2]) / 2 + math.cos(radians) * (arc_box[2] - arc_box[0]) / 2
        cy = (arc_box[1] + arc_box[3]) / 2 + math.sin(radians) * (arc_box[3] - arc_box[1]) / 2
        arc_draw.ellipse((cx - 39, cy - 39, cx + 39, cy + 39), fill=255)
    accent_layer.alpha_composite(Image.composite(accent_grad, Image.new("RGBA", accent_grad.size), arc_mask))
    image.alpha_composite(accent_layer)

    pdraw.ellipse((479, 478, 547, 546), fill=dark)
    pdraw.ellipse((576, 478, 644, 546), fill=dark)
    return image


def main() -> None:
    ICON_DIR.mkdir(parents=True, exist_ok=True)
    PNG_DIR.mkdir(parents=True, exist_ok=True)

    base = draw_icon()
    base.save(ICON_DIR / "claude_clone.png")

    sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
    resized = []
    for size in sizes:
        icon = base.resize((size, size), Image.Resampling.LANCZOS)
        icon.save(PNG_DIR / f"claude_clone-{size}.png")
        if size <= 256:
            resized.append(icon)

    resized[-1].save(
        ICON_DIR / "claude_clone.ico",
        sizes=[(icon.width, icon.height) for icon in resized],
        append_images=resized[:-1],
    )


if __name__ == "__main__":
    main()
