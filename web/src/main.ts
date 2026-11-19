// Demo page (D32): pick a topology and algorithms, run every pair on a Worker pool, show a table.
import { Cancelled, Pool, type Pair } from "./pool";
import type { AlgorithmInfo, PairReport, PlanSpec, TopologyView } from "./pkg/netavail_wasm";

type Entry = { file: string; name: string; nodes: number; edges: number; reference: boolean };

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const ui = {
  topology: $<HTMLSelectElement>("topology"),
  file: $<HTMLInputElement>("file"),
  topoInfo: $("topo-info"),
  paths: $<HTMLSelectElement>("paths"),
  cuts: $<HTMLSelectElement>("cuts"),
  avail: $<HTMLSelectElement>("avail"),
  probs: $<HTMLInputElement>("probs"),
  linkProb: $<HTMLInputElement>("link-prob"),
  run: $<HTMLButtonElement>("run"),
  reevaluate: $<HTMLButtonElement>("reevaluate"),
  cancel: $<HTMLButtonElement>("cancel"),
  progress: $<HTMLProgressElement>("progress"),
  status: $("status"),
  table: $<HTMLTableElement>("table"),
  results: document.querySelector<HTMLElement>(".results")!,
  detail: $("detail"),
  detailTitle: $("detail-title"),
  detailBody: $("detail-body"),
};

// ---- Workers: ?workers=N, else one per core but one, at most 8 ----

const asked = Number(new URLSearchParams(location.search).get("workers"));
const workers = asked >= 1 ? Math.floor(asked) : Math.min(Math.max(navigator.hardwareConcurrency - 1, 1), 8);
const pool = new Pool(() => new Worker(new URL("./worker.ts", import.meta.url), { type: "module" }), workers);

// ---- State ----

let topo: TopologyView | null = null;
let reports: PairReport[] = [];
let shownPlan: PlanSpec | null = null;
let shownProbs: number[] = [];
let shownLinkProb: number | undefined;
let running = false;

// Nodes n + i of the link graph are edge i, shown as u–v
const label = (i: number): string => {
  const n = topo?.nodes.length ?? 0;
  const edge = i >= n ? topo?.edges[i - n] : undefined;
  if (edge) return `${label(edge[0])}–${label(edge[1])}`;
  const l = topo?.nodes[i]?.label;
  return l && l !== String(i) ? `${l} (${i})` : String(i);
};

function setStatus(text: string, error = false) {
  ui.status.textContent = text;
  ui.status.className = error ? "error" : "muted";
}

function plan(): PlanSpec {
  const spec: PlanSpec = { paths: ui.paths.value };
  if (ui.cuts.value) spec.cuts = ui.cuts.value;
  if (ui.avail.value) spec.availability = ui.avail.value;
  return spec;
}

function parseProbs(): number[] {
  const parts = ui.probs.value.split(/[\s,]+/).filter(Boolean);
  const probs = parts.map(Number);
  const bad = parts.find((_, i) => !(probs[i] >= 0 && probs[i] <= 1));
  if (bad !== undefined) throw new Error(`p = ${bad} is not a number in [0, 1]`);
  return probs;
}

function parseLinkProb(): number | undefined {
  const text = ui.linkProb.value.trim();
  if (text === "") return undefined;
  const q = Number(text);
  if (!(q >= 0 && q <= 1)) throw new Error(`link availability ${text} is not a number in [0, 1]`);
  return q;
}

// Every link becomes a node, so cut sets grow quickly
const LINK_HINT = "with link failures, cut algorithms can be very slow on medium topologies; path_sdp with Cuts set to none is much faster";

function setRunning(on: boolean) {
  running = on;
  ui.run.disabled = on;
  ui.reevaluate.disabled = on;
  ui.cancel.disabled = !on;
  ui.topology.disabled = on;
  ui.file.disabled = on;
}

// ---- Topology ----

async function openTopology(json: string) {
  topo = await pool.open(json);
  ui.topoInfo.textContent = `${topo.nodes.length} nodes, ${topo.edges.length} edges, ${
    (topo.nodes.length * (topo.nodes.length - 1)) / 2
  } pairs`;
  reports = [];
  renderTable();
  hideDetail();
  setStatus("");
}

async function openBuiltIn(file: string) {
  const res = await fetch(`${import.meta.env.BASE_URL}topologies/${encodeURIComponent(file)}`);
  if (!res.ok) throw new Error(`could not fetch ${file}: ${res.status}`);
  await openTopology(await res.text());
}

// ---- Running ----

async function runAll() {
  if (!topo) return;
  let probs: number[];
  let linkProb: number | undefined;
  try {
    probs = parseProbs();
    linkProb = parseLinkProb();
  } catch (e) {
    return setStatus((e as Error).message, true);
  }
  const spec = plan();
  const n = topo.nodes.length;
  const pairs: Pair[] = [];
  for (let s = 0; s < n; s++) for (let d = s + 1; d < n; d++) pairs.push([s, d]);

  reports = [];
  shownPlan = spec;
  shownProbs = probs;
  shownLinkProb = linkProb;
  renderTable();
  hideDetail();
  setRunning(true);
  const hint = linkProb !== undefined && spec.cuts ? ` (${LINK_HINT})` : "";
  ui.progress.max = pairs.length;
  ui.progress.value = 0;
  const started = performance.now();
  const body = ui.table.tBodies[0];
  // Rows come in as Workers finish them, and are sorted at the end
  const onReport = (report: PairReport) => {
    reports.push(report);
    body.append(row(report));
    ui.progress.value = reports.length;
    setStatus(`${reports.length} / ${pairs.length} pairs on ${pool.size} Workers${hint}`);
  };
  try {
    await pool.runPairs({ plan: spec, probs, linkProb }, pairs, onReport);
    setStatus(summary(performance.now() - started) + hint);
  } catch (e) {
    if (e instanceof Cancelled) setStatus(`cancelled after ${reports.length} of ${pairs.length} pairs`);
    else setStatus((e as Error).message, true);
  } finally {
    reports.sort((a, b) => a.src - b.src || a.dst - b.dst);
    renderTable();
    setRunning(false);
  }
}

async function reevaluate() {
  if (!topo) return;
  try {
    const probs = parseProbs();
    const linkProb = parseLinkProb();
    const spec = plan();
    const started = performance.now();
    const out = await pool.reevaluate({ plan: spec, probs, linkProb });
    if (out.length === 0) return setStatus("nothing cached for these algorithms yet; run first");
    reports = out;
    shownPlan = spec;
    shownProbs = probs;
    shownLinkProb = linkProb;
    renderTable();
    // Pairs evicted from the caches, or never run, are left out
    const n = topo.nodes.length;
    const rest = reports.length < (n * (n - 1)) / 2 ? "; the other pairs are not cached, run again for them" : "";
    setStatus(`re-evaluated ${reports.length} cached pairs in ${(performance.now() - started).toFixed(1)} ms${rest}`);
  } catch (e) {
    setStatus((e as Error).message, true);
  }
}

function summary(wallMs: number) {
  const failed = reports.filter((r) => r.error).length;
  const sum = (key: "paths_ms" | "cuts_ms" | "prepare_ms" | "evaluate_ms") =>
    reports.reduce((acc, r) => acc + (r.times[key] ?? 0), 0);
  const stages = (["paths_ms", "cuts_ms", "prepare_ms", "evaluate_ms"] as const)
    .filter((k) => reports.some((r) => r.times[k] !== undefined))
    .map((k) => `${k.replace("_ms", "")} ${sum(k).toFixed(1)} ms`)
    .join(", ");
  const links = shownLinkProb === undefined ? "" : ` Links at ${shownLinkProb}.`;
  // Cached stages report no time, so stage totals cover only what was computed
  const hits = reports.filter((r) => !r.error && r.times.paths_ms === undefined).length;
  const cached = hits ? ` ${hits} pairs reused cached sets.` : "";
  return `${reports.length} pairs on ${pool.size} Workers in ${wallMs.toFixed(0)} ms (${failed} failed). Stage totals: ${stages}.${cached}${links}`;
}

// ---- Table ----

function cell(text: string, className?: string) {
  const td = document.createElement("td");
  td.textContent = text;
  if (className) td.className = className;
  return td;
}

const ms = (v: number | undefined) => (v === undefined ? "" : v.toFixed(3));

// Columns of stages the plan does not run are left out
function row(r: PairReport) {
  const tr = document.createElement("tr");
  const cuts = Boolean(shownPlan?.cuts);
  const avail = Boolean(shownPlan?.availability);
  tr.append(cell(label(r.src)), cell(label(r.dst)), cell(r.paths?.toString() ?? ""));
  if (cuts) tr.append(cell(r.cuts?.toString() ?? ""));
  if (avail) tr.append(...shownProbs.map((_, i) => cell(r.availability[i]?.toFixed(12) ?? "")));
  tr.append(cell(ms(r.times.paths_ms)));
  if (cuts) tr.append(cell(ms(r.times.cuts_ms)));
  if (avail) tr.append(cell(ms(r.times.prepare_ms)), cell(ms(r.times.evaluate_ms)));
  tr.append(cell(r.error ?? "", "error"));
  tr.onclick = () => {
    ui.table.querySelector("tr.selected")?.classList.remove("selected");
    tr.classList.add("selected");
    showSets(r.src, r.dst);
  };
  return tr;
}

function renderTable() {
  const cuts = Boolean(shownPlan?.cuts);
  const avail = Boolean(shownPlan?.availability);
  const head = ["source", "target", "paths"];
  if (cuts) head.push("cuts");
  const q = shownLinkProb === undefined ? "" : `, q=${shownLinkProb}`;
  if (avail) head.push(...shownProbs.map((p) => `A(p=${p}${q})`));
  head.push("paths ms");
  if (cuts) head.push("cuts ms");
  if (avail) head.push("prepare ms", "evaluate ms");
  head.push("error");
  const tr = document.createElement("tr");
  for (const h of head) {
    const th = document.createElement("th");
    th.textContent = h;
    tr.append(th);
  }
  ui.table.tHead!.replaceChildren(tr);
  ui.table.tBodies[0].replaceChildren(...reports.map(row));
}

// ---- Set detail ----

function hideDetail() {
  ui.detail.hidden = true;
  ui.results.classList.remove("with-detail");
}

async function showSets(src: number, dst: number) {
  if (!shownPlan || running) return;
  ui.detail.hidden = false;
  ui.results.classList.add("with-detail");
  ui.detailTitle.textContent = `${label(src)} → ${label(dst)}`;
  const parts: HTMLElement[] = [];
  const kinds = shownPlan.cuts ? (["paths", "cuts"] as const) : (["paths"] as const);
  for (const kind of kinds) {
    const links = shownLinkProb !== undefined;
    const { sets } = await pool.sets(shownPlan, src, dst, kind, links);
    const h = document.createElement("h2");
    h.textContent = kind === "paths" ? `Minimal path sets (${sets?.length ?? 0})` : `Minimal cut sets (${sets?.length ?? 0})`;
    const list = document.createElement("ol");
    for (const set of (sets ?? []).slice(0, 500)) {
      const li = document.createElement("li");
      li.textContent = set.map(label).join(kind === "paths" ? " → " : ", ");
      list.append(li);
    }
    parts.push(h, list);
    if ((sets?.length ?? 0) > 500) parts.push(Object.assign(document.createElement("p"), { textContent: "first 500 shown" }));
  }
  ui.detailBody.replaceChildren(...parts);
}

// ---- Setup ----

function fillSelect(select: HTMLSelectElement, algos: AlgorithmInfo[], none?: string) {
  const options = algos.map((a) => new Option(`${a.name} (${a.id})`, a.id));
  if (none !== undefined) options.unshift(new Option(none, ""));
  select.replaceChildren(...options);
  // Default to the first algorithm, so Run computes the whole pipeline
  if (algos.length > 0) select.value = algos[0].id;
  select.title = algos.map((a) => `${a.id}: ${a.summary}`).join("\n");
}

async function main() {
  const { algorithms } = await pool.algorithms();
  fillSelect(ui.paths, algorithms.filter((a) => a.kind === "paths"));
  fillSelect(ui.cuts, algorithms.filter((a) => a.kind === "cuts"), "none");
  fillSelect(ui.avail, algorithms.filter((a) => a.kind === "availability"), "none");

  const res = await fetch(`${import.meta.env.BASE_URL}topologies/index.json`);
  const entries: Entry[] = await res.json();
  ui.topology.replaceChildren(
    ...entries.map((e) => new Option(`${e.name} (${e.nodes} nodes${e.reference ? "" : ", large"})`, e.file)),
  );
  ui.topology.value = "Germany_17.json";
  await openBuiltIn(ui.topology.value);

  ui.topology.onchange = () => openBuiltIn(ui.topology.value).catch((e) => setStatus(e.message, true));
  ui.file.onchange = async () => {
    const f = ui.file.files?.[0];
    if (!f) return;
    try {
      await openTopology(await f.text());
    } catch (e) {
      setStatus((e as Error).message, true);
    }
  };
  ui.run.onclick = () => runAll();
  ui.reevaluate.onclick = () => reevaluate();
  ui.cancel.onclick = () => {
    pool.cancel();
    setStatus("stopping: busy Workers finish their pair, or stop within 1 s");
  };
  renderTable();
}

main().catch((e) => setStatus(String(e?.message ?? e), true));
