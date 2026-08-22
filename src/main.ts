import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

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

const state = {
  packages: [] as PackageInfo[],
  stickers: [] as StickerInfo[],
  current: 0,
  cache: new Map<string, string>(),
  root: "",
  target: null as TargetInfo | null,
  picking: false,
};

const $ = <T extends HTMLElement>(sel: string) =>
  document.querySelector(sel) as T;

const gridArea = $("#gridArea");
const tabsEl = $("#tabs");
const preview = $("#preview");
const previewImg = document.querySelector("#previewImg") as HTMLImageElement;
const ctxMenu = $("#ctxMenu");
const modalMask = $("#modalMask");
const modalText = $("#modalText");
const toast = $("#toast");
const settings = $("#settings");
const setLocationVal = $("#setLocationVal");

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
  setLocationVal.textContent = ellipsis(root, 30);
  renderTabs();
  if (state.current >= pkgs.length) state.current = 0;
  if (pkgs.length) {
    await selectGroup(state.current);
  } else {
    state.stickers = [];
    renderGrid();
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

// Tab 栏滚轮 → 横向滚动切换表情包组
tabsEl.addEventListener(
  "wheel",
  (e) => {
    if (e.deltaY !== 0) {
      e.preventDefault();
      tabsEl.scrollLeft += e.deltaY;
    }
  },
  { passive: false }
);

async function selectGroup(i: number) {
  state.current = i;
  const p = state.packages[i];
  if (!p) return;
  state.stickers = await invoke<StickerInfo[]>("list_stickers", {
    package: p.name,
  });
  tabsEl.querySelectorAll(".tab").forEach((t, idx) =>
    t.classList.toggle("active", idx === i)
  );
  gridArea.scrollTop = 0;
  renderGrid();
}

function renderGrid() {
  gridArea.innerHTML = "";
  for (const st of state.stickers) {
    const cell = document.createElement("div");
    cell.className = "stk-cell";
    cell.dataset.path = st.url;
    const img = document.createElement("img");
    img.alt = st.name;
    loadImage(st.url)
      .then((d) => (img.src = d))
      .catch(() => {});
    const label = document.createElement("div");
    label.className = "stk-name";
    label.textContent = st.name; // 图片文件名
    label.title = st.name;
    cell.appendChild(img);
    cell.appendChild(label);
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

// ---------- attach 目标窗口 (设置面板) ----------
async function refreshTarget() {
  try {
    const [target, picking] = await Promise.all([
      invoke<TargetInfo | null>("get_target"),
      invoke<boolean>("is_picking"),
    ]);
    state.target = target;
    state.picking = picking;
    renderTargetVal();
  } catch {
    /* ignore */
  }
}

function renderTargetVal() {
  const v = "#setTargetVal";
  const btn = "#setTargetBtn";
  if (state.picking) {
    $(v).textContent = "正在选择… 点击目标窗口";
    $(btn).textContent = "取消";
    $(btn).classList.add("warn");
    $(btn).classList.remove("primary");
  } else if (state.target) {
    $(v).textContent = `${state.target.process} · ${ellipsis(state.target.title, 14)}`;
    $(btn).textContent = "重选";
    $(btn).classList.remove("warn");
    $(btn).classList.add("primary");
  } else {
    $(v).textContent = "未选择";
    $(btn).textContent = "选择";
    $(btn).classList.remove("warn", "primary");
  }
}

$("#setTargetBtn").addEventListener("click", async () => {
  if (state.picking) {
    await invoke("cancel_pick");
    state.picking = false;
    renderTargetVal();
    return;
  }
  try {
    await invoke("begin_pick");
    state.picking = true;
    renderTargetVal();
    showToast("请在 15 秒内点击目标窗口");
    const t0 = Date.now();
    const timer = window.setInterval(async () => {
      const picking = await invoke<boolean>("is_picking");
      const target = await invoke<TargetInfo | null>("get_target");
      state.picking = picking;
      state.target = target;
      renderTargetVal();
      if (!picking || Date.now() - t0 > 16000) {
        window.clearInterval(timer);
        if (target) showToast(`已绑定: ${target.process} · ${target.title}`);
      }
    }, 400);
  } catch (err) {
    showToast(String(err));
  }
});

// ---------- 设置面板 ----------
$("#gearBtn").addEventListener("click", (e) => {
  e.stopPropagation();
  settings.hidden = !settings.hidden;
  if (!settings.hidden) {
    refreshTarget();
    setLocationVal.textContent = ellipsis(state.root || "-", 30);
  }
});

window.addEventListener("click", (e) => {
  ctxMenu.hidden = true;
  if (!settings.hidden && !settings.contains(e.target as Node)) {
    settings.hidden = true;
  }
});

$("#setRefreshBtn").addEventListener("click", () => {
  loadAll().then(() => showToast("已刷新表情包"));
});

// 表情包位置: 选择文件夹并持久化
$("#setLocationBtn").addEventListener("click", async () => {
  try {
    const dir = await openDialog({
      directory: true,
      multiple: false,
      title: "选择表情包文件夹",
    });
    if (typeof dir === "string" && dir) {
      const root = await invoke<string>("set_stickers_dir", { path: dir });
      state.root = root;
      showToast("表情包位置已切换");
      await loadAll();
    }
  } catch (err) {
    showToast(String(err));
  }
});

// ---------- 插入表情 ----------
async function insertSticker(st: StickerInfo) {
  if (state.picking) {
    showToast("先完成目标窗口选择");
    return;
  }
  if (!state.target) {
    showToast("请先在 ⚙ 设置里选择目标窗口");
    return;
  }
  try {
    await invoke("insert_sticker", { path: st.url });
    showToast(`已插入 → ${state.target.process}`);
  } catch (err) {
    showToast(String(err));
  }
}

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

// ---------- 启动 ----------
window.addEventListener("DOMContentLoaded", () => {
  loadAll();
});