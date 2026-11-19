// Runs the wasm Engine off the main thread. Cancel = terminate this Worker.
import init, { Engine } from "./pkg/netavail_wasm";
import type { Reply, Request } from "./protocol";

const ready = init();
let engine: Engine | null = null;

function handle(req: Request): unknown {
  if (req.method === "algorithms") return Engine.algorithms();
  if (req.method === "open") {
    const { json } = req.args as Request<"open">["args"];
    if (engine) engine.load(json);
    else engine = new Engine(json);
    return engine.topology();
  }
  if (!engine) throw new Error("no topology loaded");
  switch (req.method) {
    case "runPair": {
      const a = req.args as Request<"runPair">["args"];
      return engine.runPair(a.plan, a.src, a.dst, a.probs, a.linkProb);
    }
    case "reevaluate": {
      const a = req.args as Request<"reevaluate">["args"];
      return engine.reevaluate(a.plan, a.probs, a.linkProb);
    }
    case "sets": {
      const a = req.args as Request<"sets">["args"];
      return engine.sets(a.plan, a.src, a.dst, a.kind, a.links);
    }
  }
}

self.onmessage = async (event: MessageEvent<Request>) => {
  const req = event.data;
  let reply: Reply;
  try {
    await ready; // A failed wasm load must still answer, or the caller waits forever
    reply = { id: req.id, ok: true, value: handle(req) };
  } catch (e) {
    reply = { id: req.id, ok: false, error: e instanceof Error ? e.message : String(e) };
  }
  self.postMessage(reply);
};
