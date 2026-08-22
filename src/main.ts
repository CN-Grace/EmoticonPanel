import { invoke } from "@tauri-apps/api/core";

interface StickerInfo {
  url: string;
  name: string;
  isGif: boolean;
}

interface PackageInfo {
  name: string;
  cover: string | null;
  count: number;
  gifCount: number;
  shop: boolean;
}

const PAGE_W = 7;
const PAGE_H = 3;
const PAGE_SIZE = PAGE_W * PAGE_H;

const state = {
  packages: [] as PackageInfo[],
  stickers: [] as StickerInfo[],
  current: 0,
  page: 0,
  cache: new Map<string, string>(),
  chips: [] as { path: string; name: string }[],
  root: "",
};

const $ = <T extends HTMLElement>(sel: string) =>
  document.querySelector(sel) as T;

const gridArea = $("#gridArea");
const pageDots = $("#pageDots");
const tabsEl = $("#tabs");
const chipsEl = $("#inputChips");
const inputPh = $("#inputPh");
const sendBtn = document.querySelector("#sendBtn") as HTMLButtonElement;
const delBtn = $("#delBtn");
const preview = $("#preview");
const previewImg = document.querySelector("#previewImg") as HTMLImageElement;
const ctxMenu = $("#ctxMenu");
const shopView = $("#shopView");
const shopList = $("#shopList");
const shopHint = $("#shopHint");
const modalMask = $("#modalMask");
const modalText = $("#modalText");
const toast = $("#toast");

// ---------- 基础工具 ----------
let toastTimer: number | undefined;
function showToast(msg: string) {
  toast.textContent = msg;
  toast.classList.add("show");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toast.classList.remove("show"), 1800);
}

function confirmDialog(text: string): Promise<boolean> {
  return new Promise((resolve) => {
    modalText.textContent = text;
    modalMask.hidden = false;
    const ok = () => {
      cleanup();
      resolve(true);
    };
    const cancel = () => {
      cleanup();
      resolve(false);
    };
    const cleanup = () => {
      $("#modalOk").removeEventListener("click", ok);
      $("#modalCancel").removeEventListener("click", cancel);
      modalMask.hidden = true;
    };
    $("#modalOk").addEventListener("click", ok);
    $("#modalCancel").addEventListener("click", cancel);
  });
}

async function loadImage(path: string): Promise<string> {
  const hit = state.cache.get(path);
  if (hit) return hit;
  const data = await invoke<string>("read_sticker", { path });
  state.cache.set(path, data);
  return data;
}

function ellipsis(s: string, n: number) {
  return s.length > n ? s.slice(0, n) + "…" : s;
}

// ---------- 表情包加载 ----------
async function loadAll() {
  const [pkgs, root] = await Promise.all([
    invoke<PackageInfo[]>("list_packages"),
    invoke<string>("get_root"),
  ]);
  state.packages = pkgs;
  state.root = root;
  renderTabs();
  if (state.current >= pkgs.length) state.current = 0;
  if (pkgs.length) {
    state.page = 0;
    await selectGroup(state.current);
  } else {
    state.stickers = [];
    renderGrid();
    renderDots();
  }
  updateSend();
}

function renderTabs() {
  tabsEl.innerHTML = "";
  state.packages.forEach((p, i) => {
    const tab = document.createElement("div");
    tab.className = "tab" + (i === state.current ? " active" : "");
    tab.title = `${p.name} · ${p.count}个${p.gifCount ? ` · ${p.gifCount}个动图` : ""}`;
    const img = document.createElement("img");
    img.alt = p.name;
    if (p.cover) {
      loadImage(p.cover).then((d) => (img.src = d)).catch(() => {});
    }
    tab.appendChild(img);
    if (p.gifCount > 0) {
      const badge = document.createElement("span");
      badge.className = "gif-badge";
      badge.textContent = "GIF";
      tab.appendChild(badge);
    }
    tab.addEventListener("click", () => selectGroup(i));
    tab.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showCtxMenu(e.clientX, e.clientY, i);
    });
    tabsEl.appendChild(tab);
  });
}

async function selectGroup(i: number) {
  state.current = i;
  state.page = 0;
  const p = state.packages[i];
  if (!p) return;
  state.stickers = await invoke<StickerInfo[]>("list_stickers", {
    package: p.name,
  });
  tabsEl.querySelectorAll(".tab").forEach((t, idx) =>
    t.classList.toggle("active", idx === i)
  );
  renderGrid();
  renderDots();
}

function pageCount() {
  return Math.max(1, Math.ceil(state.stickers.length / PAGE_SIZE));
}

function renderGrid() {
  gridArea.innerHTML = "";
  const start = state.page * PAGE_SIZE;
  const slice = state.stickers.slice(start, start + PAGE_SIZE);
  for (const st of slice) {
    const cell = document.createElement("div");
    cell.className = "stk-cell";
    cell.dataset.path = st.url;
    cell.dataset.name = `${state.packages[state.current]?.name}/${st.name}`;
    const img = document.createElement("img");
    img.alt = st.name;
    loadImage(st.url)
      .then((d) => (img.src = d))
      .catch(() => {});
    cell.appendChild(img);
    cell.addEventListener("click", () => addChip(st.url, st.name));
    cell.addEventListener("pointerenter", (e) =>
      showPreview(e.clientX, e.clientY, st.url)
    );
    cell.addEventListener("pointermove", (e) =>
      movePreview(e.clientX, e.clientY)
    );
    cell.addEventListener("pointerleave", hidePreview);
    gridArea.appendChild(cell);
  }
}

function renderDots() {
  pageDots.innerHTML = "";
  const n = pageCount();
  for (let i = 0; i < n; i++) {
    const dot = document.createElement("div");
    dot.className = "dot" + (i === state.page ? " active" : "");
    dot.addEventListener("click", () => {
      state.page = i;
      renderGrid();
      renderDots();
    });
    pageDots.appendChild(dot);
  }
}

// 滚轮翻页
gridArea.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    const n = pageCount();
    const target = e.deltaY > 0 ? state.page + 1 : state.page - 1;
    if (target >= 0 && target < n) {
      state.page = target;
      renderGrid();
      renderDots();
    }
  },
  { passive: false }
);

// ---------- 预览 ----------
function showPreview(x: number, y: number, path: string) {
  const img = previewImg;
  preview.hidden = false;
  loadImage(path).then((d) => {
    if (!preview.hidden) img.src = d;
  });
  movePreview(x, y);
}

function movePreview(x: number, y: number) {
  const r = preview.getBoundingClientRect();
  let left = x + 14;
  let top = y - r.height - 8;
  if (left + r.width > window.innerWidth) left = x - r.width - 14;
  if (top < 4) top = y + 14;
  preview.style.left = left + "px";
  preview.style.top = top + "px";
}

function hidePreview() {
  preview.hidden = true;
  previewImg.removeAttribute("src");
}

// ---------- 输入框 / 发送 / 删除 ----------
function addChip(path: string, name: string) {
  state.chips.push({ path, name });
  renderChips();
}

function renderChips() {
  chipsEl.innerHTML = "";
  inputPh.style.display = state.chips.length ? "none" : "";
  for (const c of state.chips) {
    const img = document.createElement("img");
    img.title = c.name;
    img.alt = c.name;
    loadImage(c.path).then((d) => (img.src = d));
    img.addEventListener("click", () => {
      state.chips = state.chips.filter((x) => x !== c);
      renderChips();
    });
    chipsEl.appendChild(img);
  }
  updateSend();
}

function updateSend() {
  sendBtn.disabled = state.chips.length === 0;
}

sendBtn.addEventListener("click", async () => {
  const n = state.chips.length;
  if (!n) return;
  state.chips = [];
  renderChips();
  showToast(`已发送 ${n} 个表情 ✓`);
});

delBtn.addEventListener("click", () => {
  if (!state.chips.length) return;
  state.chips.pop();
  renderChips();
});

// ---------- 刷新 ----------
$("#refreshBtn").addEventListener("click", () => {
  loadAll().then(() => showToast("已刷新表情包"));
});

// ---------- 右键删除分组 ----------
function showCtxMenu(x: number, y: number, idx: number) {
  const p = state.packages[idx];
  if (!p) return;
  ctxMenu.innerHTML = "";
  const item = document.createElement("div");
  item.className = "item danger";
  item.textContent = `删除「${ellipsis(p.name, 8)}」`;
  item.addEventListener("click", async () => {
    ctxMenu.hidden = true;
    if (await confirmDialog(`确定删除表情包「${p.name}」吗?\n(会删除文件夹 ${p.name})`)) {
      try {
        await invoke("delete_package", { name: p.name });
        showToast(`已删除「${p.name}」`);
        await loadAll();
      } catch (err) {
        showToast(String(err));
      }
    }
  });
  ctxMenu.appendChild(item);
  const r = ctxMenu.getBoundingClientRect();
  ctxMenu.style.left = Math.min(x, window.innerWidth - r.width - 4) + "px";
  ctxMenu.style.top = Math.min(y, window.innerHeight - r.height - 4) + "px";
  ctxMenu.hidden = false;
}

window.addEventListener("click", () => (ctxMenu.hidden = true));

// ---------- 商店 ----------
$("#shopBtn").addEventListener("click", () => openShop());

async function openShop() {
  const installed = new Set(state.packages.map((p) => p.name));
  const [pkgs, root] = await Promise.all([
    invoke<PackageInfo[]>("shop_list"),
    invoke<string>("get_root"),
  ]);
  shopHint.innerHTML =
    `已安装分组点右侧 ⌫ ... 自定义表情包:把文件夹放入\n<code>${ellipsis(root, 34)}</code>\n然后点面板上的 ⭮ 刷新即可(支持 gif / png)` +
    `<div class="shop-hint-open"><button id="openRoot">打开文件夹</button></div>`;
  shopView.hidden = false;

  $("#openRoot")?.addEventListener("click", async () => {
    try {
      await invoke("reveal_root");
    } catch {
      showToast("无法打开文件夹");
    }
  });

  shopList.innerHTML = "";
  if (!pkgs.length) {
    const empty = document.createElement("div");
    empty.className = "shop-empty";
    empty.textContent = "商店里暂时没有表情包";
    shopList.appendChild(empty);
    return;
  }
  for (const p of pkgs) {
    const row = document.createElement("div");
    row.className = "shop-item";

    const cover = document.createElement("img");
    cover.className = "cover";
    if (p.cover) {
      loadImage(p.cover).then((d) => (cover.src = d)).catch(() => {});
    }
    const meta = document.createElement("div");
    meta.className = "meta";
    const name = document.createElement("div");
    name.className = "pname";
    name.textContent = p.name;
    const desc = document.createElement("div");
    desc.className = "pdesc";
    desc.textContent = `${p.count} 个表情${p.gifCount ? ` · ${p.gifCount} 个动图` : ""} · 点击下载后出现在底部分组`;
    meta.append(name, desc);

    const btn = document.createElement("button") as HTMLButtonElement;
    const isInstalled = installed.has(p.name);
    btn.className = "dl-btn" + (isInstalled ? " done" : "");
    btn.textContent = isInstalled ? "已下载" : "下载";
    btn.disabled = isInstalled;
    if (!isInstalled) {
      btn.addEventListener("click", async () => {
        try {
          await invoke("install_package", { name: p.name });
          showToast(`已下载「${p.name}」`);
          await loadAll();
          openShop();
        } catch (err) {
          showToast(String(err));
        }
      });
    }

    row.append(cover, meta, btn);
    shopList.appendChild(row);
  }
}

$("#shopBack").addEventListener("click", () => {
  shopView.hidden = true;
});

// ---------- 启动 ----------
window.addEventListener("DOMContentLoaded", () => {
  loadAll();
});