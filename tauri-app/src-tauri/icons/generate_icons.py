#!/usr/bin/env python3
"""
Generate macOS-compliant app icons with proper squircle transparency.

The white-corner issue on macOS happens when the icon image has a solid
square background. macOS .icns icons need transparent corners outside
the squircle (superellipse) shape.

This script:
1. Opens the source JPG/PNG
2. Resizes to 1024x1024
3. Applies a macOS-style squircle mask (transparent corners)
4. Saves as source PNG for `cargo tauri icon`
"""

from PIL import Image, ImageDraw
import math
import sys
import os


def create_squircle_mask(size: int) -> Image.Image:
    """
    Create a macOS-style squircle (superellipse) mask.

    Apple uses a superellipse with n ≈ 5.0 for app icons.
    The formula: |x/a|^n + |y/a|^n = 1
    where a = size/2 and n controls the corner roundness.
    """
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)

    center = size / 2
    # Apple's squircle uses n ≈ 5.0, which gives the distinctive
    # "rounded square" shape that's not a simple rounded rectangle
    n = 5.0

    # Generate the superellipse points
    points = []
    steps = 360
    for i in range(steps + 1):
        angle = 2 * math.pi * i / steps
        x = center + center * math.copysign(
            abs(math.cos(angle)) ** (2.0 / n), math.cos(angle)
        )
        y = center + center * math.copysign(
            abs(math.sin(angle)) ** (2.0 / n), math.sin(angle)
        )
        points.append((x, y))

    draw.polygon(points, fill=255)
    return mask


def process_icon(source_path: str, output_path: str, size: int = 1024):
    """Open source image, resize, and apply squircle mask."""
    img = Image.open(source_path).convert("RGBA")
    img = img.resize((size, size), Image.LANCZOS)

    mask = create_squircle_mask(size)
    result = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    result.paste(img, (0, 0), mask)
    result.save(output_path, "PNG")
    print(f"Generated: {output_path} ({size}x{size})")


if __name__ == "__main__":
    icons_dir = os.path.dirname(os.path.abspath(__file__))
    source = os.path.join(icons_dir, "new-icon-source.jpg")

    if len(sys.argv) > 1:
        source = sys.argv[1]

    if not os.path.exists(source):
        print(f"Error: Source image not found: {source}")
        sys.exit(1)

    output = os.path.join(icons_dir, "app-icon.png")
    process_icon(source, output)
    print(f"\nSource icon with squircle mask: {output}")
    print("Now run: cargo tauri icon app-icon.png")
