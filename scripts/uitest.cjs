// 前端 UI 端到端测试: 真实 Edge 渲染 dist/, mock __TAURI_INTERNALS__ 桥, 驱动全部交互
const http = require("http");
const fs = require("fs");
const path = require("path");

const DIST = path.join(__dirname, "..", "dist");
const MOCK = `
<script>
window.__MOCK__ = {
  calls: [],
  png: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
  pkgs: [
    { name: "基本表情", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "元气团子", cover: null, count: 16, gifCount: 16, shop: false },
    { name: "像素猫", cover: null, count: 20, gifCount: 0, shop: false }
  ],
  counts: { "基本表情": 24, "元气团子": 16, "像素猫": 20 },
  shop: [{ name: "柴犬日常", cover: null, count: 12, gifCount: 12, shop: true }],
  installed: [],
  target: null,
  picking: false
};
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args = {}) => {
    window.__MOCK__.calls.push([cmd, args]);
    const M = window.__MOCK__;
    switch (cmd) {
      case "list_packages": return M.pkgs;
      case "get_root": return "C:/mock/stickers";
      case "list_stickers": {
        const n = M.counts[args.package] != null ? M.counts[args.package] : 24;
        return Array.from({ length: n }, (_, i) => ({
          url: "C:/mock/stickers/" + args.package + "/" + String(i + 1).padStart(2, "0") + ".png",
          name: String(i + 1),
          isGif: args.package === "元气团子"
        }));
      }
      case "read_sticker": return M.png;
      case "shop_list": return M.shop;
      case "install_package": M.installed.push(args.name); M.pkgs.push({ name: args.name, cover: null, count: 12, gifCount: 12, shop: false }); return null;
      case "delete_package": M.pkgs = M.pkgs.filter((p) => p.name !== args.name); return null;
      case "reveal_root": return null;
      case "get_target": return M.target;
      case "is_picking": return M.picking;
      case "begin_pick": M.picking = true; return null;
      case "cancel_pick": M.picking = false; return null;
      case "insert_sticker": M.inserted = (M.inserted || []).concat([args.path]); return null;
      default: return null;
    }
  }
};
</script>`;

const server = http.createServer((req, res) => {
  let p = decodeURIComponent(req.url.split("?")[0]);
  if (p === "/") p = "/index.html";
  const file = path.join(DIST, p);
  if (!file.startsWith(DIST)) { res.writeHead(403); return res.end(); }
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404); return res.end("nf"); }
    const ct = file.endsWith(".html") ? "text/html" : file.endsWith(".js") ? "application/javascript" : file.endsWith(".css") ? "text/css" : "application/octet-stream";
    let body = data;
    if (file.endsWith(".html")) body = Buffer.from(data.toString("utf8").replace("<head>", "<head>" + MOCK));
    res.writeHead(200, { "Content-Type": ct });
    res.end(body);
  });
});

let failCount = 0;
function check(name, ok, extra) {
  console.log((ok ? "PASS " : "FAIL ") + name + (ok ? "" : "  " + (extra || "")));
  if (!ok) failCount++;
}

(async () => {
  await new Promise((r) => server.listen(8899, r));
  const { chromium } = require("D:/dev/playwright-mcp/node_modules/playwright");
  const browser = await chromium.launch({ channel: "msedge", headless: true });
  const page = await browser.newPage({ viewport: { width: 300, height: 340 } });
  page.on("pageerror", (e) => console.log("PAGEERROR:", e.message));

  await page.goto("http://127.0.0.1:8899/", { waitUntil: "load" });
  await page.waitForSelector(".tab");

  // 1. 无输入框 (用户要求去掉)
  const hasInputBar = await page.locator(".input-bar").count();
  check("no input bar", hasInputBar === 0, `count=${hasInputBar}`);

  // 2. 初始渲染: 3 分组; 4 列网格显示全部 24 张 (不分页); 表情包名
  check("tabs==3", (await page.locator(".tab").count()) === 3);
  check("grid cells==24 (all, no pagination)", (await page.locator(".stk-cell").count()) === 24);
  check("no page dots", (await page.locator(".page-dots").count()) === 0);
  check("group name shown", (await page.locator("#groupName").textContent()) === "基本表情 · 24 个");
  const imgRect = await page.locator(".stk-cell img").first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { w: r.width, h: r.height };
  });
  check("cell img 50x50", imgRect.w === 50 && imgRect.h === 50, JSON.stringify(imgRect));

  // 3. attach 初始态: 未选择窗口
  check("attach label 选择窗口", (await page.locator("#attachLabel").textContent()) === "选择窗口");

  // 4. 未选择窗口时点击表情 → 提示 (无插入调用)
  await page.locator(".stk-cell").first().click();
  await page.waitForTimeout(80);
  const insertedBefore = await page.evaluate(() => (window.__MOCK__.inserted || []).length);
  check("no insert without target", insertedBefore === 0, `inserted=${insertedBefore}`);

  // 5. 点击 attach 开始拾取 → 状态进入 picking
  await page.locator("#attachBtn").click();
  await page.waitForTimeout(80);
  const pickingState = await page.evaluate(() => window.__MOCK__.picking);
  const labelPicking = await page.locator("#attachLabel").textContent();
  check("begin_pick called", pickingState === true);
  check("attach shows picking label", (labelPicking || "").includes("目标窗口"));

  // 6. 模拟后端完成拾取: target 设置 + picking=false (轮询最多 2s)
  await page.evaluate(() => {
    window.__MOCK__.target = { hwnd: 12345, title: "文件传输助手", process: "WeChat.exe", pid: 8888 };
    window.__MOCK__.picking = false;
  });
  await page.waitForFunction(() => {
    const l = document.querySelector("#attachLabel");
    return l && l.textContent.includes("WeChat");
  }, null, { timeout: 5000 });
  check("attach shows target title", (await page.locator("#attachLabel").textContent()).includes("文件传输助手"));
  check("attach linked style", await page.locator("#attachBtn.linked").count() === 1);

  // 7. 点击表情 → insert_sticker 携带路径 → toast
  const firstPath = await page.evaluate(() => window.__MOCK__.counts);
  await page.locator(".stk-cell").first().click();
  await page.waitForTimeout(120);
  const inserted = await page.evaluate(() => window.__MOCK__.inserted || []);
  check("insert_sticker invoked with path", inserted.length === 1 && inserted[0].includes("基本表情"), JSON.stringify(inserted));
  const toastTxt = await page.locator("#toast").textContent();
  check("toast 已插入", (toastTxt || "").includes("已插入"), toastTxt);

  // 8. 再次点击 attach (此时已 linked) → 重新拾取
  await page.evaluate(() => { window.__MOCK__.picking = true; });
  await page.locator("#attachBtn").click();
  await page.waitForTimeout(80);
  check("re-pick sets picking", await page.evaluate(() => window.__MOCK__.picking));
  // 取消
  await page.evaluate(() => { window.__MOCK__.picking = false; });
  await page.locator("#attachBtn").click();
  await page.waitForTimeout(50);
  check("cancel ends picking", (await page.locator("#attachBtn.picking").count()) === 0);

  // 9. 网格可滚动 (24 个 → 6 行, 内容高于可视区)
  const scrollInfo = await page.locator("#gridArea").evaluate((el) => ({
    client: el.clientHeight,
    scroll: el.scrollHeight,
  }));
  check("grid scrollable", scrollInfo.scroll > scrollInfo.client, JSON.stringify(scrollInfo));

  // 10. 切组/预览/商店/删除 仍正常
  await page.locator(".tab").nth(1).click();
  await page.waitForTimeout(80);
  check("group2 cells==16", (await page.locator(".stk-cell").count()) === 16);
  check("group2 name shown", (await page.locator("#groupName").textContent()) === "元气团子 · 16 个 (16 个动图)");
  await page.locator(".stk-cell").first().hover();
  await page.waitForTimeout(80);
  check("preview visible", !(await page.locator("#preview").isHidden()));
  await page.locator(".tab").first().click({ button: "right" });
  await page.waitForTimeout(60);
  check("ctx menu visible", await page.locator("#ctxMenu").isVisible());
  await page.locator("#ctxMenu .item.danger").click();
  await page.waitForTimeout(60);
  await page.locator("#modalOk").click();
  await page.waitForTimeout(100);
  check("delete_package invoked", await page.evaluate(() => window.__MOCK__.calls.some(([c]) => c === "delete_package")));
  check("tabs==2 after delete", (await page.locator(".tab").count()) === 2);
  await page.locator("#shopBtn").click();
  await page.waitForTimeout(80);
  check("shop opens", await page.locator("#shopView").isVisible());
  await page.evaluate(() => { window.__MOCK__.picking = false; });

  await browser.close();
  server.close();
  console.log(failCount === 0 ? "=== UI TEST ALL PASS ===" : `=== UI TEST ${failCount} FAILURES ==="`);
  process.exit(failCount === 0 ? 0 : 1);
})().catch((e) => { console.error("UI TEST ERROR:", e); process.exit(2); });