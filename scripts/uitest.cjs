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
    { name: "像素猫", cover: null, count: 20, gifCount: 0, shop: false },
    { name: "表情包D", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包E", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包F", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包G", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包H", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包I", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包J", cover: null, count: 24, gifCount: 0, shop: false },
    { name: "表情包K", cover: null, count: 24, gifCount: 0, shop: false }
  ],
  counts: { "基本表情": 24, "元气团子": 16, "像素猫": 20 },
  installed: [],
  target: null,
  picking: false,
  root: "C:/mock/stickers"
};
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args = {}) => {
    window.__MOCK__.calls.push([cmd, args]);
    const M = window.__MOCK__;
    switch (cmd) {
      case "list_packages": return M.pkgs;
      case "get_root": return M.root;
      case "set_stickers_dir": M.root = args.path; M.calls.push(["set_stickers_dir_called", args.path]); return args.path;
      case "list_stickers": {
        const n = M.counts[args.package] != null ? M.counts[args.package] : 24;
        return Array.from({ length: n }, (_, i) => ({
          url: M.root + "/" + args.package + "/" + String(i + 1).padStart(2, "0") + ".png",
          name: "表情" + String(i + 1),
          isGif: args.package === "元气团子"
        }));
      }
      case "read_sticker": return M.png;
      case "plugin:dialog|open": return "D:/my/emoticons";
      case "delete_package": M.pkgs = M.pkgs.filter((p) => p.name !== args.name); return null;
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
  const page = await browser.newPage({ viewport: { width: 350, height: 470 } });
  page.on("pageerror", (e) => console.log("PAGEERROR:", e.message));

  await page.goto("http://127.0.0.1:8899/", { waitUntil: "load" });
  await page.waitForSelector(".stk-cell");

  // 1. 初始: 3 分组; 全部表情显示; 每个格子带图片名; 预览 75x75
  check("tabs==11", (await page.locator(".tab").count()) === 11);
  const tabsOverflow = await page.locator("#tabs").evaluate((el) => el.scrollWidth > el.clientWidth);
  check("tabs overflow (wheel has room)", tabsOverflow);
  check("grid cells==24 (all)", (await page.locator(".stk-cell").count()) === 24);
  check("no shop button", (await page.locator("#shopBtn").count()) === 0);
  check("cell has name label", (await page.locator(".stk-cell .stk-name").count()) === 24);
  check("first label is file name", (await page.locator(".stk-cell .stk-name").first().textContent()) === "表情1");
  const imgRect = await page.locator(".stk-cell img").first().evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { w: r.width, h: r.height };
  });
  check("cell img 75x75", Math.round(imgRect.w) === 75 && Math.round(imgRect.h) === 75, JSON.stringify(imgRect));

  // 2. 网格可滚动 (6 行 > 可视区)
  const scrollInfo = await page.locator("#gridArea").evaluate((el) => ({
    client: el.clientHeight,
    scroll: el.scrollHeight,
  }));
  check("grid scrollable", scrollInfo.scroll > scrollInfo.client, JSON.stringify(scrollInfo));

  // 3. Tab 滚轮横向滚动 tabs
  const tabScrollBefore = await page.locator("#tabs").evaluate((el) => el.scrollLeft);
  await page.locator("#tabs").hover();
  await page.mouse.wheel(0, 120);
  await page.waitForTimeout(80);
  const tabScrollAfter = await page.locator("#tabs").evaluate((el) => el.scrollLeft);
  check("tabs wheel scrolls", tabScrollAfter > tabScrollBefore, `${tabScrollBefore} -> ${tabScrollAfter}`);

  // 4. 设置面板: 打开 → 含三行 (目标窗口/刷新/位置)
  await page.locator("#gearBtn").click();
  await page.waitForTimeout(80);
  check("settings visible", await page.locator("#settings").isVisible());
  check("settings 3 rows", (await page.locator(".set-row").count()) === 3);
  check("location shows root", (await page.locator("#setLocationVal").textContent()).includes("C:/mock/stickers"));

  // 5. 位置选择: 模拟选文件夹 → set_stickers_dir called, root 更新
  await page.evaluate(() => {
    window.__MOCK__.root = "D:/my/emoticons";
  });
  await page.locator("#setLocationBtn").click();
  await page.waitForTimeout(120);
  const dirCall = await page.evaluate(() => (window.__MOCK__.calls || []).filter(([c]) => c === "set_stickers_dir_called"));
  check("set_stickers_dir invoked", dirCall.length >= 1, JSON.stringify(dirCall));
  check("location label updated", (await page.locator("#setLocationVal").textContent()).includes("D:/my/emoticons"));

  // 6. 目标窗口: 未选择时 insert 提示; 选择后 insert 成功
  await page.locator(".stk-cell").first().click();
  await page.waitForTimeout(80);
  check("no insert without target", await page.evaluate(() => (window.__MOCK__.inserted || []).length) === 0);
  const toast1 = await page.locator("#toast").textContent();
  check("prompt to pick target", (toast1 || "").includes("选择目标窗口"), toast1);

  // 通过设置面板选择目标 (刚才点格子关闭了面板, 重新打开)
  await page.locator("#gearBtn").click();
  await page.waitForTimeout(60);
  await page.locator("#setTargetBtn").click();
  await page.waitForTimeout(80);
  check("begin_pick via settings", await page.evaluate(() => window.__MOCK__.picking));
  check("setTarget shows picking", (await page.locator("#setTargetVal").textContent()).includes("正在选择"));
  await page.evaluate(() => {
    window.__MOCK__.target = { hwnd: 123, title: "文件传输助手", process: "WeChat.exe", pid: 1 };
    window.__MOCK__.picking = false;
  });
  await page.waitForFunction(() => {
    const el = document.querySelector("#setTargetVal");
    return el && el.textContent.includes("WeChat");
  }, null, { timeout: 5000 });
  check("target shown in settings", (await page.locator("#setTargetVal").textContent()).includes("文件传输助手"));

  // 重开面板 → 点空白处关闭 → 点表情成功插入
  await page.locator("#gearBtn").click();
  await page.waitForTimeout(50);
  await page.mouse.click(150, 445);
  await page.waitForTimeout(50);
  const settingsHidden = await page.locator("#settings").isHidden();
  check("settings closes on outside click", settingsHidden);
  await page.locator(".stk-cell").first().click();
  await page.waitForTimeout(120);
  const inserted = await page.evaluate(() => window.__MOCK__.inserted || []);
  check("insert_sticker invoked", inserted.length === 1, JSON.stringify(inserted));

  // 7. 刷新
  await page.locator("#gearBtn").click();
  await page.locator("#setRefreshBtn").click();
  await page.waitForTimeout(100);
  check("refresh reloaded (list_packages called)", await page.evaluate(() => window.__MOCK__.calls.filter(([c]) => c === "list_packages").length >= 2));

  // 8. 右键删除分组仍可用
  await page.locator(".tab").first().click({ button: "right" });
  await page.waitForTimeout(60);
  await page.locator("#ctxMenu .item.danger").click();
  await page.waitForTimeout(60);
  await page.locator("#modalOk").click();
  await page.waitForTimeout(100);
  check("delete_package invoked", await page.evaluate(() => window.__MOCK__.calls.some(([c]) => c === "delete_package")));

  await browser.close();
  server.close();
  console.log(failCount === 0 ? "=== UI TEST ALL PASS ===" : `=== UI TEST ${failCount} FAILURES ==="`);
  process.exit(failCount === 0 ? 0 : 1);
})().catch((e) => { console.error("UI TEST ERROR:", e); process.exit(2); });