# 生成示例表情包素材 (PNG 静态 + GIF 动图), 放在 src-tauri/stickers 下
import os
import random
import struct
import zlib

BASE = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "stickers")

random.seed(42)

# ---------- 极简 PNG 编码器 (纯标准库, 无 Pillow 依赖) ----------
def chunk(tag: bytes, data: bytes) -> bytes:
    c = struct.pack(">I", len(data)) + tag + data
    return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

def write_png(path: str, size: int, pixels: list) -> None:
    # pixels: list of (r,g,b,a) rows*cols, top-left origin
    raw = b""
    for y in range(size):
        raw += b"\x00"
        for x in range(size):
            r, g, b, a = pixels[y * size + x]
            raw += bytes((r, g, b, a))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png)

def lzw_encode(indices, code_size):
    clear = 1 << code_size
    eoi = clear + 1
    out_bits = []

    def emit(code, w):
        for b in range(w):
            out_bits.append((code >> b) & 1)

    dict_ = {(c,): c for c in range(clear)}
    next_code = eoi + 1
    width = code_size + 1
    emit(clear, width)
    prev = (indices[0],)
    for i in indices[1:]:
        cur = prev + (i,)
        if cur in dict_:
            prev = cur
        else:
            emit(dict_[prev], width)
            if next_code < 4096:
                dict_[cur] = next_code
                next_code += 1
                if next_code == (1 << width) and width < 12:
                    width += 1
            else:
                emit(clear, width)
                dict_ = {(c,): c for c in range(clear)}
                next_code = eoi + 1
                width = code_size + 1
            prev = (i,)
    emit(dict_[prev], width)
    emit(eoi, width)
    while len(out_bits) % 8:
        out_bits.append(0)
    data = bytearray()
    for i in range(0, len(out_bits), 8):
        b = 0
        for j in range(8):
            b |= out_bits[i + j] << j
        data.append(b)
    return bytes(data)


def draw_gif(path: str, size: int, frames: list, duration: int = 120) -> None:
    """frames: list of pixel lists (same layout as png)"""
    from PIL import Image  # noqa: PLC0415

    imgs = []
    for px in frames:
        img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        img.putdata(px)
        imgs.append(img)
    imgs[0].save(
        path,
        save_all=True,
        append_images=imgs[1:],
        duration=duration,
        loop=0,
        transparency=0,
        disposal=2,
    )

def solid(size: int, color) -> list:
    return [tuple(color)] * (size * size)

def circle(size: int, cx: float, cy: float, r: float, color, bg=(0, 0, 0, 0)):
    px = []
    for y in range(size):
        for x in range(size):
            d = ((x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2) ** 0.5
            px.append(color if d <= r else bg)
    return px

def rect_px(size: int, x0, y0, x1, y1, color, bg=(0, 0, 0, 0)):
    px = []
    for y in range(size):
        for x in range(size):
            px.append(color if x0 <= x < x1 and y0 <= y < y1 else bg)
    return px

def blend(base, overlay):
    return [o if o[3] > 0 else b for b, o in zip(base, overlay)]

# 表情: 圆脸 + 五官
FACES = [
    # (脸底色, 眼睛类型: 0=点 1=线 2=弯, 嘴巴类型, 附加腮红)
    ((255, 214, 92, 255), 1, 1, True),   # 大笑
    ((255, 214, 92, 255), 1, 2, False),  # 微笑
    ((255, 214, 92, 255), 0, 3, False),  # 嘟嘴
    ((255, 200, 160, 255), 2, 4, True),  # 哭
    ((255, 170, 170, 255), 0, 1, False), # 开心粉
    ((255, 224, 130, 255), 1, 5, False), # 惊讶
    ((255, 214, 92, 255), 2, 2, False),  # 眯眼笑
    ((255, 190, 150, 255), 0, 0, False), # 平静
]

def face_px(size, face_c, eye_t, mouth_t, blush, t: float = 0):
    px = circle(size, size / 2, size / 2, size * 0.44, face_c)
    e = int(size * 0.10)
    ex = int(size * 0.30)
    ey = int(size * 0.40)
    dark = (60, 45, 30, 255)
    if eye_t == 0:
        for cx in (ex, size - ex):
            px = blend(px, circle(size, cx, ey, e * 0.9, dark))
    else:
        for cx in (ex, size - ex):
            px = blend(px, rect_px(size, cx - e, ey - 1, cx + e, ey + 1, dark))
    # 嘴巴
    my = int(size * 0.62)
    if mouth_t == 0:
        px = blend(px, circle(size, size / 2, my, e * 1.1, dark))
    elif mouth_t == 1:
        px = blend(px, rect_px(size, size / 2 - e * 1.2, my, size / 2 + e * 1.2, my + 2, dark))
    elif mouth_t == 2:
        px = blend(px, circle(size, size / 2, my + e, e * 1.3, (200, 80, 80, 255)))
    elif mouth_t == 3:
        px = blend(px, circle(size, size / 2, my + e, e * 0.9, dark))
    elif mouth_t == 4:
        px = blend(px, circle(size, size / 2, my + e * 1.6, e * 1.0, dark))
    if blush:
        for cx in (int(size * 0.22), size - int(size * 0.22)):
            px = blend(px, circle(size, cx, int(size * 0.58), e * 0.8, (255, 150, 150, 190)))
    return px

def balloon_px(size, color, letter, t):
    """气球跳跳: 上下弹 + 压扁"""
    phase = (t % 4) / 4
    bob = int((1 - abs(phase * 2 - 1)) * size * 0.12)
    squash = 1.0 - 0.08 * abs(phase * 2 - 1)
    px = [tuple((0, 0, 0, 0))] * (size * size)
    r = size * 0.30 * squash
    cy = size * 0.42 + bob
    px = blend(px, circle(size, size / 2, cy, r + 1, (0, 0, 0, 60)))
    px = blend(px, circle(size, size / 2, cy - 1, r, color))
    return px

def cat_px(size, seed):
    rnd = random.Random(seed)
    bg = (rnd.randint(200, 255), rnd.randint(200, 255), rnd.randint(200, 255), 255)
    px = [tuple(bg)] * (size * size)
    # 像素猫头 (12x12 网格, 放大)
    G = 12
    cell = size // G
    grid = [[0] * G for _ in range(G)]
    for y in range(G):
        for x in range(G):
            # 耳朵
            if (x in (2, 3) and y in (1, 2)) or (x in (8, 9) and y in (1, 2)):
                grid[y][x] = 1
            # 头
            elif 2 <= x <= 9 and 3 <= y <= 10:
                grid[y][x] = 1
            # 眼睛
            elif x in (4, 7) and y in (5, 6):
                grid[y][x] = 2
            # 嘴
            elif x in (5, 6) and y in (8, 9):
                grid[y][x] = 2
            # 条纹
            elif x in (5, 6) and y in (3, 4):
                grid[y][x] = 3
    fur = (rnd.randint(120, 200), rnd.randint(90, 160), rnd.randint(70, 130), 255)
    stripe = (rnd.randint(60, 100), rnd.randint(50, 80), rnd.randint(40, 70), 255)
    dark = (30, 25, 20, 255)
    for gy in range(G):
        for gx in range(G):
            c = grid[gy][gx]
            col = bg
            if c == 1:
                col = fur
            elif c == 2:
                col = dark
            elif c == 3:
                col = stripe
            for yy in range(gy * cell, (gy + 1) * cell):
                for xx in range(gx * cell, (gx + 1) * cell):
                    px[yy * size + xx] = tuple(col)
    return px

def heart_px(size, color, t):
    """复古像素爱心: 呼吸缩放"""
    s = 0.9 + 0.12 * abs(((t % 3) / 3) * 2 - 1)
    px = [tuple((0, 0, 0, 0))] * (size * size)
    H = 11
    cell = size // H
    heart = [
        ".XX...XX.",
        "XXXX.XXXX",
        "XXXXXXXXX",
        ".XXXXXXX.",
        "..XXXXX..",
        "...XXX...",
        "....X....",
    ]
    rows = len(heart)
    cols = len(heart[0])
    for gy in range(rows):
        for gx in range(cols):
            if heart[gy][gx] != "X":
                continue
            for yy in range(int(gy * cell), int((gy + 1) * cell)):
                for xx in range(int(gx * cell), int((gx + 1) * cell)):
                    px[yy * size + xx] = tuple(color)
    return px

def puppy_px(size, seed, t):
    """柴犬: 圆脸 + 耳朵 + 吐舌, 摇尾巴"""
    rnd = random.Random(seed)
    fur = (rnd.randint(210, 235), rnd.randint(160, 185), rnd.randint(100, 130), 255)
    cream = (rnd.randint(240, 255), rnd.randint(225, 240), rnd.randint(200, 215), 255)
    dark = (70, 50, 30, 255)
    px = [tuple((0, 0, 0, 0))] * (size * size)
    r = size * 0.36
    cy = size * 0.5
    px = blend(px, circle(size, size * 0.30, cy - r * 0.8, r * 0.32, fur))  # 左耳
    px = blend(px, circle(size, size * 0.70, cy - r * 0.8, r * 0.32, fur))  # 右耳
    px = blend(px, circle(size, size / 2, cy, r, fur))
    px = blend(px, circle(size, size * 0.38, cy + r * 0.5, r * 0.45, cream))  # 口
    px = blend(px, circle(size, size * 0.62, cy + r * 0.5, r * 0.45, cream))
    for cx in (size * 0.40, size * 0.60):
        px = blend(px, circle(size, cx, cy - r * 0.25, size * 0.045, dark))
    tongue_off = int(size * 0.05 * ((t % 2) // 1))
    px = blend(px, circle(size, size * 0.5, cy + r * 0.78 + tongue_off, size * 0.05, (255, 130, 130, 255)))
    return px

# ---------- 生成 ----------
def gen():
    S = 96
    os.makedirs(BASE, exist_ok=True)

    # 1) 基础表情 (PNG × 24)
    d = os.path.join(BASE, "samples", "基本表情")
    os.makedirs(d, exist_ok=True)
    for i in range(24):
        fc, et, mt, bl = FACES[i % len(FACES)]
        px = face_px(S, fc, et, mt, bl, t=i)
        write_png(os.path.join(d, f"{i+1:02d}.png"), S, px)

    # 2) 元气团子 (GIF × 16)
    d = os.path.join(BASE, "samples", "元气团子")
    os.makedirs(d, exist_ok=True)
    palette = [(255, 170, 90, 255), (120, 210, 140, 255), (110, 180, 255, 255),
               (255, 210, 120, 255), (230, 140, 200, 255)]
    for i in range(16):
        color = palette[i % len(palette)]
        frames = [balloon_px(S, color, chr(65 + i % 26), t) for t in range(4)]
        draw_gif(os.path.join(d, f"{i+1:02d}.gif"), S, frames, duration=110)

    # 3) 像素猫 (PNG × 20)
    d = os.path.join(BASE, "samples", "像素猫")
    os.makedirs(d, exist_ok=True)
    for i in range(20):
        write_png(os.path.join(d, f"cat{i+1:02d}.png"), S, cat_px(S, i))

    # 4) 商店: 柴犬日常 (GIF × 12)
    d = os.path.join(BASE, "shop", "柴犬日常")
    os.makedirs(d, exist_ok=True)
    for i in range(12):
        frames = [puppy_px(S, i, t) for t in range(3)]
        draw_gif(os.path.join(d, f"dog{i+1:02d}.gif"), S, frames, duration=150)

    # 5) 商店: 复古像素 (PNG × 16, 心形)
    d = os.path.join(BASE, "shop", "复古像素")
    os.makedirs(d, exist_ok=True)
    colors = [(255, 80, 120, 255), (120, 190, 255, 255), (120, 220, 160, 255),
              (255, 200, 80, 255)]
    for i in range(16):
        write_png(os.path.join(d, f"heart{i+1:02d}.png"), S, heart_px(S, colors[i % 4], i))

    print("done:", BASE)
    for root, dirs, files in os.walk(BASE):
        rel = os.path.relpath(root, BASE)
        counts = {}
        for f in files:
            counts[f.split(".")[-1]] = counts.get(f.split(".")[-1], 0) + 1
        if files:
            print(f"  {rel}: {dict(counts)}")

if __name__ == "__main__":
    gen()