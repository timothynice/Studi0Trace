"""Renders the text fixture set: three OFL fonts, three strings, three cap heights, two trackings, two
backdrops, with the truth of every rendering in truth.json. Glyphs are drawn one at a time at the font's own
unhinted advances and no kerning, as the app shapes them (kern off), and anti-aliased by area."""
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
FONTS = [("JosefinSans-Light.ttf", "Josefin Sans", 300), ("Montserrat-Regular.ttf", "Montserrat", 400), ("PlayfairDisplay-Regular.ttf", "Playfair Display", 400)]
STRINGS = ["EST. 2026", "LIFE IS BETTER OUTSIDE", "Ideas for a brighter tomorrow"]
CAPS = [12.0, 18.0, 30.0]
TRACKINGS = [0.0, 0.2]  # em
INK = (24, 28, 44)


def cap_ratio(path: str) -> float:
    """Cap height over em, measured on H at 1000 px."""
    f = ImageFont.truetype(path, 1000)
    _, top, _, bottom = f.getbbox("H", anchor="ls")
    return (bottom - top) / 1000.0


SUPER = 8  # the glyphs are drawn at 8x and averaged down, so a pixel's coverage is its area, as a rasteriser's is


def render(font_path, cap_px, tracking_em, string, backdrop):
    size = cap_px / cap_ratio(font_path)
    tracking_px = tracking_em * size
    # the advances are the font's own, scaled: FreeType's hinted advances at the target size are whole pixels,
    # which would move every glyph by up to half a pixel from where the app's unhinted shaping puts it
    em = ImageFont.truetype(font_path, 1000)
    advances = [em.getlength(ch) * size / 1000.0 for ch in string]
    width = int(round(sum(advances) + tracking_px * (len(string) - 1))) + 60
    height = int(round(cap_px)) + 40
    img = Image.new("RGB", (width, height), (255, 255, 255))
    if backdrop == "gradient":
        px = img.load()
        for y in range(height):
            for x in range(width):
                t = x / max(1, width - 1)
                px[x, y] = (int(255 - 40 * t), int(250 - 20 * t), int(245 + 10 * t))
    big = ImageFont.truetype(font_path, size * SUPER)
    mask = Image.new("L", (width * SUPER, height * SUPER), 0)
    draw = ImageDraw.Draw(mask)
    x, baseline = 30.0, cap_px + 20.0
    for ch, adv in zip(string, advances):
        draw.text((x * SUPER, baseline * SUPER), ch, font=big, fill=255, anchor="ls")
        x += adv + tracking_px
    mask = mask.resize((width, height), Image.BOX)
    img = Image.composite(Image.new("RGB", (width, height), INK), img, mask)
    return img, size, baseline, tracking_px


def main():
    truth = []
    for file, family, weight in FONTS:
        for string in STRINGS:
            for cap in CAPS:
                for tr in TRACKINGS:
                    for backdrop in ("white", "gradient"):
                        slug = f"{family.split()[0].lower()}-{string.split()[0].lower().strip('.')}-{int(cap)}-t{int(tr * 10)}-{backdrop}.png"
                        img, size, baseline, tracking_px = render(str(HERE / "fonts" / file), cap, tr, string, backdrop)
                        img.save(HERE / slug)
                        truth.append({"file": slug, "font": file, "family": family, "weight": weight, "string": string, "cap_px": cap, "size_px": size, "baseline": baseline, "x": 30.0, "tracking_px": tracking_px, "backdrop": backdrop, "colour": list(INK)})
    (HERE / "truth.json").write_text(json.dumps(truth, indent=1))
    print(len(truth), "items")


if __name__ == "__main__":
    main()
