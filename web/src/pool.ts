// Worker pool (D17, D30): pairs go to whichever Worker is free, and a pair goes back
// to the Worker that ran it, whose cache holds its sets.
import type { Method, Methods, Reply, Request } from "./protocol";
import type { PairReport, PlanSpec, SetKind, Sets, TopologyView } from "./pkg/netavail_wasm";

/// The part of a Worker the pool uses; tests pass fakes.
export interface Port {
  postMessage(req: Request): void;
  onmessage: ((event: MessageEvent<Reply>) => void) | null;
  terminate(): void;
}

export class Cancelled extends Error {}

export type Pair = [number, number];
export type RunArgs = { plan: PlanSpec; probs: number[]; linkProb?: number };

/// One Worker and its outstanding calls.
class Slot {
  private port: Port;
  private nextId = 0;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();

  constructor(private spawn: () => Port) {
    this.port = this.start();
  }

  get busy() {
    return this.pending.size > 0;
  }

  call<M extends Method>(method: M, args: Methods[M]["args"]): Promise<Methods[M]["result"]> {
    const id = this.nextId++;
    const req: Request<M> = { id, method, args };
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.port.postMessage(req);
    });
  }

  /// Terminates the Worker, failing its calls with Cancelled, and starts a fresh one.
  restart() {
    this.port.terminate();
    for (const p of this.pending.values()) p.reject(new Cancelled("cancelled"));
    this.pending.clear();
    this.port = this.start();
  }

  private start(): Port {
    const port = this.spawn();
    port.onmessage = (event) => {
      const reply = event.data;
      const p = this.pending.get(reply.id);
      this.pending.delete(reply.id);
      if (reply.ok) p?.resolve(reply.value);
      else p?.reject(new Error(reply.error));
    };
    return port;
  }
}

// Link-graph pairs live in another cache than plain ones
const key = (src: number, dst: number, links: boolean) => `${links ? "l" : "n"}${src}-${dst}`;

export class Pool {
  private slots: Slot[];
  /// Pair key -> index of the Worker that last ran it
  private owner = new Map<string, number>();
  private json: string | null = null;
  private graph: string | null = null;
  private cancelled = false;

  constructor(spawn: () => Port, size: number) {
    this.slots = Array.from({ length: Math.max(1, size) }, () => new Slot(spawn));
  }

  get size() {
    return this.slots.length;
  }

  algorithms() {
    return this.slots[0].call("algorithms", {});
  }

  /// Loads the topology in every Worker; their caches survive if the graph is unchanged.
  async open(json: string): Promise<TopologyView> {
    const views = await Promise.all(this.slots.map((s) => s.call("open", { json })));
    this.json = json;
    if (views[0].graph !== this.graph) this.owner.clear();
    this.graph = views[0].graph;
    return views[0];
  }

  /// Runs `pairs`, calling `onReport` as each one finishes (not in pair order).
  /// Throws Cancelled after `cancel()`, once every Worker has stopped.
  async runPairs(args: RunArgs, pairs: Pair[], onReport: (r: PairReport) => void): Promise<void> {
    const links = args.linkProb !== undefined;
    const own: Pair[][] = this.slots.map(() => []);
    const shared: Pair[] = [];
    for (const p of pairs) {
      const o = this.owner.get(key(p[0], p[1], links));
      (o === undefined ? shared : own[o]).push(p);
    }
    // Own pairs first, then new ones, then the back of the longest other queue
    const next = (i: number): Pair | undefined =>
      own[i].shift() ?? shared.shift() ?? own.reduce((a, q) => (q.length > a.length ? q : a)).pop();
    this.cancelled = false;
    let failed = false;
    const loop = async (i: number) => {
      for (let p = next(i); p && !this.cancelled && !failed; p = next(i)) {
        try {
          const r = await this.slots[i].call("runPair", { ...args, src: p[0], dst: p[1] });
          this.owner.set(key(p[0], p[1], links), i);
          onReport(r);
        } catch (e) {
          failed = true;
          throw e;
        }
      }
    };
    const outcomes = await Promise.allSettled(this.slots.map((_, i) => loop(i)));
    const errors = outcomes.flatMap((o) => (o.status === "rejected" ? [o.reason] : []));
    const real = errors.find((e) => !(e instanceof Cancelled));
    if (real) throw real;
    if (this.cancelled) throw new Cancelled("cancelled");
  }

  /// Stops handing out pairs and restarts only the Workers in the middle of a call;
  /// idle Workers keep their caches.
  cancel() {
    this.cancelled = true;
    this.slots.forEach((slot, i) => {
      if (!slot.busy) return;
      slot.restart();
      for (const [k, o] of this.owner) if (o === i) this.owner.delete(k);
      if (this.json !== null) slot.call("open", { json: this.json }).catch(() => {});
    });
  }

  /// Every Worker re-evaluates the pairs it owns; reports come back in pair order.
  async reevaluate(args: RunArgs): Promise<PairReport[]> {
    const links = args.linkProb !== undefined;
    const outs = await Promise.all(this.slots.map((s) => s.call("reevaluate", args)));
    // A stolen pair can still sit in its old Worker's cache; keep the owner's report
    const reports = outs.flatMap((out, i) =>
      out.reports.filter((r) => this.owner.get(key(r.src, r.dst, links)) === i),
    );
    return reports.sort((a, b) => a.src - b.src || a.dst - b.dst);
  }

  sets(plan: PlanSpec, src: number, dst: number, kind: SetKind, links: boolean): Promise<Sets> {
    const slot = this.slots[this.owner.get(key(src, dst, links)) ?? 0];
    return slot.call("sets", { plan, src, dst, kind, links });
  }
}
