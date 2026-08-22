// 前端 UI 端到端测试: 用真实 Edge 渲染 dist/, mock __TAURI_INTERNALS__ 桥, 驱动全部交互
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
  installed: []
};
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args = {}) => {
    window.__MOCK__.calls.push([cmd, args]);
    const M = window.__MOCK__;
    switch (cmd) {
      case "list_packages": return M.pkgs;
      case "get_root": return "C:/mock/stickers";
      case "list_stickers": {
        const n = M.counts && M.counts[args.package] != null ? M.counts[args.package] : 24;
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
  const page = await browser.newPage({ viewport: { width: 400, height: 560 } });
  page.on("pageerror", (e) => console.log("PAGEERROR:", e.message));

  await page.goto("http://127.0.0.1:8899/", { waitUntil: "load" });
  await page.waitForSelector(".tab");

  // 1. 初始渲染: 3 分组, 第一组 24 张 → 21 格 + 2 圆点
  const tabs = await page.locator(".tab").count();
  const cells = await page.locator(".stk-cell").count();
  const dots = await page.locator(".dot").count();
  check("tabs==3", tabs === 3, `tabs=${tabs}`);
  check("grid cells==21 (page1 of 24)", cells === 21, `cells=${cells}`);
  check("page dots==2", dots === 2, `dots=${dots}`);

  // img src 为 data:image
  const src = await page.locator(".stk-cell img").first().getAttribute("src");
  check("cell img is data-url png", !!src && src.startsWith("data:image/png"), src);

  // 2. 翻页: 点第2个圆点 → 3 格
  await page.locator(".dot").nth(1).click();
  await page.waitForTimeout(100);
  const cellsP2 = await page.locator(".stk-cell").count();
  check("page2 cells==3", cellsP2 === 3, `cells=${cellsP2}`);

  // 3. 点表情 → chip 出现, 发送可用
  await page.locator(".stk-cell").first().click();
  const chips = await page.locator("#inputChips img").count();
  check("chip added on click", chips === 1, `chips=${chips}`);
  const sendDisabled = await page.locator("#sendBtn").isDisabled();
  check("send enabled", sendDisabled === false);

  // 4. 发送 → toast + 清空 + 禁用
  await page.locator("#sendBtn").click();
  const toastTxt = await page.locator("#toast").textContent();
  await page.waitForTimeout(50);
  const chipsAfter = await page.locator("#inputChips img").count();
  check("toast 已发送", (toastTxt || "").includes("已发送 1 个表情"), toastTxt);
  check("chips cleared", chipsAfter === 0, `chips=${chipsAfter}`);
  check("send disabled again", await page.locator("#sendBtn").isDisabled());

  // 5. 切组: 元气团子 16 张 → 1 页 16 格
  await page.locator(".tab").nth(1).click();
  await page.waitForTimeout(100);
  const cellsG = await page.locator(".stk-cell").count();
  const dotsG = await page.locator(".dot").count();
  check("group2 cells==16", cellsG === 16, `cells=${cellsG}`);
  check("group2 dots==1", dotsG === 1, `dots=${dotsG}`);

  // 6. GIF 徽标
  const gifBadge = await page.locator(".tab .gif-badge").count();
  check("gif badge on group2 tab", gifBadge === 1, `badge=${gifBadge}`);

  // 7. hover 预览
  await page.locator(".stk-cell").first().hover();
  await page.waitForTimeout(120);
  const prevHidden = await page.locator("#preview").isHidden();
  const prevSrc = await page.locator("#previewImg").getAttribute("src");
  check("preview visible on hover", prevHidden === false && !!prevSrc, `hidden=${prevHidden} src=${prevSrc}`);
  await page.mouse.move(5, 5);
  await page.waitForTimeout(80);
  check("preview hidden after leave", await page.locator("#preview").isHidden());

  // 8. 右键删除分组 (确认弹窗)
  await page.locator(".tab").first().click({ button: "right" });
  await page.waitForTimeout(80);
  check("ctx menu visible", await page.locator("#ctxMenu").isVisible());
  await page.locator("#ctxMenu .item.danger").click();
  await page.waitForTimeout(80);
  check("confirm modal visible", await page.locator("#modalMask").isVisible());
  await page.locator("#modalOk").click();
  await page.waitForTimeout(150);
  const tabsAfterDel = await page.locator(".tab").count();
  const delCalled = await page.evaluate(() => window.__MOCK__.calls.some(([c]) => c === "delete_package"));
  check("delete_package invoked", delCalled);
  check("tabs==2 after delete", tabsAfterDel === 2, `tabs=${tabsAfterDel}`);

  // 9. 商店: 打开 → 列表 → 下载
  await page.locator("#shopBtn").click();
  await page.waitForTimeout(120);
  check("shop view visible", await page.locator("#shopView").isVisible());
  const shopItems = await page.locator(".shop-item").count();
  check("shop items==1", shopItems === 1, `items=${shopItems}`);
  await page.locator(".shop-item .dl-btn").first().click();
  await page.waitForTimeout(150);
  const installed = await page.evaluate(() => window.__MOCK__.installed);
  check("install_package invoked with 柴犬日常", installed.includes("柴犬日常"), JSON.stringify(installed));
  await page.locator("#shopBack").click();
  await page.waitForTimeout(80);
  check("shop closes", await page.locator("#shopView").isHidden());
  const tabsFinal = await page.locator(".tab").count();
  check("tabs==3 after install (shop adds pkg)", tabsFinal === 3, `tabs=${tabsFinal}`);

  // 10. ⌫ 删除输入表情
  await page.locator(".stk-cell").first().click();
  await page.locator(".stk-cell").nth(2).click();
  check("two chips", (await page.locator("#inputChips img").count()) === 2);
  await page.locator("#delBtn").click();
  await page.waitForTimeout(50);
  check("del removes last chip", (await page.locator("#inputChips img").count()) === 1);

  await browser.close();
  server.close();
  console.log(failCount === 0 ? "=== UI TEST ALL PASS ===" : `=== UI TEST ${failCount} FAILURES ==="`);
  process.exit(failCount === 0 ? 0 : 1);
})().catch((e) => { console.error("UI TEST ERROR:", e); process.exit(2); });