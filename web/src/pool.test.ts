import { describe, expect, it, vi } from "vitest";
import { Cancelled, Pool, type Pair, type Port } from "./pool";
import type { Reply, Request } from "./protocol";
import type { PairReport } from "./pkg/netavail_wasm";

/// Replies on the next timer tick; `hold` keeps runPair calls until `release()`.
class FakeWorker implements Port {
  onmessage: ((event: MessageEvent<Reply>) => void) | null = null;
  hold = false;
  terminated = false;
  log: Request[] = [];
  private held: Request[] = [];
  private cache = new Set<string>();

  constructor(private index: number) {}

  postMessage(req: Request) {
    this.log.push(req);
    if (this.hold && req.method === "runPair") this.held.push(req);
    else setTimeout(() => this.reply(req), 0);
  }

  release() {
    this.hold = false;
    for (const req of this.held.splice(0)) this.reply(req);
  }

  terminate() {
    this.terminated = true;
  }

  ran(): string[] {
    return this.log.filter((r) => r.method === "runPair").map((r) => pairOf(r.args as { src: number; dst: number }));
  }

  private reply(req: Request) {
    if (this.terminated) return;
    const a = req.args as { json: string; src: number; dst: number };
    let value: unknown;
    if (req.method === "open") value = { graph: a.json, name: "", nodes: [], edges: [] };
    if (req.method === "runPair") {
      if (a.src === 99) return this.send({ id: req.id, ok: false, error: "bad pair" });
      this.cache.add(`${a.src}-${a.dst}`);
      value = report(a.src, a.dst);
    }
    if (req.method === "reevaluate")
      value = { reports: [...this.cache].map((k) => report(...(k.split("-").map(Number) as Pair))) };
    if (req.method === "sets") value = { sets: [[this.index]] };
    this.send({ id: req.id, ok: true, value });
  }

  private send(reply: Reply) {
    this.onmessage?.({ data: reply } as MessageEvent<Reply>);
  }
}

const pairOf = (a: { src: number; dst: number }) => `${a.src}-${a.dst}`;
const report = (src: number, dst: number) => ({ src, dst, availability: [], times: {} }) as unknown as PairReport;
const args = { plan: { paths: "dfs" }, probs: [] };

function allPairs(n: number): Pair[] {
  const pairs: Pair[] = [];
  for (let s = 0; s < n; s++) for (let d = s + 1; d < n; d++) pairs.push([s, d]);
  return pairs;
}

async function setup(size: number) {
  const workers: FakeWorker[] = [];
  const pool = new Pool(() => {
    const w = new FakeWorker(workers.length);
    workers.push(w);
    return w;
  }, size);
  await pool.open("g");
  return { pool, workers };
}

async function run(pool: Pool, pairs: Pair[]) {
  const reports: PairReport[] = [];
  await pool.runPairs(args, pairs, (r) => reports.push(r));
  return reports.map(pairOf).sort();
}

const owner = async (pool: Pool, [src, dst]: Pair) =>
  (await pool.sets(args.plan, src, dst, "paths", false)).sets![0][0];

describe("Pool", () => {
  it("runs every pair once, spread over the Workers", async () => {
    const { pool, workers } = await setup(3);
    const pairs = allPairs(4);

    expect(await run(pool, pairs)).toEqual(pairs.map(([s, d]) => `${s}-${d}`).sort());
    expect(workers.flatMap((w) => w.ran()).length).toBe(pairs.length);
    expect(workers.every((w) => w.ran().length > 0)).toBe(true);
  });

  it("sends a pair back to the Worker that ran it", async () => {
    const { pool, workers } = await setup(2);
    const pairs = allPairs(4);
    await run(pool, pairs);
    const first = workers.map((w) => w.ran());

    await run(pool, pairs);

    expect(workers.map((w) => w.ran())).toEqual(first.map((ran) => [...ran, ...ran]));
    for (const [i, ran] of first.entries())
      for (const p of ran) expect(await owner(pool, p.split("-").map(Number) as Pair)).toBe(i);
  });

  it("lets an idle Worker take pairs queued for a busy one", async () => {
    const { pool, workers } = await setup(2);
    const pairs = allPairs(4);
    await run(pool, pairs);
    const queued = workers[1].ran();
    expect(queued.length).toBe(3);

    // Worker 1 holds its first pair; Worker 0 runs its own, then takes Worker 1's from the back
    workers[1].hold = true;
    const second = pool.runPairs(args, pairs, () => {});
    await vi.waitFor(() => expect(workers[0].ran().length).toBe(3 + 3 + 2));
    workers[1].release();
    await second;

    expect(workers[1].ran().slice(3)).toEqual([queued[0]]);
    expect(workers[0].ran().slice(-2)).toEqual([queued[2], queued[1]]);
    for (const p of queued.slice(1)) expect(await owner(pool, p.split("-").map(Number) as Pair)).toBe(0);
    // Worker 1 still caches the stolen pair; only the owner's report counts
    expect((await pool.reevaluate(args)).map(pairOf)).toEqual(pairs.map(([s, d]) => `${s}-${d}`));
  });

  it("cancel restarts only the busy Workers, so idle ones keep their pairs", async () => {
    const { pool, workers } = await setup(2);
    const pairs = allPairs(4);
    workers[0].hold = true;

    const running = pool.runPairs(args, pairs, () => {});
    await vi.waitFor(() => expect(workers[1].ran().length).toBe(pairs.length - 1));
    pool.cancel();

    await expect(running).rejects.toBeInstanceOf(Cancelled);
    expect(workers.length).toBe(3);
    expect([workers[0].terminated, workers[1].terminated]).toEqual([true, false]);
    expect(workers[2].log[0]).toMatchObject({ method: "open", args: { json: "g" } });
    const kept = await pool.reevaluate(args);
    expect(kept.map(pairOf)).toEqual(workers[1].ran());
    expect(kept.length).toBe(pairs.length - 1);
  });

  it("forgets owners when another graph is opened", async () => {
    const { pool } = await setup(2);
    await run(pool, [[0, 1], [0, 2]]);
    const second = (await owner(pool, [0, 1])) === 1 ? [0, 1] : [0, 2];

    await pool.open("g");
    expect(await owner(pool, second as Pair)).toBe(1);
    await pool.open("other");
    expect(await owner(pool, second as Pair)).toBe(0);
  });

  it("stops handing out pairs after a call fails", async () => {
    const { pool, workers } = await setup(1);

    await expect(run(pool, [[99, 1], [0, 1]])).rejects.toThrow("bad pair");
    expect(workers[0].ran()).toEqual(["99-1"]);
  });
});
