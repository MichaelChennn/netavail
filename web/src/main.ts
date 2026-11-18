// Demo page (D32): pick a topology and algorithms, run every pair in one Worker, show a table.
import type { Method, Methods, Reply, Request } from "./protocol";
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

// ---- Worker RPC; cancelling terminates the Worker and starts a fresh one ----

let worker: Worker;
let nextId = 0;
const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

function startWorker() {
  worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  worker.onmessage = (event: MessageEvent<Reply>) => {
    const reply = event.data;
    const p = pending.get(reply.id);
    pending.delete(reply.id);
    if (reply.ok) p?.resolve(reply.value);
    else p?.reject(new Error(reply.error));
  };
}

function call<M extends Method>(method: M, args: Methods[M]["args"]): Promise<Methods[M]["result"]> {
  const id = nextId++;
  const req: Request<M> = { id, method, args };
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
    worker.postMessage(req);
  });
}

class Cancelled extends Error {}

function restartWorker() {
  worker.terminate();
  for (const p of pending.values()) p.reject(new Cancelled("cancelled"));
  pending.clear();
  startWorker();
}

// ---- State ----

let topoJson = "";
let topo: TopologyView | null = null;
let reports: PairReport[] = [];
let shownPlan: PlanSpec | null = null;
let shownProbs: number[] = [];
let running = false;

const label = (i: number) => {
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
  topoJson = json;
  topo = await call("open", { json });
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
  try {
    probs = parseProbs();
  } catch (e) {
    return setStatus((e as Error).message, true);
  }
  const spec = plan();
  const n = topo.nodes.length;
  const pairs: [number, number][] = [];
  for (let s = 0; s < n; s++) for (let d = s + 1; d < n; d++) pairs.push([s, d]);

  reports = [];
  shownPlan = spec;
  shownProbs = probs;
  renderTable();
  hideDetail();
  setRunning(true);
  ui.progress.max = pairs.length;
  ui.progress.value = 0;
  const started = performance.now();
  const body = ui.table.tBodies[0];
  try {
    for (const [src, dst] of pairs) {
      const report = await call("runPair", { plan: spec, src, dst, probs });
      reports.push(report);
      body.append(row(report));
      ui.progress.value = reports.length;
      setStatus(`${reports.length} / ${pairs.length} pairs`);
    }
    setStatus(summary(performance.now() - started));
  } catch (e) {
    if (e instanceof Cancelled) {
      setStatus(`cancelled after ${reports.length} of ${pairs.length} pairs`);
      await call("open", { json: topoJson });
    } else {
      setStatus((e as Error).message, true);
    }
  } finally {
    setRunning(false);
  }
}

async function reevaluate() {
  if (!topo) return;
  try {
    const probs = parseProbs();
    const spec = plan();
    const started = performance.now();
    const out = await call("reevaluate", { plan: spec, probs });
    if (out.reports.length === 0) return setStatus("nothing cached for these algorithms yet; run first");
    reports = out.reports;
    shownPlan = spec;
    shownProbs = probs;
    renderTable();
    setStatus(`re-evaluated ${reports.length} cached pairs in ${(performance.now() - started).toFixed(1)} ms`);
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
  return `${reports.length} pairs in ${wallMs.toFixed(0)} ms (${failed} failed). Stage totals: ${stages}.`;
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
  if (avail) head.push(...shownProbs.map((p) => `A(p=${p})`));
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
    const { sets } = await call("sets", { plan: shownPlan, src, dst, kind });
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
  startWorker();
  const { algorithms } = await call("algorithms", {});
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
  ui.cancel.onclick = () => restartWorker();
  renderTable();
}

main().catch((e) => setStatus(String(e?.message ?? e), true));
