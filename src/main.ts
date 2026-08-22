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

interface TargetInfo {
  hwnd: number;
  title: string;
  process: string;
  pid: number;
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
  root: "",
  target: null as TargetInfo | null,
  picking: false,
};

const $ = <T extends HTMLElement>(sel: string) =>
  document.querySelector(sel) as T;

const gridArea = $("#gridArea");
const pageDots = $("#pageDots");
const tabsEl = $("#tabs");
const preview = $("#preview");
const previewImg = document.querySelector("#previewImg") as HTMLImageElement;
const ctxMenu = $("#ctxMenu");
const shopView = $("#shopView");
const shopList = $("#shopList");
const shopHint = $("#shopHint");
const modalMask = $("#modalMask");
const modalText = $("#modalText");
const toast = $("#toast");
const attachBtn = $("#attachBtn");
const attachLabel = $("#attachLabel");

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
  refreshTarget();
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
    cell.addEventListener("click", () => insertSticker(st));
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
  preview.hidden = false;
  loadImage(path).then((d) => {
    if (!preview.hidden) previewImg.src = d;
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

// ---------- attach 目标窗口 + 插入 ----------
async function refreshTarget() {
  try {
    const [target, picking] = await Promise.all([
      invoke<TargetInfo | null>("get_target"),
      invoke<boolean>("is_picking"),
    ]);
    state.target = target;
    state.picking = picking;
    renderAttach();
  } catch {
    /* ignore */
  }
}

function renderAttach() {
  attachBtn.classList.toggle("linked", !!state.target && !state.picking);
  attachBtn.classList.toggle("picking", state.picking);
  if (state.picking) {
    attachLabel.textContent = "点击目标窗口… 取消";
    attachBtn.title = "正在拾取 (再次点击取消)";
  } else if (state.target) {
    const t = state.target;
    attachLabel.textContent = `${t.process} · ${ellipsis(t.title, 10)}`;
    attachBtn.title = `目标: ${t.title} — 点击重新选择`;
  } else {
    attachLabel.textContent = "选择窗口";
    attachBtn.title = "点击选择要插入表情的目标窗口";
  }
}

attachBtn.addEventListener("click", async () => {
  if (state.picking) {
    await invoke("cancel_pick");
    state.picking = false;
    renderAttach();
    return;
  }
  try {
    await invoke("begin_pick");
    state.picking = true;
    renderAttach();
    showToast("请在 15 秒内点击目标窗口");
    // 轮询拾取结果
    const t0 = Date.now();
    const timer = window.setInterval(async () => {
      const picking = await invoke<boolean>("is_picking");
      state.picking = picking;
      const target = await invoke<TargetInfo | null>("get_target");
      state.target = target;
      renderAttach();
      if (!picking || Date.now() - t0 > 16000) {
        window.clearInterval(timer);
        if (target) showToast(`已绑定: ${target.process} · ${target.title}`);
        else if (!picking) showToast("未选择目标窗口");
      }
    }, 400);
  } catch (err) {
    showToast(String(err));
  }
});

async function insertSticker(st: StickerInfo) {
  if (state.picking) {
    showToast("先完成目标窗口选择");
    return;
  }
  if (!state.target) {
    showToast("请先点击右下角「选择窗口」绑定目标");
    return;
  }
  try {
    await invoke("insert_sticker", { path: st.url });
    showToast(`已插入 → ${state.target.process}`);
  } catch (err) {
    showToast(String(err));
  }
}

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
    `点「下载」把商店表情包装进分组; 自定义表情包:把文件夹放入\n<code>${ellipsis(root, 34)}</code>\n然后点面板上的 ⭮ 刷新即可(支持 gif / png)` +
    `<div class="open-root"><button id="openRoot">打开文件夹</button></div>`;
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

    const btn = document.createElement("button");
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