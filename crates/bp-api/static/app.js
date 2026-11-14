"use strict";

const SESSION_KEY = "bp-session";
const NETWORK_KEY = "bp-network";
const VIEW_KEY = "bp-view";
const RATE_KEY = "bp-refresh";
const THEME_KEY = "bp-theme";
const SEEN_KEY = "bp-chat-seen";
const EARNED_KEY = "bp-earned";
const THEMES = [
  ["sunrise", "Sunrise", "Light · orange and ocean"],
  ["lagoon", "Lagoon", "Light · cyan and indigo"],
  ["midnight", "Midnight", "Dark · orange and ocean"],
  ["abyss", "Abyss", "Dark · cyan and indigo"],
];
const BADGE_ICONS = {
  founder: "flag",
  pouch_open: "box",
  big_pouch: "layers",
  deep_pouch: "net",
  generous: "heart",
  rock_solid: "shield",
  first_fish: "fish",
  full_net: "folders",
  recruiter: "user-plus",
  flock_leader: "users",
  good_eye: "eye",
  oracle: "spark",
  everywhere: "devices",
  reliable: "star",
  pillar: "crown",
  month: "calendar",
  year: "trophy",
  chatterbox: "chat",
};
const MIB = 1 << 20;
const HISTORY = 120;
const VIEWS = {
  fish: "Fish",
  flock: "Flock",
  nest: "Nest",
  roost: "Roost",
  gossip: "Gossip",
  profile: "You",
};

const fragment = new URLSearchParams(location.hash.slice(1));
if (fragment.get("session")) {
  localStorage.setItem(SESSION_KEY, fragment.get("session"));
  history.replaceState(null, "", location.pathname);
}

const $ = (id) => document.getElementById(id);

function stored(key, fallback) {
  try {
    return localStorage.getItem(key) || fallback;
  } catch (_) {
    return fallback;
  }
}

function remember(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (_) {}
}

const state = {
  kind: "all",
  network: stored(NETWORK_KEY, ""),
  view: VIEWS[stored(VIEW_KEY, "fish")] ? stored(VIEW_KEY, "fish") : "fish",
  folder: "",
  rate: Number(stored(RATE_KEY, "5000")),
  data: null,
  history: [],
  timer: null,
  chat: { network: null, revision: null, messages: [] },
  member: null,
};

function session() {
  return stored(SESSION_KEY, "");
}

class ApiError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

const native = window.__TAURI__ ? window.__TAURI__.core : null;

async function call(path, options) {
  if (!native) {
    return fetch(path, {
      ...options,
      headers: { Authorization: "Session " + session(), ...(options.headers || {}) },
    });
  }
  const reply = await native.invoke("api", { method: options.method || "GET", path, body: options.body ?? null });
  return new Response(reply.body, { status: reply.status, headers: { "Content-Type": "application/json" } });
}

async function api(path, options = {}) {
  const response = await call(path, options);
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

async function send(path, method, body) {
  const response = await api(path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return response.json().catch(() => ({}));
}

function icon(name, cls = "") {
  return `<svg class="ico ${cls}" aria-hidden="true"><use href="#i-${name}"/></svg>`;
}

function hue(text) {
  let h = 0;
  for (const c of String(text)) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

function avatar(name, fingerprint, cls = "") {
  return `<span class="avatar ${cls}" style="--hue:${hue(fingerprint)}">${esc(initials(name))}</span>`;
}

function tierChip(tier) {
  const cls = { Quarantine: "r0", Reliable: "r2", Trusted: "r3", Pillar: "r4" }[tier] || "";
  return `<span class="chip ${cls}">${esc(tier)}</span>`;
}

function esc(value) {
  return String(value ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

function bytes(n) {
  n = Number(n) || 0;
  if (n >= 2 ** 40) return (n / 2 ** 40).toFixed(1) + " TiB";
  if (n >= 2 ** 30) return (n / 2 ** 30).toFixed(1) + " GiB";
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
  if (s < 86400) return Math.round(s / 3600) + " h ago";
  return Math.round(s / 86400) + " days ago";
}

function short(id) {
  return id ? id.slice(0, 12) + "…" : "";
}

function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}

function toast(message, kind = "") {
  const el = document.createElement("div");
  el.className = "toast " + kind;
  el.innerHTML = (kind === "badge" ? icon("trophy", "l") : "") + `<span>${esc(message)}</span>`;
  $("toasts").append(el);
  setTimeout(() => el.remove(), kind === "error" ? 9000 : 4500);
}

function banner(html) {
  const el = $("banner");
  el.hidden = !html;
  el.innerHTML = html || "";
}

function bar(total, parts) {
  const shown = parts.filter(([value]) => value > 0);
  const sum = shown.reduce((a, [value]) => a + value, 0);
  const rest = Math.max(0, total - sum);
  return `<div class="bar">${[...shown, [rest, "rest"]]
    .filter(([value]) => value > 0)
    .map(([value, cls]) => `<span class="${cls}" style="flex:${value} 0 0"></span>`)
    .join("")}</div>`;
}

function legend(items) {
  return `<div class="legend">${items.map(([cls, text]) => `<span><i class="${cls}"></i>${text}</span>`).join("")}</div>`;
}

function card(title, big, body = "") {
  return `<div class="card"><h3>${title}</h3><div class="big">${big}</div>${body}</div>`;
}

/* ---------- data ---------- */

async function load() {
  const [status, flock, storage, files] = await Promise.all([json("/status"), json("/peers"), json("/storage"), json("/files")]);
  const members = {};
  await Promise.all(
    status.networks.map(async (network) => {
      try {
        members[network] = await json(`/networks/${encodeURIComponent(network)}/members`);
      } catch (_) {}
    }),
  );
  const network = state.network && status.networks.includes(state.network) ? state.network : status.networks.length === 1 ? status.networks[0] : null;
  let chat = null;
  if (network) {
    const known = state.chat.network === network && state.chat.revision !== null ? `?revision=${state.chat.revision}` : "";
    try {
      chat = await json(`/networks/${encodeURIComponent(network)}/chat${known}`);
    } catch (_) {}
  }
  return { status, flock, storage, files, members, chat };
}

function names(data) {
  const out = {};
  for (const m of Object.values(data.members)) {
    for (const member of m.members) out[member.fingerprint] = member.name || short(member.fingerprint);
  }
  return out;
}

function myName(data) {
  return names(data)[data.status.fingerprint] || data.status.alias || "you";
}

function inNetwork(network) {
  return !state.network || state.network === network;
}

function currentNetwork() {
  if (state.network) return state.network;
  const networks = state.data ? state.data.status.networks : [];
  return networks.length === 1 ? networks[0] : null;
}

function nodes(data) {
  const me = data.status.fingerprint;
  const who = names(data);
  const qos = Object.fromEntries(data.flock.qos.map((q) => [q.peer_id, q]));
  const counted = Object.assign({}, ...Object.values(data.members).map((m) => m.availability || {}));
  const local = data.status.local_services.map((svc) => {
    const stat = data.storage.pouches.find((p) => p.service_id === svc.id);
    return {
      kind: svc.service_type,
      network: svc.network_id,
      owner: me,
      name: myName(data),
      mine: true,
      here: true,
      peer: data.status.peer_id,
      paused: svc.status !== "running",
      quota: stat ? stat.quota_bytes : 0,
      stored: stat ? stat.fragment_bytes + stat.filler_bytes : 0,
      data: stat ? stat.fragment_bytes : 0,
      tier: stat ? stat.tier : svc.metadata.tier,
      seen: null,
      qos: null,
      counted: counted[data.status.peer_id],
    };
  });
  const remote = data.flock.known_peers.map((n) => ({
    kind: n.service_type,
    network: n.network_id,
    owner: n.user_fingerprint,
    name: who[n.user_fingerprint] || n.user_alias || short(n.user_fingerprint),
    mine: n.user_fingerprint === me,
    here: false,
    peer: n.peer_id,
    paused: Boolean(n.metadata.paused),
    evicting: Boolean(n.metadata.evicting),
    quota: Number(n.metadata.storage_bytes) || 0,
    stored: Number(n.metadata.stored_bytes) || 0,
    data: 0,
    tier: n.metadata.tier,
    seen: n.announced_at,
    qos: qos[n.peer_id] || null,
    counted: counted[n.peer_id],
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
  if (node.here) {
    const c = data.storage.credits.find((c) => c.network_id === node.network) || {};
    if (!c.credit_bytes) return ["wait", "Waiting for the network to verify it"];
    return ["ok", `All your Pouches: ${bytes(c.credit_bytes)} verified · quota ${bytes(c.quota_verified_bytes)}`];
  }
  if (node.mine) return ["idle", "Your other device: others challenge it"];
  const q = node.qos;
  if (!q || q.challenges_sent === 0) return ["wait", "Not challenged yet"];
  const rate = Math.round(q.challenge_success_ewma * 100);
  const counted = node.counted === undefined ? "not counted by the network yet" : `the network counts it ${(node.counted * 100).toFixed(1)}% available`;
  const detail = `${q.challenges_succeeded}/${q.challenges_sent} of your challenges passed (${rate}%) · ${counted}`;
  if (q.challenge_success_ewma >= 0.9) return ["ok", detail];
  if (q.challenge_success_ewma >= 0.5) return ["wait", detail];
  return ["bad", detail];
}

function nodeCards(list, data) {
  if (!list.length) return `<div class="empty">No nodes here yet.</div>`;
  return list
    .map((n) => {
      const [cls, text] = verification(n, data);
      const setup =
        n.kind === "pouch"
          ? `${bar(n.quota, [[n.data, "data"], [Math.max(0, n.stored - n.data), "filler"]])}
             <div class="meta">${n.tier ? esc(n.tier) + " · " : ""}setup ${pct(n.stored, n.quota)}% — ${bytes(n.stored)} of ${bytes(n.quota)}</div>`
          : "";
      return `<div class="node ${n.mine ? "mine" : ""}">
        <div class="node-top"><span class="name">${esc(n.name)}</span>${n.here ? '<span class="you">this node</span>' : n.mine ? '<span class="you">you</span>' : ""}<span class="kind ${esc(n.kind)}">${icon({ pouch: "box", bill: "fish", post: "relay" }[n.kind] || "box", "s")}${esc(n.kind)}</span></div>
        <div class="meta">${esc(n.network)} · <code>${esc(short(n.peer))}</code></div>
        ${setup}
        <div class="state ${cls}">${esc(text)}</div>
      </div>`;
    })
    .join("");
}

function quota(data) {
  const credits = data.storage.credits.filter((c) => inNetwork(c.network_id));
  const offered = credits.reduce((a, c) => a + c.offered_bytes, 0);
  const total = credits.reduce((a, c) => a + c.quota_bytes, 0);
  const verified = Math.min(total, credits.reduce((a, c) => a + c.quota_verified_bytes, 0));
  const used = Math.min(verified, credits.reduce((a, c) => a + c.quota_used_bytes, 0));
  const rates = [...new Set(credits.filter((c) => c.rate).map((c) => c.rate.join("/")))];
  return {
    offered,
    total,
    used,
    free: Math.max(0, verified - used),
    pending: Math.max(0, total - verified),
    lost: Math.max(0, offered - total),
    rates,
  };
}

/* ---------- Fish ---------- */

function renderFish(data) {
  const network = currentNetwork();
  const folders = data.storage.folders.filter((f) => !network || f.network_id === network);
  if (state.folder && !folders.some((f) => f.tier === state.folder)) state.folder = "";
  const all = `<button class="folder ${state.folder ? "" : "selected"}" data-folder="">
      <span class="tile">${icon("folders", "l")}</span>
      <strong>All folders</strong>
      <span>${plural(data.files.files.filter((f) => inNetwork(f.network_id)).length, "file", "files")}</span>
    </button>`;
  $("folders").innerHTML =
    all +
    folders
      .map(
        (f) => `<button class="folder ${f.active ? "" : "waiting"} ${state.folder === f.tier ? "selected" : ""}" data-folder="${esc(f.tier)}">
          <span class="tile">${icon("folder", "l")}</span>
          <strong>${esc(f.tier)}${network ? "" : ` <small>${esc(f.network_id)}</small>`}</strong>
          <span>${plural(f.users, "user", "users")} · ${plural(f.pouches, "Pouch", "Pouches")} · ${bytes(f.free_bytes)} free</span>
          <span>${f.active ? `k/n ${f.k}/${f.n} · recovery ${(f.pe * 100).toFixed(2)}%` : "local only until 3 users have a Pouch here"}</span>
        </button>`,
      )
      .join("");

  const q = quota(data);
  $("quota-line").innerHTML = q.rates.length ? `quota <b>${bytes(q.free)}</b> free of ${bytes(q.total)}` : "";
  const target = state.folder || "T1";
  $("files-title").textContent = state.folder ? `Folder ${state.folder}` : "All files";
  $("drop-hint").textContent = network
    ? `Uploads go to folder ${target} of ${network}. They are compressed and encrypted on this device, split and spread across its Pouches.`
    : "Pick a network at the top to upload.";

  const who = names(data);
  const files = data.files.files
    .filter((f) => inNetwork(f.network_id) && (!state.folder || f.tier === state.folder))
    .sort((a, b) => b.uploaded_at - a.uploaded_at);
  if (!files.length) {
    $("files").innerHTML = `<div class="empty">No files here yet. Drop one above.</div>`;
    return;
  }
  $("files").innerHTML = `<table>
    <thead><tr><th>Name</th><th>Size</th><th>Owner</th><th>Uploaded</th><th></th></tr></thead>
    <tbody>${files
      .map(
        (f) => `<tr>
          <td class="fname"><div class="file-name">${fileKind(f.file_name)}<div>${esc(f.file_name)}<small>${esc(f.network_id)}${f.tier ? ` · ${esc(f.tier)}${f.local ? " (local only)" : ""}` : ""} · ${plural(f.chunks, "chunk", "chunks")}${f.mine && f.shared_with ? ` · shared with ${f.shared_with}` : ""}</small></div></div></td>
          <td>${bytes(f.size_bytes)}</td>
          <td>${f.mine ? '<span class="owner-you">you</span>' : esc(who[f.owner] || short(f.owner))}</td>
          <td>${new Date(f.uploaded_at * 1000).toLocaleString()}</td>
          <td class="actions">
            ${!native && playable(f.file_name) ? `<button class="button small" data-play="${esc(f.file_id)}" data-network="${esc(f.network_id)}" data-name="${esc(f.file_name)}">${icon("play", "s")}Play</button>` : ""}
            <button class="button small" data-download="${esc(f.file_id)}" data-network="${esc(f.network_id)}" data-name="${esc(f.file_name)}">${icon("download", "s")}Download</button>
            <button class="button small ghost danger" title="${f.mine ? "Delete" : "Remove"}" aria-label="${f.mine ? "Delete" : "Remove"}" data-delete="${esc(f.file_id)}" data-network="${esc(f.network_id)}" data-name="${esc(f.file_name)}">${icon("trash", "s")}</button>
          </td>
        </tr>`,
      )
      .join("")}</tbody></table>`;
}

const IMAGES = ["jpg", "jpeg", "png", "gif", "webp", "heic", "svg", "bmp"];
const AUDIO = ["mp3", "m4a", "ogg", "oga", "opus", "wav", "flac"];

function fileKind(name) {
  const ext = String(name).split(".").pop().toLowerCase();
  if (IMAGES.includes(ext)) return `<span class="file-kind image">${icon("image")}</span>`;
  if (AUDIO.includes(ext)) return `<span class="file-kind media">${icon("music")}</span>`;
  if (playable(name)) return `<span class="file-kind media">${icon("film")}</span>`;
  return `<span class="file-kind">${icon("file")}</span>`;
}

/* ---------- Flock ---------- */

function networkAvailability(data) {
  const values = Object.entries(data.members)
    .filter(([network]) => inNetwork(network))
    .flatMap(([, m]) => Object.values(m.availability || {}));
  return values.length ? values.reduce((a, v) => a + v, 0) / values.length : null;
}

function sample(data) {
  const online = nodes(data).filter((n) => !n.paused && !n.evicting).length;
  state.history.push({ at: Date.now(), online, availability: networkAvailability(data) });
  if (state.history.length > HISTORY) state.history.shift();
}

function lineChart(el, points, format, max) {
  const values = points.map((p) => p.value).filter((v) => v !== null);
  if (values.length < 2) {
    el.innerHTML = `<div class="chart-value">${values.length ? format(values[0]) : "—"}</div><div class="empty">Collecting live data…</div>`;
    return;
  }
  const w = 600;
  const h = 180;
  const pad = { l: 40, r: 10, t: 10, b: 22 };
  const top = max ?? Math.max(1, ...values) * 1.15;
  const x = (i) => pad.l + (i / (points.length - 1)) * (w - pad.l - pad.r);
  const y = (v) => pad.t + (1 - v / top) * (h - pad.t - pad.b);
  const kept = points.map((p, i) => [i, p.value]).filter(([, v]) => v !== null);
  const path = kept.map(([i, v], n) => `${n ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const area = `${path}L${x(kept[kept.length - 1][0]).toFixed(1)},${y(0)}L${x(kept[0][0]).toFixed(1)},${y(0)}Z`;
  const ticks = [0, top / 2, top];
  const last = kept[kept.length - 1];
  el.innerHTML = `<div class="chart-value">${format(last[1])} <small>now</small></div>
    <svg viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" role="img" aria-label="${esc(el.dataset.label || "")}">
      ${ticks.map((t) => `<line class="grid" x1="${pad.l}" x2="${w - pad.r}" y1="${y(t)}" y2="${y(t)}"/><text class="tick" x="${pad.l - 6}" y="${y(t) + 4}" text-anchor="end">${format(t)}</text>`).join("")}
      <line class="axis" x1="${pad.l}" x2="${w - pad.r}" y1="${y(0)}" y2="${y(0)}"/>
      <path class="area" d="${area}"/>
      <path class="line" d="${path}"/>
      <circle class="dot" cx="${x(last[0])}" cy="${y(last[1])}" r="4"/>
      <line class="cross" id="${el.id}-cross" y1="${pad.t}" y2="${y(0)}" visibility="hidden"/>
      <rect x="${pad.l}" y="0" width="${w - pad.l - pad.r}" height="${h}" fill="transparent" data-hover="${el.id}"/>
    </svg>`;
  const svg = el.querySelector("svg");
  const cross = el.querySelector(".cross");
  svg.onmousemove = (e) => {
    const box = svg.getBoundingClientRect();
    const ratio = ((e.clientX - box.left) / box.width) * w;
    const i = Math.round(((ratio - pad.l) / (w - pad.l - pad.r)) * (points.length - 1));
    const p = points[Math.max(0, Math.min(points.length - 1, i))];
    if (!p || p.value === null) return;
    cross.setAttribute("x1", x(points.indexOf(p)));
    cross.setAttribute("x2", x(points.indexOf(p)));
    cross.setAttribute("visibility", "visible");
    showTip(e, `${format(p.value)}<br><small>${new Date(p.at).toLocaleTimeString()}</small>`);
  };
  svg.onmouseleave = () => {
    cross.setAttribute("visibility", "hidden");
    hideTip();
  };
}

function showTip(e, html) {
  const tip = $("tooltip");
  tip.innerHTML = html;
  tip.hidden = false;
  tip.style.left = Math.min(window.innerWidth - 160, e.clientX + 12) + "px";
  tip.style.top = e.clientY + 12 + "px";
}

function hideTip() {
  $("tooltip").hidden = true;
}

function memberBars(data) {
  const rows = Object.entries(data.members)
    .filter(([network]) => inNetwork(network))
    .flatMap(([network, m]) => m.members.map((u) => ({ ...u, network })));
  if (!rows.length) return `<div class="empty">No members yet.</div>`;
  const top = Math.max(1, ...rows.flatMap((u) => [u.credit_bytes, u.used_bytes]));
  const width = (v) => `${Math.max(0.5, (v / top) * 85)}%`;
  return `${legend([
    ["hosts", "hosts (verified by the others)"],
    ["stores", "stores on the others"],
  ])}<div class="hbars" style="margin-top:12px">${rows
    .sort((a, b) => b.credit_bytes - a.credit_bytes)
    .map(
      (u) => `<div class="hbar">
        <span class="who" title="${esc(u.fingerprint)}">${esc(u.name || short(u.fingerprint))}${state.network ? "" : ` <small>${esc(u.network)}</small>`}</span>
        <span class="tracks">
          <span class="track"><span class="fill hosts" style="width:${width(u.credit_bytes)}"></span>${bytes(u.credit_bytes)}</span>
          <span class="track"><span class="fill stores" style="width:${width(u.used_bytes)}"></span>${bytes(u.used_bytes)}</span>
        </span>
      </div>`,
    )
    .join("")}</div>`;
}

function membersTable(data) {
  const shown = Object.entries(data.members).filter(([network]) => inNetwork(network));
  if (!shown.length) return `<div class="empty">You are in no network yet.</div>`;
  return shown
    .map(([network, m]) => {
      const p = m.params;
      const params = p
        ? `<div class="params">target recovery <b>${(p.ph_ppm / 1e4).toFixed(2)}%</b> · trust decay <b>${(p.decay_ppm / 1e6).toFixed(2)}</b> · bets last <b>${plural(p.horizon_epochs, "epoch", "epochs")}</b> · a silent device counts for <b>${plural(p.grace_epochs, "epoch", "epochs")}</b></div>`
        : "";
      const rows = m.members
        .map(
          (u) => `<tr>
            <td class="fname">${esc(u.name || short(u.fingerprint))}${u.fingerprint === m.me ? ' <span class="owner-you">you</span>' : ""}<small><code>${esc(short(u.fingerprint))}</code> · ${plural(u.devices, "device", "devices")}${u.expel_approvals ? ` · expulsion: ${u.expel_approvals} approval(s)` : ""}</small></td>
            <td>${tierChip(u.tier)} <small>weight ${u.weight}</small></td>
            <td>${bytes(u.credit_bytes)}</td>
            <td>${bytes(u.used_bytes)}</td>
            <td class="${u.trust > 0 ? "up" : u.trust < 0 ? "down" : ""}">${u.trust >= 0 ? "+" : ""}${u.trust.toFixed(2)}${u.open_bets || u.sponsoring ? ` <small>${u.locked_trust.toFixed(2)} locked · ${plural(u.open_bets, "bet", "bets")} · ${plural(u.sponsoring, "invitation", "invitations")}</small>` : ""}</td>
          </tr>`,
        )
        .join("");
      return `<div class="network-block">
        <h3>${esc(network)} <small>ledger height ${m.height} · epoch ${m.epoch}</small></h3>
        ${params}
        <table>
          <thead><tr><th>Member</th><th>Tier · weight</th><th>Hosts</th><th>Stores</th><th>Trust</th></tr></thead>
          <tbody>${rows}</tbody>
        </table>
      </div>`;
    })
    .join("");
}

function renderFlock(data) {
  const all = nodes(data);
  const count = (kind) => all.filter((n) => n.kind === kind).length;
  const members = Object.entries(data.members)
    .filter(([network]) => inNetwork(network))
    .reduce((a, [, m]) => a + m.members.length, 0);
  const capacity = all.filter((n) => n.kind === "pouch").reduce((a, n) => a + n.quota, 0);
  const stored = all.filter((n) => n.kind === "pouch").reduce((a, n) => a + n.stored, 0);
  const availability = networkAvailability(data);
  const ledgers = Object.entries(data.members).filter(([network]) => inNetwork(network));
  $("flock-kpis").innerHTML = [
    card("Members", members, `<div class="sub">${plural(ledgers.length, "network", "networks")}</div>`),
    card("Nodes", all.length, `<div class="sub">${plural(count("pouch"), "Pouch", "Pouches")} · ${plural(count("bill"), "Bill", "Bills")} · ${plural(count("post"), "Post", "Posts")}</div>`),
    card("Space offered", bytes(capacity), `${bar(capacity, [[stored, "data"]])}<div class="sub">${bytes(stored)} filled and challenged</div>`),
    card("Availability", availability === null ? "—" : (availability * 100).toFixed(1) + "%", `<div class="sub">average the network counts for its devices</div>`),
  ].join("");
  lineChart(
    $("chart-online"),
    state.history.map((p) => ({ at: p.at, value: p.online })),
    (v) => String(Math.round(v)),
  );
  lineChart(
    $("chart-availability"),
    state.history.map((p) => ({ at: p.at, value: p.availability })),
    (v) => (v * 100).toFixed(1) + "%",
    1,
  );
  $("chart-members").innerHTML = memberBars(data);
  $("network-info").innerHTML = membersTable(data);
  const list = all
    .filter((n) => state.kind === "all" || n.kind === state.kind)
    .sort((a, b) => Number(b.mine) - Number(a.mine) || a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
  $("nodes").innerHTML = nodeCards(list, data);
}

/* ---------- Nest ---------- */

function renderNest(data) {
  const q = quota(data);
  const quotaCard = !q.offered
    ? card("Your quota", "—", `<div class="sub">Start a Pouch below to get a quota.</div>`)
    : !q.rates.length
      ? card("Your quota", "—", `${bar(q.offered, [[q.offered, "pending"]])}<div class="sub">Waiting for enough Pouches to set k/n.</div>`)
      : card(
          "Your quota",
          `${bytes(q.free)} <small>free</small>`,
          `${bar(q.offered, [[q.used, "used"], [q.free, "left"], [q.pending, "pending"], [q.lost, "lost"]])}
           ${legend([
             ["used", `used ${bytes(q.used)}`],
             ["left", `free ${bytes(q.free)}`],
             ["pending", `not verified yet ${bytes(q.pending)}`],
             ["lost", `redundancy ${bytes(q.lost)} (${pct(q.lost, q.offered)}%)`],
           ])}
           <div class="sub">Quota ${bytes(q.total)} of the ${bytes(q.offered)} you offer · k/n ${q.rates.join(", ")}</div>`,
        );
  const mine = data.files.files.filter((f) => f.mine && inNetwork(f.network_id));
  const local = mine.filter((f) => f.local).length;
  const shared = mine.filter((f) => f.shared_with).length;
  const own = nodes(data).filter((n) => n.mine);
  const devices = new Set(own.map((n) => n.peer)).size;
  $("nest-cards").innerHTML = [
    quotaCard,
    card("Your files", mine.length, `<div class="sub">${bytes(mine.reduce((a, f) => a + f.size_bytes, 0))} · ${plural(shared, "shared", "shared")}${local ? ` · ${local} local only` : ""}</div>`),
    card("Your devices", devices, `<div class="sub">${plural(own.length, "service", "services")} running across them</div>`),
  ].join("");

  const me = data.status.fingerprint;
  const rows = Object.entries(data.members)
    .filter(([network]) => inNetwork(network))
    .map(([network, m]) => [network, m.members.find((u) => u.fingerprint === me)])
    .filter(([, u]) => u);
  $("nest-reputation").innerHTML = rows.length
    ? `<table><thead><tr><th>Network</th><th>Tier · weight</th><th>Hosts</th><th>Stores</th><th>Trust</th></tr></thead><tbody>${rows
        .map(
          ([network, u]) => `<tr>
            <td class="fname">${esc(network)}<small>member since ${new Date(u.joined_at * 1000).toLocaleDateString()}</small></td>
            <td>${tierChip(u.tier)} <small>weight ${u.weight}</small></td>
            <td>${bytes(u.credit_bytes)}</td>
            <td>${bytes(u.used_bytes)}</td>
            <td class="${u.trust > 0 ? "up" : u.trust < 0 ? "down" : ""}">${u.trust >= 0 ? "+" : ""}${u.trust.toFixed(2)} <small>${u.locked_trust.toFixed(2)} locked · ${plural(u.open_bets, "bet", "bets")} · ${plural(u.sponsoring, "invitation", "invitations")}</small></td>
          </tr>`,
        )
        .join("")}</tbody></table>`
    : `<div class="empty">You are in no network yet.</div>`;
  $("nest-nodes").innerHTML = nodeCards(own, data);
  renderHatch(data);
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

function renderHatch(data) {
  $("hatch-panel").hidden = Boolean(native);
  const network = currentNetwork();
  if (!network) {
    $("hatch").innerHTML = `<div class="empty">Pick a network at the top first.</div>`;
    return;
  }
  const running = (kind) => data.status.local_services.some((s) => s.service_type === kind && s.network_id === network);
  const wanted = `${network}|${running("pouch")}|${running("bill")}`;
  if ($("hatch").dataset.shown === wanted) return;
  $("hatch").dataset.shown = wanted;
  const pouch = running("pouch")
    ? `<div class="option"><strong>Pouch</strong><span class="running">Running on this node</span></div>`
    : `<div class="option"><strong>Pouch</strong><p>Offer disk space to ${esc(network)}. It is filled and challenged before it counts as credit.</p>
        <div class="row"><select id="tier">${TIERS.map(([t, size]) => `<option value="${t}">${t} · ${size}</option>`).join("")}</select></div>
        <div class="row"><select id="size">${SIZES.map(([b, label]) => `<option value="${b}">${label}</option>`).join("")}</select>
        <button class="button accent small" data-hatch="pouch">Start</button></div></div>`;
  const bill = running("bill")
    ? `<div class="option"><strong>Bill</strong><span class="running">Running on this node</span></div>`
    : `<div class="option"><strong>Bill</strong><p>Your access point to the files of ${esc(network)}.</p>
        <div class="row"><button class="button accent small" data-hatch="bill">Start</button></div></div>`;
  $("hatch").innerHTML = `<div class="hatch-options">${pouch}${bill}</div>`;
}

async function hatch(kind) {
  const network = currentNetwork();
  const metadata = {};
  if (kind === "pouch") {
    metadata.tier = $("tier").value;
    if ($("size").value) metadata.size_bytes = Number($("size").value);
  }
  try {
    await send("/services", "POST", { service_type: kind, network_id: network, metadata });
    toast(`${kind === "pouch" ? "Pouch" : "Bill"} started on ${network}`, "ok");
    $("hatch").dataset.shown = "";
    refresh();
  } catch (e) {
    toast(`Cannot start the ${kind}: ${e.message}`, "error");
  }
}

/* ---------- Roost ---------- */

function renderRoost(data) {
  const s = data.status;
  const services = s.local_services.filter((svc) => inNetwork(svc.network_id));
  const oldest = s.local_services.reduce((a, svc) => Math.min(a, Date.parse(svc.started_at) / 1000), Infinity);
  const qos = s.network_qos;
  $("roost-cards").innerHTML = [
    card("This node", esc(s.alias || myName(data)), `<div class="sub"><code>${esc(short(s.peer_id))}</code> · version ${esc(s.version)}</div>`),
    card("Services", services.length, `<div class="sub">${Number.isFinite(oldest) ? "up since " + ago(oldest) : "none running"}</div>`),
    card("Peers", s.known_peers, `<div class="sub">${qos ? `${plural(qos.observed_peers, "peer", "peers")} observed · ${(qos.avg_availability * 100).toFixed(1)}% available on average, as seen from here` : "none observed yet"}</div>`),
  ].join("");

  const ledgers = Object.entries(data.members).filter(([network]) => inNetwork(network));
  $("roost-sync").innerHTML = ledgers.length
    ? `<table><thead><tr><th>Network</th><th>Ledger height</th><th>Last block</th><th>Pending ops</th></tr></thead><tbody>${ledgers
        .map(
          ([network, m]) => `<tr>
            <td class="fname">${esc(network)}<small>epoch ${m.epoch}</small></td>
            <td>${m.height}</td>
            <td>${m.time ? ago(m.time) : "—"}</td>
            <td>${m.pending_ops ? `<span class="state wait">${m.pending_ops} waiting for a block</span>` : '<span class="state ok">in sync</span>'}</td>
          </tr>`,
        )
        .join("")}</tbody></table>`
    : `<div class="empty">You are in no network yet.</div>`;

  $("roost-services").innerHTML = services.length
    ? services
        .map((svc) => {
          const paused = svc.status !== "running";
          const stat = data.storage.pouches.find((p) => p.service_id === svc.id);
          const detail = stat
            ? `${esc(stat.tier)} · ${bytes(stat.fragment_bytes)} of files and ${bytes(stat.filler_bytes)} of filler in ${bytes(stat.quota_bytes)}`
            : `started ${ago(Date.parse(svc.started_at) / 1000)}`;
          return `<div class="service">
            <span class="what">${esc(svc.service_type)} on ${esc(svc.network_id)}<small>${detail}</small><span class="state ${paused ? "wait" : "ok"}">${paused ? "in maintenance" : "running"}</span></span>
            <span class="row">
              ${paused ? `<button class="button small" data-resume="${esc(svc.id)}">Resume</button>` : `<button class="button small" data-pause="${esc(svc.id)}">Maintenance</button>`}
              <button class="button small danger" data-stop="${esc(svc.id)}">Stop</button>
              ${svc.service_type === "pouch" ? `<button class="button small danger" data-evict="${esc(svc.id)}">Evict</button>` : ""}
            </span>
          </div>`;
        })
        .join("")
    : `<div class="empty">No service runs on this node${state.network ? " in this network" : ""}. Start one from Nest.</div>`;

  $("roost-addresses").innerHTML = s.addresses.length
    ? `<ul class="addresses">${s.addresses.map((a) => `<li><code>${esc(a)}</code></li>`).join("")}</ul>`
    : `<div class="empty">This node only dials out.</div>`;
  $("leave-row").innerHTML = s.networks
    .filter((n) => inNetwork(n))
    .map((n) => `<button class="button danger" data-leave="${esc(n)}">Leave ${esc(n)}</button>`)
    .join("");
}

async function serviceAction(button) {
  const { pause, resume, stop, evict } = button.dataset;
  try {
    if (pause) {
      const eta = prompt("Maintenance for how many minutes?", "30");
      if (eta === null) return;
      await send(`/services/${encodeURIComponent(pause)}/pause`, "POST", { eta_minutes: Number(eta) || 30 });
      toast("In maintenance: the others know you will be back", "ok");
    } else if (resume) {
      await send(`/services/${encodeURIComponent(resume)}/resume`, "POST");
      toast("Back online", "ok");
    } else if (stop) {
      if (!confirm("Stop this service?")) return;
      await send(`/services/${encodeURIComponent(stop)}`, "DELETE");
      toast("Service stopped", "ok");
    } else if (evict) {
      if (!confirm("Evict this Pouch? Its fragments are deleted and the network repairs what it held.")) return;
      await send(`/services/${encodeURIComponent(evict)}?evict=true`, "DELETE");
      toast("Pouch evicted", "ok");
    }
    refresh();
  } catch (e) {
    toast(e.message, "error");
  }
}

async function leave(network) {
  if (!confirm(`Leave ${network}? Your Pouches are evicted and your files there are deleted.`)) return;
  try {
    await send("/networks/leave", "POST", { network_id: network, force: true });
    toast(`You left ${network}`, "ok");
    refresh();
  } catch (e) {
    toast(e.message, "error");
  }
}

/* ---------- You ---------- */

function initials(name) {
  return (
    String(name)
      .split(/[\s._-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0].toUpperCase())
      .join("") || "?"
  );
}

function renderMe(data) {
  const name = myName(data);
  $("avatar").textContent = initials(name);
  $("avatar").style.setProperty("--hue", hue(data.status.fingerprint));
  $("me-name").textContent = name;
  $("me-fp").textContent = short(data.status.fingerprint);
}

function renderProfile(data) {
  const s = data.status;
  const name = myName(data);
  const rows = Object.entries(data.members)
    .map(([network, m]) => [network, m.members.find((u) => u.fingerprint === s.fingerprint)])
    .filter(([, u]) => u);
  $("profile").innerHTML = `
    ${avatar(name, s.fingerprint, "big")}
    <span><h2>${esc(name)}</h2><code>${esc(s.fingerprint)}</code></span>
    <div class="networks files">${
      rows.length
        ? `<table><thead><tr><th>Network</th><th>Tier</th><th>Trust</th><th>Devices</th></tr></thead><tbody>${rows
            .map(([network, u]) => `<tr><td class="fname">${esc(network)}<small>level ${u.level.number} · ${esc(u.level.name)} · ${u.level.points} points</small></td><td>${tierChip(u.tier)}</td><td>${u.trust >= 0 ? "+" : ""}${u.trust.toFixed(2)}</td><td>${u.devices}</td></tr>`)
            .join("")}</tbody></table>`
        : `<div class="empty">You are in no network yet.</div>`
    }</div>`;
  renderThemes();
  $("refresh-rate").value = String(state.rate);
  $("sign-out").hidden = Boolean(native);
}

function renderThemes() {
  const current = document.documentElement.dataset.theme;
  $("themes").innerHTML = THEMES.map(
    ([id, name, about]) => `<button class="theme ${id === current ? "active" : ""}" data-theme="${id}" data-pick="${id}">
      <span class="preview"><i></i><b><u></u><u></u><u></u></b></span>
      <span>${name}<small>${about}</small></span>
    </button>`,
  ).join("");
}

/* ---------- Gossip ---------- */

function keepChat(data) {
  const network = currentNetwork();
  if (!network) {
    state.chat = { network: null, revision: null, messages: [] };
    return;
  }
  if (!data.chat || data.chat.network_id !== network) return;
  state.chat = {
    network,
    revision: data.chat.revision,
    messages: data.chat.messages === null ? state.chat.messages : data.chat.messages,
  };
}

function seen() {
  try {
    return JSON.parse(stored(SEEN_KEY, "{}"));
  } catch (_) {
    return {};
  }
}

function unread() {
  const { network, messages } = state.chat;
  if (!network) return 0;
  const last = seen()[network] || 0;
  return messages.filter((m) => !m.mine && m.at > last).length;
}

function markRead() {
  const { network, messages } = state.chat;
  if (!network || !messages.length) return;
  const all = seen();
  const newest = messages[messages.length - 1].at;
  if (all[network] === newest) return;
  all[network] = newest;
  remember(SEEN_KEY, JSON.stringify(all));
}

function renderUnread() {
  if (state.view === "gossip" && !document.hidden) markRead();
  const n = unread();
  $("unread").hidden = !n;
  $("unread").textContent = n > 99 ? "99+" : String(n);
}

function scrollChat(force) {
  const log = $("chat-log");
  if (force || log.scrollHeight - log.scrollTop - log.clientHeight < 140) log.scrollTop = log.scrollHeight;
}

function dayLabel(at) {
  const day = new Date(at * 1000);
  const today = new Date();
  const yesterday = new Date(Date.now() - 86400000);
  if (day.toDateString() === today.toDateString()) return "Today";
  if (day.toDateString() === yesterday.toDateString()) return "Yesterday";
  return day.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
}

function renderChat(data) {
  const network = currentNetwork();
  const log = $("chat-log");
  $("chat-form").hidden = !network;
  if (!network) {
    $("chat-title").textContent = "Chat";
    $("chat-sub").textContent = "";
    log.innerHTML = `<div class="empty">Pick a network at the top to open its chat.</div>`;
    log.dataset.shown = "";
    return;
  }
  const members = data.members[network] ? data.members[network].members.length : 0;
  $("chat-title").textContent = `Chat of ${network}`;
  $("chat-sub").textContent = `${plural(members, "member", "members")} · signed, and encrypted with the network key`;
  const shown = `${network}|${state.chat.revision}|${state.chat.messages.length}`;
  if (log.dataset.shown === shown) return;
  const first = log.dataset.shown === undefined || !log.dataset.shown.startsWith(network + "|");
  log.dataset.shown = shown;
  if (!state.chat.messages.length) {
    log.innerHTML = `<div class="empty">Nobody has written yet. Say hello to the flock.</div>`;
    return;
  }
  const who = names(data);
  let previous = null;
  log.innerHTML = state.chat.messages
    .map((m) => {
      const day = dayLabel(m.at);
      const newDay = !previous || dayLabel(previous.at) !== day;
      const same = !newDay && previous.author === m.author && m.at - previous.at < 300;
      previous = m;
      const name = m.name || who[m.author] || short(m.author);
      const time = new Date(m.at * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
      return `${newDay ? `<div class="chat-day">${esc(day)}</div>` : ""}
        <div class="msg ${m.mine ? "mine" : ""} ${same ? "same" : ""}">
          <span class="who">${m.mine ? "" : `<b data-member="${esc(m.author)}">${esc(name)}</b> · `}${time}</span>
          <span data-member="${esc(m.author)}">${avatar(name, m.author, "s")}</span>
          <span class="bubble" title="${esc(new Date(m.at * 1000).toLocaleString())}">${esc(m.text)}</span>
        </div>`;
    })
    .join("");
  scrollChat(first || state.chat.messages[state.chat.messages.length - 1].mine);
}

function ranked(data, network) {
  const m = data.members[network];
  return m ? [...m.members].sort((a, b) => b.level.points - a.level.points || b.trust - a.trust || a.joined_at - b.joined_at) : [];
}

function levelPanel(u) {
  const { number, name, points, floor, next } = u.level;
  const value = next ? Math.round(((points - floor) / (next - floor)) * 100) : 100;
  const earned = u.achievements.filter((a) => a.earned).length;
  return `<div class="level">
    <span class="ring" style="--value:${value}"><span>${number}</span></span>
    <span class="eyebrow">Your level</span>
    <strong>${esc(name)}</strong>
    <span class="sub">${points} points · ${next ? `${next - points} to the next level` : "the highest level"} · ${earned}/${u.achievements.length} badges</span>
  </div>`;
}

function badgeCard(a) {
  return `<div class="badge ${a.earned ? "earned" : ""}">
    <span class="medal">${icon(a.earned ? BADGE_ICONS[a.id] || "trophy" : "lock", "l")}</span>
    <strong>${esc(a.name)}<small>+${a.points}</small></strong>
    <span>${esc(a.description)}</span>
    ${a.earned ? "" : `<div class="bar">${a.progress_percent ? `<span class="used" style="flex:${a.progress_percent} 0 0"></span>` : ""}<span class="rest" style="flex:${100 - a.progress_percent} 0 0"></span></div>`}
  </div>`;
}

function renderGossip(data) {
  renderChat(data);
  const network = currentNetwork();
  const list = network ? ranked(data, network) : [];
  const me = list.find((u) => u.fingerprint === data.status.fingerprint);
  $("level-panel").hidden = !me;
  if (me) $("level-panel").innerHTML = levelPanel(me);
  $("roster").innerHTML = list.length
    ? list
        .map(
          (u, i) => `<button class="mate" data-member="${esc(u.fingerprint)}">
            <span class="rank">${i + 1}</span>
            ${avatar(u.name || "?", u.fingerprint)}
            <span class="who"><b>${esc(u.name || short(u.fingerprint))}${u.fingerprint === data.status.fingerprint ? ' <span class="owner-you">you</span>' : ""}</b><small>Level ${u.level.number} · ${esc(u.level.name)} · ${esc(u.tier)}</small></span>
            <span class="pts">${u.level.points} <small>pts</small></span>
          </button>`,
        )
        .join("")
    : `<div class="empty">Pick a network at the top to see its members.</div>`;
  if (!me) {
    $("badges-sub").textContent = "";
    $("badges").innerHTML = `<div class="empty">Pick a network at the top to see your badges.</div>`;
    return;
  }
  const sorted = [...me.achievements].sort((a, b) => Number(b.earned) - Number(a.earned) || b.progress_percent - a.progress_percent);
  $("badges-sub").textContent = `${sorted.filter((a) => a.earned).length} of ${sorted.length} earned in ${network}`;
  $("badges").innerHTML = sorted.map(badgeCard).join("");
}

function renderMember(data) {
  const { network, fingerprint } = state.member;
  const u = data.members[network] && data.members[network].members.find((x) => x.fingerprint === fingerprint);
  if (!u) {
    closeMember();
    return;
  }
  const name = u.name || short(u.fingerprint);
  const rate = u.challenges_sent ? ((u.challenges_passed / u.challenges_sent) * 100).toFixed(1) + "%" : "—";
  const earned = u.achievements.filter((a) => a.earned);
  const shown = `${fingerprint}|${u.level.points}|${u.trust}|${u.credit_bytes}|${u.used_bytes}|${u.challenges_sent}|${u.messages}`;
  if ($("member-box").dataset.shown === shown) return;
  $("member-box").dataset.shown = shown;
  $("member-box").innerHTML = `
    <div class="sheet-head">
      ${avatar(name, u.fingerprint, "big")}
      <h2>${esc(name)}</h2>
      <button class="button ghost small" data-close aria-label="Close">${icon("close")}</button>
      <span class="row">${tierChip(u.tier)}<span class="chip">Level ${u.level.number} · ${esc(u.level.name)}</span><span class="sub">${u.level.points} points</span></span>
    </div>
    <code>${esc(u.fingerprint)}</code>
    <div class="stats">
      <div class="stat"><b class="${u.trust > 0 ? "up" : u.trust < 0 ? "down" : ""}">${u.trust >= 0 ? "+" : ""}${u.trust.toFixed(2)}</b><small>trust</small></div>
      <div class="stat"><b>${u.weight}</b><small>voting weight</small></div>
      <div class="stat"><b>${rate}</b><small>${u.challenges_passed}/${u.challenges_sent} challenges passed</small></div>
      <div class="stat"><b>${bytes(u.credit_bytes)}</b><small>hosts for the others</small></div>
      <div class="stat"><b>${bytes(u.used_bytes)}</b><small>stores on the others</small></div>
      <div class="stat"><b>${u.devices}</b><small>${u.devices === 1 ? "device" : "devices"} · ${plural(u.invited, "invite", "invites")}</small></div>
    </div>
    <div class="sub">Member of ${esc(network)} since ${new Date(u.joined_at * 1000).toLocaleDateString()} · ${plural(u.messages, "message", "messages")} seen here</div>
    <div class="badges">${earned.length ? earned.map(badgeCard).join("") : `<div class="empty">No badges yet.</div>`}</div>`;
}

function openMember(fingerprint) {
  const network = currentNetwork();
  if (!network || !state.data) return;
  state.member = { network, fingerprint };
  $("member-box").dataset.shown = "";
  $("member-sheet").hidden = false;
  renderMember(state.data);
}

function closeMember() {
  state.member = null;
  $("member-sheet").hidden = true;
}

function celebrate(data) {
  let known;
  try {
    known = JSON.parse(stored(EARNED_KEY, "null"));
  } catch (_) {
    known = null;
  }
  const now = {};
  for (const [network, m] of Object.entries(data.members)) {
    const me = m.members.find((u) => u.fingerprint === data.status.fingerprint);
    if (!me) continue;
    now[network] = me.achievements.filter((a) => a.earned).map((a) => a.id);
    if (!known || !known[network]) continue;
    for (const a of me.achievements) {
      if (a.earned && !known[network].includes(a.id)) toast(`Badge earned in ${network}: ${a.name} (+${a.points})`, "badge");
    }
  }
  remember(EARNED_KEY, JSON.stringify(now));
}

async function sendChat() {
  const network = currentNetwork();
  const box = $("chat-text");
  const text = box.value.trim();
  if (!network || !text) return;
  box.value = "";
  box.style.height = "";
  try {
    const chat = await send(`/networks/${encodeURIComponent(network)}/chat`, "POST", { text });
    state.chat = { network, revision: chat.revision, messages: chat.messages || state.chat.messages };
    render();
  } catch (e) {
    box.value = text;
    toast(`Cannot send: ${e.message}`, "error");
  }
}

/* ---------- shell ---------- */

function renderNetworkPicker(data) {
  const select = $("network");
  const options = ["", ...data.status.networks];
  if (state.network && !data.status.networks.includes(state.network)) state.network = "";
  const wanted = options.map((n) => `<option value="${esc(n)}">${n ? esc(n) : "All networks"}</option>`).join("");
  if (select.dataset.options !== wanted) {
    select.innerHTML = wanted;
    select.dataset.options = wanted;
  }
  select.value = state.network;
}

function showView(view) {
  state.view = view;
  remember(VIEW_KEY, view);
  for (const id of Object.keys(VIEWS)) $("view-" + id).hidden = id !== view;
  for (const b of document.querySelectorAll(".nav")) b.classList.toggle("active", b.dataset.view === view);
  $("view-title").textContent = VIEWS[view];
  render();
  if (view === "gossip") scrollChat(true);
}

function render() {
  const data = state.data;
  if (!data) return;
  renderNetworkPicker(data);
  renderMe(data);
  const renderers = { fish: renderFish, flock: renderFlock, nest: renderNest, roost: renderRoost, gossip: renderGossip, profile: renderProfile };
  if (renderers[state.view]) renderers[state.view](data);
  renderUnread();
  if (state.member) renderMember(data);
}

async function refresh() {
  try {
    state.data = await load();
    sample(state.data);
    keepChat(state.data);
    celebrate(state.data);
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

function schedule() {
  clearInterval(state.timer);
  state.timer = setInterval(refresh, state.rate);
}

/* ---------- files ---------- */

function upload(file) {
  const network = currentNetwork();
  if (!network) {
    toast("Pick a network first, at the top of the page.", "error");
    return Promise.resolve();
  }
  const row = document.createElement("div");
  row.className = "upload";
  row.innerHTML = `<span>${esc(file.name)}</span><span class="status">${bytes(file.size)}</span><div class="bar"><span class="used" style="flex:0 0 0%"></span></div>`;
  $("uploads").append(row);
  const fill = row.querySelector(".bar span");
  const label = row.querySelector(".status");
  return new Promise((resolve) => {
    const xhr = new XMLHttpRequest();
    const query = new URLSearchParams({ network, name: file.name, tier: state.folder || "T1" });
    xhr.open("POST", "/files?" + query);
    xhr.setRequestHeader("Authorization", "Session " + session());
    xhr.upload.onprogress = (e) => {
      if (!e.lengthComputable) return;
      fill.style.flexBasis = (e.loaded / e.total) * 100 + "%";
      label.textContent = e.loaded < e.total ? `${pct(e.loaded, e.total)}%` : "Compressing, encrypting and spreading…";
    };
    xhr.onload = () => {
      let body = {};
      try {
        body = JSON.parse(xhr.responseText);
      } catch (_) {}
      if (xhr.status === 201) {
        row.remove();
        if (body.local) {
          toast(`${file.name} is kept on your Pouches only: folder ${body.tier} needs 3 users with a Pouch before it spreads files`, "error");
        } else {
          toast(`${file.name} stored in folder ${body.tier}, ${body.chunks} chunk(s) across ${body.n} Pouches`, "ok");
        }
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

const PLAYABLE = ["mp4", "m4v", "webm", "mkv", "mov", "ogv", "mp3", "m4a", "ogg", "oga", "opus", "wav", "flac"];

function playable(name) {
  return PLAYABLE.includes(String(name).split(".").pop().toLowerCase());
}

function play(id, network, name) {
  const query = new URLSearchParams({ network, session: session() });
  const media = $("player-media");
  $("player-title").textContent = name;
  media.src = `/files/${encodeURIComponent(id)}?${query}`;
  $("player").hidden = false;
  media.play().catch(() => {});
}

function closePlayer() {
  const media = $("player-media");
  media.pause();
  media.removeAttribute("src");
  media.load();
  $("player").hidden = true;
}

async function download(id, network, name) {
  if (native) {
    try {
      toast(`Fetching ${name}…`);
      if (await native.invoke("save_file", { fileId: id, network, name })) toast(`${name} saved`, "ok");
    } catch (e) {
      toast(`Cannot download ${name}: ${e}`, "error");
    }
    return;
  }
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

/* ---------- events ---------- */

$("sidebar").addEventListener("click", (e) => {
  const b = e.target.closest(".nav");
  if (b) showView(b.dataset.view);
});

$("network").addEventListener("change", (e) => {
  state.network = e.target.value;
  remember(NETWORK_KEY, state.network);
  state.folder = "";
  state.chat = { network: null, revision: null, messages: [] };
  render();
  refresh();
});

$("folders").addEventListener("click", (e) => {
  const b = e.target.closest("[data-folder]");
  if (!b) return;
  state.folder = b.dataset.folder;
  render();
});

$("files").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  if (b.dataset.play) play(b.dataset.play, b.dataset.network, b.dataset.name);
  if (b.dataset.download) download(b.dataset.download, b.dataset.network, b.dataset.name);
  if (b.dataset.delete) remove(b.dataset.delete, b.dataset.network, b.dataset.name);
});

$("node-tabs").addEventListener("click", (e) => {
  const kind = e.target.dataset.kind;
  if (!kind) return;
  state.kind = kind;
  for (const b of $("node-tabs").children) b.classList.toggle("active", b.dataset.kind === kind);
  render();
});

$("hatch").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-hatch]");
  if (!b) return;
  b.disabled = true;
  hatch(b.dataset.hatch).finally(() => (b.disabled = false));
});

$("roost-services").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (b) serviceAction(b);
});

$("leave-row").addEventListener("click", (e) => {
  const b = e.target.closest("button[data-leave]");
  if (b) leave(b.dataset.leave);
});

$("invite-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const network = currentNetwork();
  if (!network) {
    toast("Pick a network first, at the top of the page.", "error");
    return;
  }
  try {
    const data = await send(`/networks/${encodeURIComponent(network)}/invites`, "POST", { member_key: $("invite-key").value.trim() });
    $("invite-token").textContent = data.token;
    $("invite-token").hidden = false;
    toast("Token created: send it to them, they join with bp join <token>", "ok");
  } catch (err) {
    toast(`Cannot invite: ${err.message}`, "error");
  }
});

$("chat-form").addEventListener("submit", (e) => {
  e.preventDefault();
  sendChat();
});

$("chat-text").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
    e.preventDefault();
    sendChat();
  }
});

$("chat-text").addEventListener("input", (e) => {
  e.target.style.height = "";
  e.target.style.height = Math.min(120, e.target.scrollHeight + 2) + "px";
});

$("view-gossip").addEventListener("click", (e) => {
  const who = e.target.closest("[data-member]");
  if (who) openMember(who.dataset.member);
});

$("member-sheet").addEventListener("click", (e) => {
  if (e.target === $("member-sheet") || e.target.closest("[data-close]")) closeMember();
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && state.member) closeMember();
});

document.addEventListener("visibilitychange", renderUnread);

$("themes").addEventListener("click", (e) => {
  const b = e.target.closest("[data-pick]");
  if (!b) return;
  document.documentElement.dataset.theme = b.dataset.pick;
  remember(THEME_KEY, b.dataset.pick);
  renderThemes();
});

$("refresh-rate").addEventListener("change", (e) => {
  state.rate = Number(e.target.value);
  remember(RATE_KEY, String(state.rate));
  schedule();
});

$("sign-out").addEventListener("click", () => {
  try {
    localStorage.removeItem(SESSION_KEY);
  } catch (_) {}
  banner("Signed out. Run <code>bp dashboard</code> on this device to open a new session.");
  clearInterval(state.timer);
});

$("player-close").addEventListener("click", closePlayer);
$("player").addEventListener("click", (e) => {
  if (e.target === $("player")) closePlayer();
});

$("picker").addEventListener("click", async (e) => {
  if (!native) return;
  e.preventDefault();
  const network = currentNetwork();
  if (!network) {
    toast("Pick a network first, at the top of the page.", "error");
    return;
  }
  try {
    const stored = await native.invoke("upload_files", { network, tier: state.folder || "T1" });
    if (stored.length) toast(`Stored ${stored.join(", ")}`, "ok");
  } catch (err) {
    toast(`Upload failed: ${err}`, "error");
  }
  refresh();
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

/* ---------- start ---------- */

function showOnly(id) {
  for (const el of ["onboarding", "unlock", "main"]) $(el).hidden = el !== id;
}

async function start() {
  showView(state.view);
  if (!native) {
    if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});
    refresh();
    schedule();
    return;
  }
  document.body.classList.add("native");
  $("dropzone").hidden = true;
  const app = await native.invoke("app_state");
  if (!app.identity || (app.running && !app.member)) {
    showOnly("onboarding");
  } else if (!app.running) {
    showOnly("unlock");
  } else {
    showOnly("main");
    refresh();
    schedule();
  }
}

$("pick-key").addEventListener("click", async () => {
  try {
    const key = await native.invoke("pick_key");
    if (key) $("key").value = key;
  } catch (e) {
    toast(`Cannot read the key: ${e}`, "error");
  }
});

$("onboarding").addEventListener("submit", async (e) => {
  e.preventDefault();
  const button = e.target.querySelector("button[type=submit]");
  button.disabled = true;
  button.textContent = "Joining…";
  try {
    await native.invoke("setup", {
      alias: $("alias").value,
      key: $("key").value,
      passphrase: $("passphrase").value,
      token: $("token").value,
    });
    location.reload();
  } catch (err) {
    toast(String(err), "error");
    button.disabled = false;
    button.textContent = "Join";
  }
});

$("unlock").addEventListener("submit", async (e) => {
  e.preventDefault();
  try {
    await native.invoke("unlock", { passphrase: $("unlock-passphrase").value });
    location.reload();
  } catch (err) {
    toast(String(err), "error");
  }
});

start();
