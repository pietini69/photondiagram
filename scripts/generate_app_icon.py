import math
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageEnhance
import os
import subprocess

def build_masterpiece():
    SIZE = 1024
    SCALE = 2
    W = SIZE * SCALE
    H = SIZE * SCALE
    
    # Apple HIG Squircle
    margin = 100 * SCALE
    tile_w = 824 * SCALE
    tile_h = 824 * SCALE
    cx = W / 2.0
    cy = H / 2.0
    rx = tile_w / 2.0
    ry = tile_h / 2.0
    n = 4.8
    
    y_coords, x_coords = np.ogrid[:H, :W]
    dx = np.abs((x_coords - cx) / rx)
    dy = np.abs((y_coords - cy) / ry)
    dist = dx**n + dy**n
    mask_np = np.clip(1.0 - (dist - 1.0) * (rx / 4.0), 0.0, 1.0)
    
    # 1. Base: Load AI Concept and fit into squircle
    ai_path = "/Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_diagram/assets/icon_ai_concept.png"
    ai_raw = Image.open(ai_path).convert("RGBA")
    # Enhance contrast and saturation slightly
    ai_raw = ImageEnhance.Contrast(ai_raw).enhance(1.15)
    ai_raw = ImageEnhance.Color(ai_raw).enhance(1.2)
    ai_scaled = ai_raw.resize((W, H), Image.Resampling.LANCZOS)
    
    # Darken outer regions slightly so the tile has a rich deep border
    darken = Image.new("RGBA", (W, H), (10, 15, 25, 0))
    dark_draw = ImageDraw.Draw(darken)
    # radial darkening towards edges of squircle
    rad = np.sqrt(((x_coords - cx)/rx)**2 + ((y_coords - cy)/ry)**2)
    dark_alpha = np.clip((rad - 0.4) * 1.4, 0.0, 0.6) * 255.0
    dark_arr = np.zeros((H, W, 4), dtype=np.uint8)
    dark_arr[:, :, 0] = 8
    dark_arr[:, :, 1] = 12
    dark_arr[:, :, 2] = 20
    dark_arr[:, :, 3] = dark_alpha.astype(np.uint8)
    dark_layer = Image.fromarray(dark_arr, mode="RGBA")
    
    base_tile = Image.alpha_composite(ai_scaled, dark_layer)
    
    # 2. Blueprint Technical CAD Grid Lines
    grid_img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    grid_draw = ImageDraw.Draw(grid_img)
    minor_grid = int(16 * SCALE)
    major_grid = int(48 * SCALE)
    
    for x in range(0, W, minor_grid):
        grid_draw.line([(x, 0), (x, H)], fill=(56, 189, 248, 10), width=1 * SCALE)
    for y in range(0, H, minor_grid):
        grid_draw.line([(0, y), (W, y)], fill=(56, 189, 248, 10), width=1 * SCALE)
        
    for x in range(0, W, major_grid):
        grid_draw.line([(x, 0), (x, H)], fill=(56, 189, 248, 25), width=int(1.5 * SCALE))
    for y in range(0, H, major_grid):
        grid_draw.line([(0, y), (W, y)], fill=(56, 189, 248, 25), width=int(1.5 * SCALE))
        
    # Crosshairs (+) at grid intersections
    for x in range(int(cx - rx) + major_grid, int(cx + rx), major_grid * 2):
        for y in range(int(cy - ry) + major_grid, int(cy + ry), major_grid * 2):
            cs = int(4 * SCALE)
            grid_draw.line([(x - cs, y), (x + cs, y)], fill=(56, 189, 248, 60), width=int(1.5 * SCALE))
            grid_draw.line([(x, y - cs), (x, y + cs)], fill=(56, 189, 248, 60), width=int(1.5 * SCALE))
            
    base_tile = Image.alpha_composite(base_tile, grid_img)
    
    # 3. Vector CAD Overlays:
    # Focus point
    fx = int(cx - 10 * SCALE)
    fy = int(cy + 20 * SCALE)
    
    vector_img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    vdraw = ImageDraw.Draw(vector_img)
    
    # Concentric Reticle & Angle Ticks
    for r in [int(90 * SCALE), int(135 * SCALE), int(180 * SCALE)]:
        vdraw.ellipse([fx - r, fy - r, fx + r, fy + r], outline=(56, 189, 248, 45), width=int(1.5 * SCALE))
        
    for deg in range(0, 360, 15):
        rad = math.radians(deg)
        r1 = 175 * SCALE
        r2 = (185 if deg % 45 == 0 else 180) * SCALE
        x1 = fx + r1 * math.cos(rad)
        y1 = fy + r1 * math.sin(rad)
        x2 = fx + r2 * math.cos(rad)
        y2 = fy + r2 * math.sin(rad)
        w = int(2 * SCALE) if deg % 45 == 0 else int(1.2 * SCALE)
        vdraw.line([(x1, y1), (x2, y2)], fill=(125, 211, 252, 90 if deg % 45 == 0 else 50), width=w)
        
    # 4. Schematic Branches (Emerging from focal center)
    # Upper Branch: Logic IC Block
    p1_start = (fx + int(40 * SCALE), fy - int(20 * SCALE))
    p1_mid = (fx + int(130 * SCALE), fy - int(170 * SCALE))
    p1_end = (fx + int(240 * SCALE), fy - int(170 * SCALE))
    
    vdraw.line([p1_start, p1_mid, p1_end], fill=(16, 185, 129, 70), width=int(10 * SCALE))
    vdraw.line([p1_start, p1_mid, p1_end], fill=(52, 211, 153, 190), width=int(4 * SCALE))
    vdraw.line([p1_start, p1_mid, p1_end], fill=(240, 253, 244, 255), width=int(1.5 * SCALE))
    
    # Logic Block
    c1_w = int(120 * SCALE)
    c1_h = int(68 * SCALE)
    c1_x0 = p1_end[0]
    c1_y0 = p1_end[1] - c1_h // 2
    vdraw.rounded_rectangle([c1_x0, c1_y0, c1_x0 + c1_w, c1_y0 + c1_h], radius=int(10*SCALE), fill=(6, 78, 59, 170), outline=(52, 211, 153, 240), width=int(2.5 * SCALE))
    vdraw.ellipse([c1_x0 - 5*SCALE, p1_end[1] - 5*SCALE, c1_x0 + 5*SCALE, p1_end[1] + 5*SCALE], fill=(255, 255, 255, 255))
    vdraw.line([(c1_x0 + 24*SCALE, p1_end[1]), (c1_x0 + c1_w - 24*SCALE, p1_end[1])], fill=(167, 243, 208, 190), width=int(2*SCALE))
    vdraw.line([(c1_x0 + 40*SCALE, p1_end[1] - 12*SCALE), (c1_x0 + 40*SCALE, p1_end[1] + 12*SCALE)], fill=(167, 243, 208, 190), width=int(2*SCALE))
    vdraw.line([(c1_x0 + c1_w - 40*SCALE, p1_end[1] - 12*SCALE), (c1_x0 + c1_w - 40*SCALE, p1_end[1] + 12*SCALE)], fill=(167, 243, 208, 190), width=int(2*SCALE))
    vdraw.line([(c1_x0 + c1_w, p1_end[1] - 14*SCALE), (c1_x0 + c1_w + 24*SCALE, p1_end[1] - 14*SCALE)], fill=(52, 211, 153, 220), width=int(2*SCALE))
    vdraw.line([(c1_x0 + c1_w, p1_end[1] + 14*SCALE), (c1_x0 + c1_w + 24*SCALE, p1_end[1] + 14*SCALE)], fill=(52, 211, 153, 220), width=int(2*SCALE))
    
    # Middle Branch: Bus & Diamond Decision Hub
    p2_start = (fx + int(45 * SCALE), fy + int(5 * SCALE))
    p2_end = (fx + int(260 * SCALE), fy + int(5 * SCALE))
    vdraw.line([p2_start, p2_end], fill=(14, 165, 233, 80), width=int(12 * SCALE))
    vdraw.line([p2_start, p2_end], fill=(56, 189, 248, 200), width=int(5 * SCALE))
    vdraw.line([p2_start, p2_end], fill=(255, 255, 255, 255), width=int(2 * SCALE))
    
    d_cx = p2_end[0] + int(42 * SCALE)
    d_cy = p2_end[1]
    d_size = int(36 * SCALE)
    diamond = [(d_cx, d_cy - d_size), (d_cx + d_size, d_cy), (d_cx, d_cy + d_size), (d_cx - d_size, d_cy)]
    vdraw.polygon(diamond, fill=(12, 74, 110, 180), outline=(56, 189, 248, 240), width=int(2.5 * SCALE))
    vdraw.ellipse([d_cx - 6*SCALE, d_cy - 6*SCALE, d_cx + 6*SCALE, d_cy + 6*SCALE], fill=(255, 255, 255, 255))
    vdraw.line([(d_cx + d_size, d_cy), (d_cx + d_size + 30*SCALE, d_cy)], fill=(56, 189, 248, 200), width=int(2.5*SCALE))
    vdraw.ellipse([d_cx + d_size + 30*SCALE - 4*SCALE, d_cy - 4*SCALE, d_cx + d_size + 30*SCALE + 4*SCALE, d_cy + 4*SCALE], fill=(56, 189, 248, 255))
    
    # Lower Branch: Amber/Gold Wave Terminal
    p3_start = (fx + int(35 * SCALE), fy + int(30 * SCALE))
    p3_mid = (fx + int(120 * SCALE), fy + int(175 * SCALE))
    p3_end = (fx + int(230 * SCALE), fy + int(175 * SCALE))
    vdraw.line([p3_start, p3_mid, p3_end], fill=(217, 119, 6, 70), width=int(10 * SCALE))
    vdraw.line([p3_start, p3_mid, p3_end], fill=(245, 158, 11, 190), width=int(4 * SCALE))
    vdraw.line([p3_start, p3_mid, p3_end], fill=(254, 243, 199, 255), width=int(1.5 * SCALE))
    
    c3_cx = p3_end[0] + int(42 * SCALE)
    c3_cy = p3_end[1]
    c3_r = int(32 * SCALE)
    vdraw.ellipse([c3_cx - c3_r, c3_cy - c3_r, c3_cx + c3_r, c3_cy + c3_r], fill=(120, 53, 15, 180), outline=(245, 158, 11, 240), width=int(2.5 * SCALE))
    vdraw.arc([c3_cx - 18*SCALE, c3_cy - 12*SCALE, c3_cx, c3_cy + 12*SCALE], start=180, end=0, fill=(254, 215, 170, 240), width=int(2*SCALE))
    vdraw.arc([c3_cx, c3_cy - 12*SCALE, c3_cx + 18*SCALE, c3_cy + 12*SCALE], start=180, end=0, fill=(254, 215, 170, 240), width=int(2*SCALE))
    
    # 5. Starburst Flare at focus point
    flare_img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    fdraw = ImageDraw.Draw(flare_img)
    fdraw.line([(fx - 150*SCALE, fy), (fx + 150*SCALE, fy)], fill=(255, 255, 255, 220), width=int(2.5 * SCALE))
    fdraw.line([(fx, fy - 120*SCALE), (fx, fy + 120*SCALE)], fill=(255, 255, 255, 220), width=int(2.5 * SCALE))
    d_flare = int(80 * SCALE)
    fdraw.line([(fx - d_flare, fy - d_flare), (fx + d_flare, fy + d_flare)], fill=(255, 255, 255, 180), width=int(2 * SCALE))
    fdraw.line([(fx - d_flare, fy + d_flare), (fx + d_flare, fy - d_flare)], fill=(255, 255, 255, 180), width=int(2 * SCALE))
    fdraw.ellipse([fx - 16*SCALE, fy - 16*SCALE, fx + 16*SCALE, fy + 16*SCALE], fill=(255, 255, 255, 255))
    fdraw.ellipse([fx - 28*SCALE, fy - 28*SCALE, fx + 28*SCALE, fy + 28*SCALE], outline=(186, 230, 253, 200), width=int(2*SCALE))
    
    # 6. Technical Monospace Typography
    anno_img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    adraw = ImageDraw.Draw(anno_img)
    try:
        font_cad = ImageFont.truetype("/System/Library/Fonts/SFCompact.ttf", int(15 * SCALE))
    except:
        try:
            font_cad = ImageFont.truetype("/System/Library/Fonts/Menlo.ttc", int(15 * SCALE))
        except:
            font_cad = ImageFont.load_default()
            
    adraw.text((int(cx - rx + 38*SCALE), int(cy - ry + 38*SCALE)), "PHOTON // OPTIC ENGINE", fill=(186, 230, 253, 140), font=font_cad)
    adraw.text((int(cx - rx + 38*SCALE), int(cy + ry - 54*SCALE)), "VSDX • CAD • SCHEMATIC", fill=(56, 189, 248, 120), font=font_cad)
    adraw.text((int(cx + rx - 180*SCALE), int(cy + ry - 54*SCALE)), "v1.0 • 64-BIT", fill=(56, 189, 248, 120), font=font_cad)
    
    # Composite all layers on tile
    tile = Image.alpha_composite(base_tile, vector_img)
    tile = Image.alpha_composite(tile, flare_img)
    tile = Image.alpha_composite(tile, anno_img)
    
    # Mask to Squircle
    tile_arr = np.array(tile, dtype=np.float32)
    tile_arr[:, :, 3] = tile_arr[:, :, 3] * mask_np
    
    # 7. Rim Light (White at top, subtle dark at bottom)
    rim_mask = np.clip(1.0 - np.abs(dist - 0.985) * (rx / 3.0), 0.0, 1.0)
    y_norm = (y_coords - cy) / ry
    top_rim_alpha = np.clip(-y_norm * 0.5 + 0.5, 0.0, 1.0) * rim_mask * 160.0
    rim_arr = np.zeros((H, W, 4), dtype=np.uint8)
    rim_arr[:, :, :3] = 255
    rim_arr[:, :, 3] = top_rim_alpha.astype(np.uint8)
    rim_img = Image.fromarray(rim_arr, mode="RGBA")
    
    # 8. Drop Shadow
    shadow_mask = np.clip(1.0 - (dist - 1.0) * (rx / 6.0), 0.0, 1.0)
    shadow_arr = np.zeros((H, W, 4), dtype=np.uint8)
    shadow_arr[:, :, 3] = (shadow_mask * 120.0).astype(np.uint8)
    shadow_img = Image.fromarray(shadow_arr, mode="RGBA")
    shadow_offset = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    shadow_offset.paste(shadow_img, (0, int(22 * SCALE)))
    shadow_offset = shadow_offset.filter(ImageFilter.GaussianBlur(radius=int(28 * SCALE)))
    
    # Final assembly
    final_canvas = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    final_canvas = Image.alpha_composite(final_canvas, shadow_offset)
    final_canvas = Image.alpha_composite(final_canvas, Image.fromarray(tile_arr.astype(np.uint8), mode="RGBA"))
    final_canvas = Image.alpha_composite(final_canvas, rim_img)
    
    # Downsample to 1024x1024 Lanczos
    master_1024 = final_canvas.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    
    out_dir = "/Volumes/Dataa/Ohjelmointi/ProjektitRust/photon_diagram/assets"
    master_path = os.path.join(out_dir, "icon_1024.png")
    master_1024.save(master_path, "PNG")
    print(f"🌟 Uusi mestariteos-ikoni tallennettu: {master_path}")
    
    # Also save 512, 256
    master_1024.resize((512, 512), Image.Resampling.LANCZOS).save(os.path.join(out_dir, "icon_512.png"), "PNG")
    master_1024.resize((256, 256), Image.Resampling.LANCZOS).save(os.path.join(out_dir, "icon_256.png"), "PNG")
    
    # Update iconset
    iconset_dir = os.path.join(out_dir, "PhotonDiagram.iconset")
    sizes = [
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ]
    for s, name in sizes:
        resized = master_1024.resize((s, s), Image.Resampling.LANCZOS)
        resized.save(os.path.join(iconset_dir, name), "PNG")
        
    icns_path = os.path.join(out_dir, "PhotonDiagram.icns")
    res = subprocess.run(["iconutil", "-c", "icns", iconset_dir, "-o", icns_path], capture_output=True, text=True)
    if res.returncode == 0:
        print(f"🚀 PhotonDiagram.icns päivitetty: {icns_path} ({os.path.getsize(icns_path)} tavua)")
    else:
        print(f"Virhe iconutil: {res.stderr}")

if __name__ == "__main__":
    build_masterpiece()
