#!/usr/bin/env python3
"""
Generates Gumroad Hero Cover (1280x720) and Square Thumbnail (1000x1000) for PhotonDiagram
and publishes PhotonDiagram v1.0.0 to Gumroad (https://shop.pietiniart.com/l/photondiagram)
with Pay-What-You-Want (€0+, suggested €5) pricing.
"""

import json
import os
import subprocess
import sys
import datetime
from PIL import Image, ImageDraw, ImageFont

PROJECT_DIR = "/Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_diagram"
DIST_DIR = os.path.join(PROJECT_DIR, "dist")
ASSETS_DIR = os.path.join(PROJECT_DIR, "assets")
TRACKER_FILE = "/Volumes/Dataa/EtsyTuotteet/gumroad_published.json"

COVER_PATH = os.path.join(DIST_DIR, "photondiagram_gumroad_cover.jpg")
THUMB_PATH = os.path.join(DIST_DIR, "photondiagram_gumroad_thumb.jpg")


def load_font(size, bold=False):
    candidates = (
        [
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ]
        if bold
        else [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ]
    )
    for c in candidates:
        if os.path.exists(c):
            try:
                return ImageFont.truetype(c, size)
            except Exception:
                pass
    return ImageFont.load_default()


def generate_images():
    os.makedirs(DIST_DIR, exist_ok=True)
    icon_path = os.path.join(ASSETS_DIR, "icon_1024.png")
    icon = Image.open(icon_path).convert("RGBA") if os.path.exists(icon_path) else None

    # 1. 1280x720 Hero Cover
    W, H = 1280, 720
    cover = Image.new("RGB", (W, H), (15, 23, 42))
    draw = ImageDraw.Draw(cover)

    # Subtle blueprint grid
    for gx in range(0, W, 32):
        col = (30, 41, 59) if (gx // 32) % 4 != 0 else (38, 52, 74)
        draw.line([(gx, 0), (gx, H)], fill=col, width=1)
    for gy in range(0, H, 32):
        col = (30, 41, 59) if (gy // 32) % 4 != 0 else (38, 52, 74)
        draw.line([(0, gy), (W, gy)], fill=col, width=1)

    # Decorative schematic trace accents on right side
    draw.line([(820, 120), (1180, 120)], fill=(220, 38, 38), width=4)
    draw.line([(820, 600), (1180, 600)], fill=(37, 99, 235), width=4)

    if icon:
        ic_resized = icon.resize((360, 360), Image.Resampling.LANCZOS)
        cover.paste(ic_resized, (840, 180), ic_resized)

    f_badge = load_font(20, bold=True)
    f_title = load_font(64, bold=True)
    f_sub = load_font(28, bold=False)
    f_feat = load_font(23, bold=False)
    f_plat = load_font(22, bold=True)

    # Top badge pill
    draw.rounded_rectangle([70, 68, 590, 114], radius=22, fill=(37, 99, 235))
    draw.text(
        (94, 79),
        "OPEN-SOURCE NATIVE CAD & VISIO (.VSDX) SUITE",
        fill=(255, 255, 255),
        font=f_badge,
    )

    draw.text((70, 145), "PhotonDiagram", fill=(255, 255, 255), font=f_title)
    draw.text(
        (70, 228),
        "Electrical Schematics, Floor Plans & Technical Diagrams",
        fill=(148, 163, 184),
        font=f_sub,
    )

    features = [
        "✓ Full Microsoft Visio (.vsdx, .vssx, .vdx, .vsd) Import & Export",
        "✓ IEC 60617 Electrical, Relay & PLC Automation Schematics",
        "✓ Smart Obstacle-Avoiding A* Wire Routing & Jump Bridges",
        "✓ Scaled Architectural Floor Plans (1:1 to 1:100) with Auto m²",
        "✓ Direct Export to AutoCAD (.dxf), LightBurn (.lbrn2), SVG & CSV",
    ]
    y = 310
    for ft in features:
        draw.text((75, y), ft, fill=(226, 232, 240), font=f_feat)
        y += 46

    # Bottom platform bar
    draw.rounded_rectangle([70, 575, 760, 635], radius=12, fill=(30, 41, 59), outline=(59, 130, 246), width=2)
    draw.text(
        (95, 593),
        "macOS (Universal M1–M4 & Intel)  •  Windows 10/11 x64  •  Linux x64",
        fill=(96, 165, 250),
        font=f_plat,
    )

    cover.save(COVER_PATH, "JPEG", quality=95)

    # 2. 1000x1000 Square Thumbnail
    TW, TH = 1000, 1000
    thumb = Image.new("RGB", (TW, TH), (15, 23, 42))
    tdraw = ImageDraw.Draw(thumb)
    for gx in range(0, TW, 40):
        tdraw.line([(gx, 0), (gx, TH)], fill=(30, 41, 59), width=1)
    for gy in range(0, TH, 40):
        tdraw.line([(0, gy), (TW, gy)], fill=(30, 41, 59), width=1)

    if icon:
        ic_sq = icon.resize((520, 520), Image.Resampling.LANCZOS)
        thumb.paste(ic_sq, ((TW - 520) // 2, 90), ic_sq)

    tf_title = load_font(68, bold=True)
    tf_sub = load_font(32, bold=True)
    tf_tag = load_font(26, bold=False)

    tdraw.text((195, 650), "PhotonDiagram", fill=(255, 255, 255), font=tf_title)
    tdraw.text(
        (135, 745),
        "Visio (.vsdx) • Electrical CAD • Floor Plans",
        fill=(96, 165, 250),
        font=tf_sub,
    )
    tdraw.rounded_rectangle([160, 830, 840, 895], radius=16, fill=(37, 99, 235))
    tdraw.text(
        (215, 848),
        "macOS  •  Windows  •  Linux  (Free / Open Source)",
        fill=(255, 255, 255),
        font=tf_tag,
    )

    thumb.save(THUMB_PATH, "JPEG", quality=95)
    print(f"[OK] Generated {COVER_PATH} and {THUMB_PATH}")


def publish_to_gumroad():
    generate_images()

    dmg_file = os.path.join(DIST_DIR, "PhotonDiagram-1.0.0-universal.dmg")
    win_file = os.path.join(DIST_DIR, "PhotonDiagram-1.0.0-windows-x64.zip")
    lin_file = os.path.join(DIST_DIR, "PhotonDiagram-1.0.0-linux-x86_64.tar.gz")

    html_desc = (
        "<h3>⚡ PhotonDiagram v1.0.0 — Fast Native Visio (.vsdx), Electrical Schematic & 2D CAD Suite</h3>"
        "<p><b>PhotonDiagram</b> is a lightweight, high-performance native desktop CAD and technical diagramming application engineered in Rust. "
        "It launches in a fraction of a second, renders at a smooth 60+ FPS, and requires zero subscriptions or cloud accounts.</p>"
        "<h3>🚀 Key Features</h3>"
        "<ul>"
        "<li><b>Full Microsoft Visio® Interoperability:</b> Open and save <code>.vsdx</code> drawings natively, import <code>.vssx</code> stencil libraries, and inspect <code>.vdx</code> / <code>.vsd</code> files on macOS, Windows, and Linux.</li>"
        "<li><b>Electrical & PLC Control Schematics (IEC 60617):</b> Built-in symbols for resistors, capacitors, inductors, relays, circuit breakers (MCB), motors, power supplies, and 8-pin PLC I/O blocks.</li>"
        "<li><b>Smart A* Orthogonal Wire Routing:</b> Automatically routes wires around components and pins with automatic semicircle wire jump bridges where wires cross.</li>"
        "<li><b>Architectural Floor Plans (1:1 to 1:100):</b> Draw scaled rooms with automatic real-time <code>m²</code> area calculation, walls, doors, windows, furniture, sauna heaters, and building electrical layouts.</li>"
        "<li><b>Direct CAD/CAM Exports:</b> Export to AutoCAD <code>.dxf</code>, LightBurn / PhotonCAM <code>.lbrn2</code> (for laser-cutting control panels & equipment tags), clean vector <code>.svg</code>, and Bill of Materials <code>.csv</code>.</li>"
        "<li><b>Bilingual Interface:</b> Switch between <b>English (EN)</b> and <b>Finnish (FI)</b> with one click in the top bar.</li>"
        "</ul>"
        "<h3>📦 Included Downloads (All Platforms)</h3>"
        "<ul>"
        "<li><b>macOS (Apple Silicon M1–M4 & Intel Universal):</b> <code>PhotonDiagram-1.0.0-universal.dmg</code></li>"
        "<li><b>Windows (10 / 11 64-bit Standalone):</b> <code>PhotonDiagram-1.0.0-windows-x64.zip</code></li>"
        "<li><b>Linux (x86_64 Standalone):</b> <code>PhotonDiagram-1.0.0-linux-x86_64.tar.gz</code></li>"
        "</ul>"
        "<p><i>Free & Open Source (GPL-3.0) — Pay what you want (€0+ to download for free, or leave a small tip to support independent open-source engineering software!).</i></p>"
    )

    cmd = [
        "gumroad", "products", "create",
        "--name", "PhotonDiagram – Native Visio (.vsdx), Electrical Schematic & 2D CAD Suite (macOS, Windows, Linux)",
        "--custom-permalink", "photondiagram",
        "--price", "0",
        "--pay-what-you-want",
        "--suggested-price", "5",
        "--currency", "eur",
        "--type", "digital",
        "--custom-summary", "Fast native Visio (.vsdx), IEC Electrical Schematic, PLC & Floor Plan CAD for macOS, Windows & Linux.",
        "--description", html_desc,
        "--cover-image", COVER_PATH,
        "--thumbnail", THUMB_PATH,
        "--file", dmg_file,
        "--file-name", "PhotonDiagram-1.0.0-universal.dmg",
        "--file", win_file,
        "--file-name", "PhotonDiagram-1.0.0-windows-x64.zip",
        "--file", lin_file,
        "--file-name", "PhotonDiagram-1.0.0-linux-x86_64.tar.gz",
        "--tag", "visio",
        "--tag", "cad",
        "--tag", "schematic",
        "--tag", "plc",
        "--tag", "floorplan",
        "--json",
        "--non-interactive",
    ]

    print("==> Running gumroad products create for PhotonDiagram...")
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode == 0:
        out_json = json.loads(res.stdout)
        prod_id = out_json.get("product", {}).get("id") or out_json.get("id")
        prod_url = out_json.get("product", {}).get("short_url") or out_json.get("short_url")
        if prod_id:
            subprocess.run(["gumroad", "products", "publish", prod_id], capture_output=True)
        custom_url = "https://shop.pietiniart.com/l/photondiagram"
        print(f"✅ LIVE ON GUMROAD: {custom_url} (id={prod_id})")

        if os.path.exists(TRACKER_FILE):
            with open(TRACKER_FILE, "r") as f:
                tracker = json.load(f)
            tracker["PhotonDiagram"] = {
                "id": prod_id,
                "url": prod_url,
                "custom_domain_url": custom_url,
                "title": "PhotonDiagram – Native Visio (.vsdx), Electrical Schematic & 2D CAD Suite (macOS, Windows, Linux)",
                "version": "1.0.0",
                "time": datetime.datetime.now().isoformat(),
            }
            with open(TRACKER_FILE, "w") as f:
                json.dump(tracker, f, indent=2)
        return 0
    else:
        err = (res.stdout + " " + res.stderr).strip()
        print(f"⚠️ Gumroad response: {err}")
        return 1


if __name__ == "__main__":
    sys.exit(publish_to_gumroad())
