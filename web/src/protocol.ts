// Worker messages; one method per Engine method (architecture.md §5).
import type {
  AlgorithmList,
  PairReport,
  PairReports,
  PlanSpec,
  SetKind,
  Sets,
  TopologyView,
} from "./pkg/netavail_wasm";

export type Methods = {
  // new Engine(json) the first time, engine.load(json) afterwards
  open: { args: { json: string }; result: TopologyView };
  algorithms: { args: Record<string, never>; result: AlgorithmList };
  runPair: {
    // linkProb set: every link can fail too; edge i is node n + i
    args: { plan: PlanSpec; src: number; dst: number; probs: number[]; linkProb?: number };
    result: PairReport;
  };
  reevaluate: { args: { plan: PlanSpec; probs: number[]; linkProb?: number }; result: PairReports };
  sets: {
    args: { plan: PlanSpec; src: number; dst: number; kind: SetKind; links: boolean };
    result: Sets;
  };
};

export type Method = keyof Methods;

export type Request<M extends Method = Method> = {
  id: number;
  method: M;
  args: Methods[M]["args"];
};

export type Reply =
  | { id: number; ok: true; value: unknown }
  | { id: number; ok: false; error: string };
