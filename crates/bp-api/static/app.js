"use strict";

const SESSION_KEY = "bp-session";
const NETWORK_KEY = "bp-network";
const REFRESH_MS = 5000;
const MIB = 1 << 20;

const fragment = new URLSearchParams(location.hash.slice(1));
if (fragment.get("session")) {
  localStorage.setItem(SESSION_KEY, fragment.get("session"));
  history.replaceState(null, "", location.pathname);
}

const $ = (id) => document.getElementById(id);
const state = { kind: "all", network: localStorage.getItem(NETWORK_KEY) || "", data: null };

function session() {
  return localStorage.getItem(SESSION_KEY) || "";
}

class ApiError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    ...options,
    headers: { Authorization: "Session " + session(), ...(options.headers || {}) },
  });
  if (!response.ok) {
    let message = response.statusText;
    try {
      message = (await response.json()).error || message;
    } catch (_) {}
    throw new ApiError(response.status, message);
  }
  return response;
}

const json = async (path) => (await api(path)).json();

function esc(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function bytes(n) {
  n = Number(n) || 0;
  if (n >= 1 << 30) return (n / (1 << 30)).toFixed(1) + " GiB";
  if (n >= MIB) return (n / MIB).toFixed(1) + " MiB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KiB";
  return n + " B";
}

function pct(part, whole) {
  return whole > 0 ? Math.round((part / whole) * 100) : 0;
}

function ago(seconds) {
  const s = Math.max(0, Math.round(Date.now() / 1000 - seconds));
  if (s < 60) return s + " s ago";
  if (s < 3600) return Math.round(s / 60) + " min ago";
  return Math.round(s / 3600) + " h ago";
}

function short(id) {
  return id ? id.slice(0, 12) + "…" : "";
}

function toast(message, kind = "") {
  const el = document.createElement("div");
  el.className = "toast " + kind;
  el.textContent = message;
  $("toasts").append(el);
  setTimeout(() => el.remove(), kind === "error" ? 9000 : 4500);
}

function banner(html) {
  const el = $("banner");
  el.hidden = !html;
  el.innerHTML = html || "";
}

function bar(total, parts) {
  return `<div class="bar">${parts
    .filter(([value]) => value > 0 && total > 0)
    .map(([value, cls]) => `<span class="${cls}" style="width:max(4px, ${Math.min(100, (value / total) * 100)}%)"></span>`)
    .join("")}</div>`;
}

function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}

function legend(items) {
  return `<div class="legend">${items.map(([cls, text]) => `<span><i class="${cls}"></i>${text}</span>`).join("")}</div>`;
}

async function load() {
  const [status, flock, storage, files] = await Promise.all([
    json("/status"),
    json("/peers"),
    json("/storage"),
    json("/files"),
  ]);
  const members = {};
  await Promise.all(
    status.networks.map(async (network) => {
      try {
        members[network] = await json(`/networks/${encodeURIComponent(network)}/members`);
      } catch (_) {}
    }),
  );
  return { status, flock, storage, files, members };
}

function names(data) {
  const out = {};
  for (const m of Object.values(data.members)) {
    for (const member of m.members) out[member.fingerprint] = member.name || short(member.fingerprint);
  }
  return out;
}

function inNetwork(network) {
  return !state.network || state.network === network;
}

function nodes(data) {
  const me = data.status.fingerprint;
  const who = names(data);
  const qos = Object.fromEntries(data.flock.qos.map((q) => [q.peer_id, q]));
  const local = data.status.local_services.map((svc) => {
    const stat = data.storage.pouches.find((p) => p.service_id === svc.id);
    return {
      kind: svc.service_type,
      network: svc.network_id,
      owner: me,
      name: who[me] || data.status.alias || "you",
      mine: true,
      peer: data.status.peer_id,
      paused: svc.status !== "running",
      quota: stat ? stat.quota_bytes : 0,
      stored: stat ? stat.fragment_bytes + stat.filler_bytes : 0,
      data: stat ? stat.fragment_bytes : 0,
      tier: stat ? stat.tier : svc.metadata.tier,
      seen: null,
      qos: null,
    };
  });
  const remote = data.flock.known_peers.map((n) => ({
    kind: n.service_type,
    network: n.network_id,
    owner: n.user_fingerprint,
    name: who[n.user_fingerprint] || n.user_alias || short(n.user_fingerprint),
    mine: n.user_fingerprint === me,
    peer: n.peer_id,
    paused: Boolean(n.metadata.paused),
    evicting: Boolean(n.metadata.evicting),
    quota: Number(n.metadata.storage_bytes) || 0,
    stored: Number(n.metadata.stored_bytes) || 0,
    data: 0,
    tier: n.metadata.tier,
    seen: n.announced_at,
    qos: qos[n.peer_id] || null,
  }));
  return [...local, ...remote].filter((n) => inNetwork(n.network));
}

function verification(node, data) {
  if (node.kind !== "pouch") {
    if (node.paused) return ["wait", "In maintenance"];
    return ["ok", node.seen ? "Online · seen " + ago(node.seen) : "Running on this device"];
  }
  if (node.evicting) return ["bad", "Leaving the network"];
  if (node.paused) return ["wait", "In maintenance"];
  if (node.mine && node.peer === data.status.peer_id) {
    const c = data.storage.credits.find((c) => c.network_id === node.network) || {};
    if (!c.credit_bytes) return ["wait", "Waiting for the network to verify it"];
    return ["ok", `Verified ${bytes(c.credit_bytes)} · your quota ${bytes(c.quota_verified_bytes)}`];
  }
  if (node.mine) return ["idle", "Your other device: others challenge it"];
  const q = node.qos;
  if (!q || q.challenges_sent === 0) return ["wait", "Not challenged yet"];
  const rate = Math.round(q.challenge_success_ewma * 100);
  const detail = `${q.challenges_succeeded}/${q.challenges_sent} challenges passed (${rate}%)`;
  if (q.challenge_success_ewma >= 0.9) return ["ok", detail];
  if (q.challenge_success_ewma >= 0.5) return ["wait", detail];
  return ["bad", detail];
}

function renderSummary(data) {
  const credits = data.storage.credits.filter((c) => inNetwork(c.network_id));
  const offered = credits.reduce((a, c) => a + c.offered_bytes, 0);
  const quota = credits.reduce((a, c) => a + c.quota_bytes, 0);
  const verified = credits.reduce((a, c) => a + c.quota_verified_bytes, 0);
  const used = Math.min(verified, credits.reduce((a, c) => a + c.quota_used_bytes, 0));
  const free = Math.max(0, verified - used);
  const pending = Math.max(0, quota - verified);
  const lost = Math.max(0, offered - quota);
  const rated = credits.filter((c) => c.rate);
  const rates = [...new Set(rated.map((c) => c.rate.join("/")))];
  const mine = data.storage.pouches.filter((p) => inNetwork(p.network_id));
  const filled = mine.reduce((a, p) => a + p.fragment_bytes + p.filler_bytes, 0);
  const files = data.files.files.filter((f) => f.mine && inNetwork(f.network_id));
  const fileBytes = files.reduce((a, f) => a + f.size_bytes, 0);
  const all = nodes(data);
  const count = (kind) => all.filter((n) => n.kind === kind).length;
  const survive = rated
    .map((c) => {
      const spare = c.rate[1] - c.rate[0];
      const text = spare === 1 ? "1 Pouch" : `${spare} Pouches`;
      return `${rated.length > 1 ? esc(c.network_id) + ": " : ""}they survive ${text} going offline (k/n ${c.rate.join("/")})`;
    })
    .join("<br>");

  const quotaCard = !offered
    ? `<div class="big">—</div>${bar(1, [])}<div class="sub">Offer space with <b>＋ Service</b> to get a quota.</div>`
    : !rates.length
      ? `<div class="big">—</div>${bar(offered, [[offered, "pending"]])}<div class="sub">Waiting for enough Pouches to set k/n.</div>`
      : `<div class="big">${bytes(free)} <small>free</small></div>
         ${bar(offered, [[used, "used"], [free, "left"], [pending, "pending"], [lost, "lost"]])}
         ${legend([
           ["used", `used ${bytes(used)}`],
           ["left", `free ${bytes(free)}`],
           ["pending", `not verified yet ${bytes(pending)}`],
           ["lost", `lost to redundancy ${bytes(lost)} (${pct(lost, offered)}%)`],
         ])}
         <div class="sub">Quota ${bytes(quota)} of the ${bytes(offered)} you offer · k/n ${rates.join(", ")}</div>`;

  $("summary").innerHTML = `
    <div class="card">
      <h3>Your quota</h3>
      ${quotaCard}
    </div>
    <div class="card">
      <h3>Your Pouch setup</h3>
      <div class="big">${mine.length ? pct(filled, offered) + "%" : "—"}</div>
      ${bar(offered, [[filled, "filler"]])}
      <div class="sub">${mine.length ? `${bytes(filled)} of ${bytes(offered)} filled and challenged by the network` : "No Pouch on this node yet."}</div>
    </div>
    <div class="card">
      <h3>Your files</h3>
      <div class="big">${files.length}</div>
      <div class="sub">${bytes(fileBytes)} stored${survive ? "<br>" + survive : ""}</div>
    </div>
    <div class="card">
      <h3>Nodes</h3>
      <div class="big">${all.length}</div>
      <div class="sub">${plural(count("pouch"), "Pouch", "Pouches")} · ${plural(count("bill"), "Bill", "Bills")} · ${plural(count("post"), "Roost", "Roosts")}</div>
    </div>`;
}

function renderNodes(data) {
  const list = nodes(data)
    .filter((n) => state.kind === "all" || n.kind === state.kind)
    .sort((a, b) => Number(b.mine) - Number(a.mine) || a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
  if (!list.length) {
    $("nodes").innerHTML = `<div class="empty">No nodes here yet.</div>`;
    return;
  }
  $("nodes").innerHTML = list
    .map((n) => {
      const [cls, text] = verification(n, data);
      const label = n.kind === "post" ? "roost" : n.kind;
      const setup =
        n.kind === "pouch"
          ? `${bar(n.quota, [[n.data, "data"], [Math.max(0, n.stored - n.data), "filler"]])}
             <div class="meta">${n.tier ? esc(n.tier) + " · " : ""}setup ${pct(n.stored, n.quota)}% — ${bytes(n.stored)} of ${bytes(n.quota)}</div>`
          : "";
      return `<div class="node ${n.mine ? "mine" : ""}">
        <div class="node-top"><span class="name">${esc(n.name)}</span>${n.mine ? '<span class="you">you</span>' : ""}<span class="kind">${label}</span></div>
        <div class="meta">${esc(n.network)} · <code>${esc(short(n.peer))}</code></div>
        ${setup}
        <div class="state ${cls}">${esc(text)}</div>
      </div>`;
    })
    .join("");
}

function renderFiles(data) {
  const who = names(data);
  const files = data.files.files.filter((f) => inNetwork(f.network_id)).sort((a, b) => b.uploaded_at - a.uploaded_at);
  if (!files.length) {
    $("files").innerHTML = `<div class="empty">No files yet. Drop one above.</div>`;
    return;
  }
  $("files").innerHTML = `<table>
    <thead><tr><th>Name</th><th>Size</th><th>Owner</th><th>Uploaded</th><th></th></tr></thead>
    <tbody>${files
      .map(
        (f) => `<tr>
          <td class="fname">${esc(f.file_name)}<small>${esc(f.network_id)} · ${f.chunks} chunk${f.chunks === 1 ? "" : "s"}${f.mine && f.shared_with ? ` · shared with ${f.shared_with}` : ""}</small></td>
          <td>${bytes(f.size_bytes)}</td>
          <td>${f.mine ? '<span class="owner-you">you</span>' : esc(who[f.owner] || short(f.owner))}</td>
          <td>${new Date(f.uploaded_at * 1000).toLocaleString()}</td>
          <td class="actions">
            <button class="button small" data-download="${esc(f.file_id)}" data-network="${esc(f.network_id)}" data-name="${esc(f.file_name)}">Download</button>
            <button class="button small danger" data-delete="${esc(f.file_id)}" data-network="${esc(f.network_id)}" data-name="${esc(f.file_name)}">${f.mine ? "Delete" : "Remove"}</button>
          </td>
        </tr>`,
      )
      .join("")}</tbody></table>`;
}

function renderHeader(data) {
  const s = data.status;
  const who = names(data);
  $("me").innerHTML = `<strong>${esc(who[s.fingerprint] || s.alias || "you")}</strong><br><code>${esc(s.fingerprint)}</code>`;
  const select = $("network");
  const options = ["", ...s.networks];
  if (state.network && !s.networks.includes(state.network)) state.network = "";
  const wanted = options.map((n) => `<option value="${esc(n)}">${n ? esc(n) : "All networks"}</option>`).join("");
  if (select.dataset.options !== wanted) {
    select.innerHTML = wanted;
    select.dataset.options = wanted;
  }
  select.value = state.network;
}

function render() {
  if (!state.data) return;
  renderHeader(state.data);
  renderSummary(state.data);
  renderNodes(state.data);
  renderFiles(state.data);
}

async function refresh() {
  try {
    state.data = await load();
    $("live").classList.add("ok");
    banner("");
    render();
  } catch (e) {
    $("live").classList.remove("ok");
    if (e.status === 401) {
      banner("Your session is missing or expired. Run <code>bp dashboard</code> on this device to open a new one.");
    } else {
      banner(`Cannot reach the daemon: ${esc(e.message)}`);
    }
  }
}

function uploadNetwork() {
  if (state.network) return state.network;
  const networks = state.data ? state.data.status.networks : [];
  if (networks.length === 1) return networks[0];
  return null;
}

function upload(file) {
  const network = uploadNetwork();
  if (!network) {
    toast("Pick a network first, at the top of the page.", "error");
    return Promise.resolve();
  }
  const row = document.createElement("div");
  row.className = "upload";
  row.innerHTML = `<span>${esc(file.name)}</span><span class="status">${bytes(file.size)}</span><div class="bar"><span class="used" style="width:0%"></span></div>`;
  $("uploads").append(row);
  const fill = row.querySelector(".bar span");
  const label = row.querySelector(".status");
  return new Promise((resolve) => {
    const xhr = new XMLHttpRequest();
    const query = new URLSearchParams({ network, name: file.name });
    xhr.open("POST", "/files?" + query);
    xhr.setRequestHeader("Authorization", "Session " + session());
    xhr.upload.onprogress = (e) => {
      if (!e.lengthComputable) return;
      fill.style.width = (e.loaded / e.total) * 100 + "%";
      label.textContent = e.loaded < e.total ? `${pct(e.loaded, e.total)}%` : "Encrypting and spreading…";
    };
    xhr.onload = () => {
      let body = {};
      try {
        body = JSON.parse(xhr.responseText);
      } catch (_) {}
      if (xhr.status === 201) {
        row.remove();
        toast(`${file.name} stored in ${body.chunks} chunk(s) across ${body.n} Pouches`, "ok");
      } else {
        row.classList.add("failed");
        label.textContent = body.error || xhr.statusText;
        setTimeout(() => row.remove(), 12000);
      }
      refresh();
      resolve();
    };
    xhr.onerror = () => {
      row.classList.add("failed");
      label.textContent = "Connection lost";
      resolve();
    };
    xhr.send(file);
  });
}

async function uploadAll(files) {
  for (const file of files) await upload(file);
}

async function download(id, network, name) {
  try {
    toast(`Fetching ${name}…`);
    const response = await api(`/files/${encodeURIComponent(id)}?network=${encodeURIComponent(network)}`);
    const url = URL.createObjectURL(await response.blob());
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 30000);
  } catch (e) {
    toast(`Cannot download ${name}: ${e.message}`, "error");
  }
}

async function remove(id, network, name) {
  if (!confirm(`Remove ${name}?`)) return;
  try {
    await api(`/files/${encodeURIComponent(id)}?network=${encodeURIComponent(network)}`, { method: "DELETE" });
    toast(`${name} removed`, "ok");
    refresh();
  } catch (e) {
    toast(`Cannot remove ${name}: ${e.message}`, "error");
  }
}

const TIERS = [
  ["T1", "10 GiB"],
  ["T2", "100 GiB"],
  ["T3", "500 GiB"],
  ["T4", "1 TiB"],
  ["T5", "5 TiB"],
];

const SIZES = [
  ["", "Offer the whole tier"],
  [String(512 * MIB), "Offer 512 MiB"],
  [String(1024 * MIB), "Offer 1 GiB"],
  [String(2048 * MIB), "Offer 2 GiB"],
  [String(5120 * MIB), "Offer 5 GiB"],
];

function renderMenu() {
  const menu = $("service-menu");
  if (!state.data) return;
  const network = uploadNetwork();
  if (!network) {
    menu.innerHTML = `<h4>Services</h4><p class="sub">Pick a network at the top first.</p>`;
    return;
  }
  const running = (kind) => state.data.status.local_services.some((s) => s.service_type === kind && s.network_id === network);
  const pouch = running("pouch")
    ? `<div class="option"><strong>Pouch</strong><span class="running">Running on this node</span></div>`
    : `<div class="option"><strong>Pouch</strong><p>Offer disk space to the network. It is filled and challenged before it counts as credit.</p>
        <div class="row"><select id="tier">${TIERS.map(([t, size]) => `<option value="${t}">${t} · ${size}</option>`).join("")}</select></div>
        <div class="row"><select id="size">${SIZES.map(([bytes, label]) => `<option value="${bytes}">${label}</option>`).join("")}</select>
        <button class="button accent small" data-hatch="pouch">Start</button></div></div>`;
  const bill = running("bill")
    ? `<div class="option"><strong>Bill</strong><span class="running">Running on this node</span></div>`
    : `<div class="option"><strong>Bill</strong><p>Your access point to the network's files.</p>
        <div class="row"><button class="button accent small" data-hatch="bill">Start</button></div></div>`;
  menu.innerHTML = `<h4>Services on ${esc(network)}</h4>${pouch}${bill}`;
}

async function hatch(kind) {
  const network = uploadNetwork();
  const metadata = {};
  if (kind === "pouch") {
    metadata.tier = $("tier").value;
    if ($("size").value) metadata.size_bytes = Number($("size").value);
  }
  try {
    await api("/services", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ service_type: kind, network_id: network, metadata }),
    });
    toast(`${kind === "pouch" ? "Pouch" : "Bill"} started on ${network}`, "ok");
    toggleMenu(false);
    refresh();
  } catch (e) {
    toast(`Cannot start the ${kind}: ${e.message}`, "error");
  }
}

function toggleMenu(open) {
  const menu = $("service-menu");
  menu.hidden = !open;
  $("service-button").setAttribute("aria-expanded", String(open));
  if (open) renderMenu();
}

$("service-button").addEventListener("click", (e) => {
  e.stopPropagation();
  toggleMenu($("service-menu").hidden);
});

$("service-menu").addEventListener("click", (e) => {
  e.stopPropagation();
  const b = e.target.closest("button[data-hatch]");
  if (b) {
    b.disabled = true;
    hatch(b.dataset.hatch).finally(() => (b.disabled = false));
  }
});

document.addEventListener("click", () => toggleMenu(false));

$("network").addEventListener("change", (e) => {
  state.network = e.target.value;
  localStorage.setItem(NETWORK_KEY, state.network);
  render();
});

$("node-tabs").addEventListener("click", (e) => {
  const kind = e.target.dataset.kind;
  if (!kind) return;
  state.kind = kind;
  for (const b of $("node-tabs").children) b.classList.toggle("active", b.dataset.kind === kind);
  render();
});

$("files").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  if (b.dataset.download) download(b.dataset.download, b.dataset.network, b.dataset.name);
  if (b.dataset.delete) remove(b.dataset.delete, b.dataset.network, b.dataset.name);
});

$("picker").addEventListener("change", (e) => {
  uploadAll([...e.target.files]);
  e.target.value = "";
});

const panel = $("files-panel");
let depth = 0;
panel.addEventListener("dragenter", (e) => {
  e.preventDefault();
  depth++;
  panel.classList.add("dragging");
});
panel.addEventListener("dragover", (e) => e.preventDefault());
panel.addEventListener("dragleave", () => {
  if (--depth <= 0) {
    depth = 0;
    panel.classList.remove("dragging");
  }
});
panel.addEventListener("drop", (e) => {
  e.preventDefault();
  depth = 0;
  panel.classList.remove("dragging");
  uploadAll([...e.dataTransfer.files].filter((f) => f.size > 0 || f.type));
});

if ("serviceWorker" in navigator) {
  navigator.serviceWorker.register("/sw.js").catch(() => {});
}

refresh();
setInterval(refresh, REFRESH_MS);
