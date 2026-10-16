// The UI (docs/architecture.md, step 7): a daemon watched through its
// feed, GET /events, and its plain HTTP answers: the schema, the time,
// the sessions, the journal's tail, and the pace, which it may set.

const $ = (id) => document.getElementById(id);

/// An element, its class and its text.
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

async function json(path) {
  const r = await fetch(path);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.json();
}

async function text(path) {
  const r = await fetch(path);
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.text();
}

/// Calls `f` once things settle: many changes, one fetch.
function later(f, ms) {
  let timer = null;
  return () => {
    clearTimeout(timer);
    timer = setTimeout(f, ms);
  };
}

// The schema

async function schema() {
  const s = await json("/schema");
  const names = new Map(s.classes.map((c) => [c.id, c.name]));
  const box = $("schema");
  box.replaceChildren();
  for (const c of s.classes) {
    const card = el("div", "class");
    card.append(el("div", "name", c.name));
    const meta = [];
    if (c.kind !== "class") meta.push(c.kind);
    if (c.state !== "active") meta.push(c.state);
    meta.push(`${c.objects} object${c.objects === 1 ? "" : "s"}`);
    card.append(el("div", "meta", meta.join(", ")));
    if (c.hyper.length) {
      card.append(el("div", "hyper", "in " + c.hyper.map((h) => names.get(h)).join(", ")));
    }
    if (c.attributes.length) {
      const ul = el("ul");
      for (const a of c.attributes) ul.append(el("li", null, a));
      card.append(ul);
    }
    box.append(card);
  }
  const links = $("links");
  links.replaceChildren();
  for (const l of s.links) {
    const back = l.reverse === null ? "" : ` (back: ${l.reverse})`;
    links.append(el("li", null, `${names.get(l.from)} "${l.relation}" ${names.get(l.to)}${back}`));
  }
}

// The time: the sequencing set as a timeline, the processes beside it

async function time() {
  const t = await json("/time");
  $("now").textContent = `now ${t.now}`;
  const svg = $("timeline");
  const ns = "http://www.w3.org/2000/svg";
  const node = (tag, attrs, content) => {
    const e = document.createElementNS(ns, tag);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
    if (content !== undefined) e.textContent = content;
    return e;
  };
  svg.replaceChildren();
  const last = Math.max(t.now + 1, ...t.due.map((d) => d.at));
  const x = (at) => 20 + ((at - t.now) / (last - t.now)) * 560;
  svg.append(node("line", { class: "axis", x1: 20, y1: 60, x2: 580, y2: 60 }));
  svg.append(node("line", { class: "now", x1: 20, y1: 20, x2: 20, y2: 70 }));
  svg.append(node("text", { x: 22, y: 82 }, String(t.now)));
  svg.append(node("text", { x: 548, y: 82 }, String(last)));
  t.due.forEach((d, i) => {
    const cx = x(d.at);
    svg.append(node("circle", { class: "mark", cx, cy: 60, r: 4 }));
    // Labels alternate in height so that close ones stay readable.
    svg.append(node("text", { x: cx + 4, y: i % 2 ? 50 : 36 }, `${d.agent} @ ${d.at}`));
  });
  const rows = $("processes");
  rows.replaceChildren();
  for (const p of t.processes) {
    const tr = el("tr");
    tr.append(el("td", null, p.name), el("td", null, p.state), el("td", null, String(p.clock)));
    rows.append(tr);
  }
}

// The sessions

const sessions = new Map();

function showSessions() {
  const ul = $("sessions");
  ul.replaceChildren();
  for (const [id, s] of [...sessions].reverse()) {
    const li = el("li", s.ended ? (s.error ? "failed" : "ended") : null);
    li.textContent = `session ${id}: ${s.ended ? s.error || "ended" : "running"}`;
    if (!s.ended) {
      const kill = el("button", null, "kill");
      kill.addEventListener("click", () => fetch(`/sessions/${id}/kill`, { method: "POST" }));
      li.append(kill);
    }
    ul.append(li);
  }
}

// The journal's tail

const TAIL = 200;

function journal(line, cls) {
  const ol = $("journal");
  ol.prepend(el("li", cls, line));
  while (ol.children.length > TAIL) ol.lastChild.remove();
}

// The pace

function showPace(p) {
  const kind = p.pace;
  $("pace-kind").value = kind;
  let now = kind;
  if (kind === "fast" && p.horizon !== undefined) now += ` to ${p.horizon}`;
  if (kind === "real") now += ` x${p.factor}`;
  if (kind === "step") now += ` (${p.steps} left)`;
  $("pace-now").textContent = `now: ${now}`;
  for (const [field, shown] of [
    ["horizon", kind === "fast"],
    ["factor", kind === "real"],
    ["steps", kind === "step"],
  ]) {
    $(`pace-${field}-label`).hidden = !shown;
  }
}

$("pace-kind").addEventListener("change", () => showPace({ pace: $("pace-kind").value }));

$("pace").addEventListener("submit", async (e) => {
  e.preventDefault();
  const body = { pace: $("pace-kind").value };
  const horizon = parseFloat($("pace-horizon").value);
  if (body.pace === "fast" && !Number.isNaN(horizon)) body.horizon = horizon;
  if (body.pace === "real") body.factor = parseFloat($("pace-factor").value) || 1;
  if (body.pace === "step") body.steps = parseInt($("pace-steps").value, 10) || 1;
  const r = await fetch("/pace", { method: "POST", body: JSON.stringify(body) });
  if (r.ok) showPace(await r.json());
});

// The feed

const refreshSchema = later(() => schema().catch(console.error), 200);
const refreshTime = later(() => time().catch(console.error), 100);

function frame(f) {
  if ("change" in f) {
    journal(f.change);
    refreshSchema();
    refreshTime();
  } else if ("module" in f) {
    journal(`module ${f.module}`, "session");
  } else if ("output" in f) {
    journal(`[process ${f.process ?? "?"}] ${f.output.trimEnd()}`, "output");
  } else if ("started" in f) {
    sessions.set(f.session, { ended: false });
    journal(`session ${f.session} started`, "session");
    showSessions();
  } else if ("ended" in f) {
    sessions.set(f.session, { ended: true, error: f.ended });
    journal(`session ${f.session} ${f.ended ? "failed: " + f.ended : "ended"}`, "session");
    showSessions();
    refreshTime();
  } else if ("time" in f) {
    refreshTime();
  } else if ("pace" in f) {
    showPace(f.pace);
  }
}

function connect(wait) {
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${scheme}://${location.host}/events`);
  ws.addEventListener("open", () => {
    $("link").textContent = "live";
    $("link").className = "link up";
    wait = 500;
    load();
  });
  ws.addEventListener("message", (m) => frame(JSON.parse(m.data)));
  ws.addEventListener("close", () => {
    $("link").textContent = "reconnecting";
    $("link").className = "link down";
    // Back off, up to ten seconds between tries.
    setTimeout(() => connect(Math.min(wait * 2, 10000)), wait);
  });
}

/// What the feed does not repeat: where things stand when it opens.
async function load() {
  await Promise.all([
    schema(),
    time(),
    json("/pace").then(showPace),
    text("/sessions").then((t) => {
      for (const id of t.split("\n").filter(Boolean)) {
        if (!sessions.has(Number(id))) sessions.set(Number(id), { ended: false });
      }
      showSessions();
    }),
  ]).catch(console.error);
}

connect(500);
