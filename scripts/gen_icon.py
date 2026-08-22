#!/usr/bin/env python3
"""生成表情面板图标: 微信绿渐变圆角 + 白色微笑脸 → app.png / app.ico"""
from PIL import Image, ImageDraw, ImageFilter

S = 512
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# ---- 背景: 绿渐变圆角方块 (微信绿) ----
top, bot = (61, 216, 120), (18, 140, 90)  # #3DD878 -> #128C5A
m = 44
grad = Image.new("RGBA", (S, S), (0, 0, 0, 0))
for y in range(m, S - m):
    t = (y - m) / (S - 2 * m)
    c = tuple(int(top[i] + (bot[i] - top[i]) * t) for i in range(3))
    ImageDraw.Draw(grad).line([(m, y), (S - m, y)], fill=(*c, 255))
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([m, m, S - m, S - m], radius=110, fill=255)
img = Image.composite(grad, img, mask)

# ---- 底部软阴影 ----
sh = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(sh).ellipse([140, 420, 372, 462], fill=(0, 0, 0, 70))
sh = sh.filter(ImageFilter.GaussianBlur(12))
img = Image.alpha_composite(img, sh)

# ---- 白色圆脸 ----
face = (256, 232)
img2 = Image.new("RGBA", (S, S), (0, 0, 0, 0))
ImageDraw.Draw(img2).ellipse([face[0]-150, face[1]-150, face[0]+150, face[1]+150], fill=(255, 255, 255, 255))
img = Image.alpha_composite(img, img2)
d = ImageDraw.Draw(img)

green = (18, 140, 90)

# ---- 眼睛: 两点 ----
for ex in (198, 314):
    d.ellipse([ex-17, 196-17, ex+17, 196+17], fill=green)

# ---- 腮红 ----
for sxx in (122, 390):
    d.ellipse([sxx-22, 250-16, sxx+22, 250+16], fill=(255, 205, 130, 160))

# ---- 微笑: 弧线 ----
d.arc([176, 200, 336, 320], start=200, end=340, fill=green, width=24)

# ---- 顶部小高光 ----
d.ellipse([168, 96, 210, 138], fill=(255, 255, 255, 90))

img.save("egui-app-lite/assets/app.png")
img.save("egui-app-lite/assets/app.ico", sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])
print("icons written")