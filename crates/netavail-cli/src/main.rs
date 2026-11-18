//! `netavail` command line (architecture.md §6; D33).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use netavail_core::control::Cancelled;
use netavail_core::registry::{self, AlgoInfo};
use netavail_core::types::{SetFamily, SetKind};
use netavail_core::{Cache, Control, Plan, all_pairs, run_pair, run_pair_cached};
use netavail_io::Topology;
use netavail_io::reference::{
    check_availability, check_cuts, check_paths, read_availability, read_sets, validate_sets,
};
use netavail_io::results::{AlgoMeta, Meta, PairReport, StageTimer, hex, results_csv, sets_csv};

#[derive(Parser)]
#[command(
    name = "netavail",
    version,
    about = "Network availability from path and cut sets"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the registered algorithms.
    Algorithms,
    /// Compute every node pair of a topology and write a CSV with stage times.
    Run {
        /// Topology JSON (PyRBD-Suite frontend format).
        topology: PathBuf,
        /// Path algorithm id.
        #[arg(long, default_value = "dfs")]
        paths: String,
        /// Cut algorithm id.
        #[arg(long)]
        cuts: Option<String>,
        /// Availability algorithm id.
        #[arg(long)]
        avail: Option<String>,
        /// Node availabilities, one scenario each; every node gets the same p.
        #[arg(long, value_delimiter = ',', default_value = "0.9,0.99,0.999,0.9999")]
        p: Vec<f64>,
        /// Result CSV; standard output if omitted.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also export the sets in the reference format, with meta.json.
        #[arg(long)]
        sets_dir: Option<PathBuf>,
    },
    /// Compare every registered algorithm with reference answers.
    Verify {
        /// Topology JSON.
        topology: PathBuf,
        /// Directory with Pathset_*.csv, Mincutset_*.csv and Availability_*.csv.
        reference_dir: PathBuf,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Algorithms => {
            print!("{}", algorithms_table());
            Ok(true)
        }
        Command::Run {
            topology,
            paths,
            cuts,
            avail,
            p,
            out,
            sets_dir,
        } => run(&topology, &paths, cuts, avail, &p, out, sets_dir).map(|()| true),
        Command::Verify {
            topology,
            reference_dir,
        } => verify(&topology, &reference_dir),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn algorithms_table() -> String {
    let mut rows: Vec<(&str, &AlgoInfo)> = Vec::new();
    rows.extend(
        registry::path_algorithms()
            .iter()
            .map(|a| ("paths", a.info())),
    );
    rows.extend(
        registry::cut_algorithms()
            .iter()
            .map(|a| ("cuts", a.info())),
    );
    rows.extend(
        registry::availability_algorithms()
            .iter()
            .map(|a| ("avail", a.info())),
    );
    let mut out = format!(
        "{:<6} {:<18} {:<18} {:>3}  {:<16} {}\n",
        "kind", "id", "name", "ver", "old names", "summary"
    );
    for (kind, info) in rows {
        out += &format!(
            "{:<6} {:<18} {:<18} {:>3}  {:<16} {}\n",
            kind,
            info.id,
            info.name,
            info.version,
            info.aliases.join(","),
            info.summary
        );
    }
    out
}

fn read_topology(path: &Path) -> Result<Topology> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Topology::from_json(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Set by Ctrl-C; pairs already finished are still written out.
static CANCELLED: AtomicBool = AtomicBool::new(false);

struct CtrlC;

impl Control for CtrlC {
    fn check(&self) -> Result<(), Cancelled> {
        if CANCELLED.load(Ordering::Relaxed) {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Milliseconds since the first call.
fn clock() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1e3
}

fn run(
    topology: &Path,
    paths: &str,
    cuts: Option<String>,
    avail: Option<String>,
    probs: &[f64],
    out: Option<PathBuf>,
    sets_dir: Option<PathBuf>,
) -> Result<()> {
    let topo = read_topology(topology)?;
    let path_algo = registry::find_path(paths).with_context(|| unknown("path", paths))?;
    let cut_algo = match &cuts {
        Some(id) => Some(registry::find_cut(id).with_context(|| unknown("cut", id))?),
        None => None,
    };
    let avail_algo = match &avail {
        Some(id) => {
            Some(registry::find_availability(id).with_context(|| unknown("availability", id))?)
        }
        None => None,
    };
    if let Some(&p) = probs.iter().find(|p| !(0.0..=1.0).contains(*p)) {
        bail!("--p {p} is not in [0, 1]");
    }
    let plan = Plan::new(path_algo, cut_algo, avail_algo)?;
    let g = topo.graph();
    let n = g.node_count() as usize;
    let scenarios: Vec<_> = probs
        .iter()
        .map(|&p| netavail_core::ProbabilityMap::uniform(n, p))
        .collect();
    ctrlc::set_handler(|| CANCELLED.store(true, Ordering::Relaxed))
        .context("installing the Ctrl-C handler")?;

    // Sets are kept only when they will be exported
    let mut cache = sets_dir.as_ref().map(|_| Cache::new(g));
    let pairs = all_pairs(g.node_count());
    let started = clock();
    let mut reports = Vec::with_capacity(pairs.len());
    for &(src, dst) in &pairs {
        if CANCELLED.load(Ordering::Relaxed) {
            eprintln!("cancelled after {} of {} pairs", reports.len(), pairs.len());
            break;
        }
        let timer = StageTimer::new(&clock, &CtrlC);
        let result = match cache.as_mut() {
            Some(cache) => run_pair_cached(&plan, g, src, dst, &scenarios, &timer, cache),
            None => run_pair(&plan, g, src, dst, &scenarios, &timer),
        };
        reports.push(PairReport {
            result,
            times: timer.times(),
        });
    }
    let failed = reports.iter().filter(|r| r.result.error.is_some()).count();
    eprintln!(
        "{}: {} pairs in {:.1} ms ({failed} failed)",
        topo.name(),
        reports.len(),
        clock() - started
    );

    let csv = results_csv(if avail_algo.is_some() { probs } else { &[] }, &reports);
    match out {
        Some(path) => {
            std::fs::write(&path, csv).with_context(|| format!("writing {}", path.display()))?
        }
        None => print!("{csv}"),
    }
    if let (Some(dir), Some(cache)) = (sets_dir, cache) {
        export_sets(&dir, &topo, &plan, &pairs[..reports.len()], &cache)?;
    }
    Ok(())
}

fn unknown(kind: &str, id: &str) -> String {
    format!("unknown {kind} algorithm {id:?}; see `netavail algorithms`")
}

/// Writes Pathset_<T>.csv, Mincutset_<T>.csv (if cuts ran) and meta.json.
fn export_sets(
    dir: &Path,
    topo: &Topology,
    plan: &Plan,
    pairs: &[(u32, u32)],
    cache: &Cache,
) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let keys: Vec<_> = pairs
        .iter()
        .map(|&(s, d)| plan.keys(cache.graph(), s, d))
        .collect();
    // A pair that failed has no sets; it is left out of the export
    let paths: Vec<SetFamily> = keys
        .iter()
        .filter_map(|k| cache.paths(&k.paths).cloned())
        .collect();
    let cuts: Vec<SetFamily> = keys
        .iter()
        .filter_map(|k| cache.cuts(&k.cuts?).cloned())
        .collect();
    let name = if topo.name().is_empty() {
        "topology"
    } else {
        topo.name()
    };
    let write = |file: String, text: String| {
        let path = dir.join(file);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    };
    write(format!("Pathset_{name}.csv"), sets_csv(&paths))?;
    // Only the algorithm parts of these keys matter here
    let algos = plan.keys(cache.graph(), 0, 0);
    if algos.cuts.is_some() {
        write(format!("Mincutset_{name}.csv"), sets_csv(&cuts))?;
    }
    let meta = Meta {
        graph: hex(cache.graph()),
        paths: AlgoMeta::from(algos.paths.algo),
        cuts: algos.cuts.map(|c| AlgoMeta::from(c.algo)),
    };
    write("meta.json".into(), meta.to_json() + "\n")?;
    Ok(())
}

/// Returns whether everything that could be checked matched.
fn verify(topology: &Path, dir: &Path) -> Result<bool> {
    let topo = read_topology(topology)?;
    let g = topo.graph();
    let find = |prefix: &str| -> Result<Option<String>> {
        let mut hits = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            if file.starts_with(prefix) && file.ends_with(".csv") {
                hits.push(path);
            }
        }
        match hits.as_slice() {
            [] => Ok(None),
            [path] => Ok(Some(std::fs::read_to_string(path)?)),
            _ => bail!("more than one {prefix}*.csv in {}", dir.display()),
        }
    };
    let sets = |prefix: &str, kind| -> Result<Option<Vec<SetFamily>>> {
        let Some(text) = find(prefix)? else {
            return Ok(None);
        };
        let families = read_sets(&text, kind).with_context(|| format!("{prefix}*.csv"))?;
        validate_sets(g, &families).with_context(|| format!("{prefix}*.csv"))?;
        Ok(Some(families))
    };
    let paths = sets("Pathset_", SetKind::Paths)?;
    let cuts = sets("Mincutset_", SetKind::Cuts)?;
    let table = find("Availability_")?
        .map(|text| read_availability(&text))
        .transpose()
        .context("Availability_*.csv")?;

    let mut all_ok = true;
    let mut report = |what: String, failures: Option<Vec<String>>| match failures {
        None => println!("skip  {what}: reference file missing"),
        Some(f) if f.is_empty() => println!("ok    {what}"),
        Some(f) => {
            all_ok = false;
            println!("FAIL  {what}: {} mismatches", f.len());
            for line in f.iter().take(10) {
                println!("      {line}");
            }
        }
    };
    for algo in registry::path_algorithms() {
        let failures = paths.as_ref().map(|p| check_paths(*algo, g, p));
        report(format!("paths {}", algo.info().id), failures);
    }
    for algo in registry::cut_algorithms() {
        let failures = paths
            .as_ref()
            .zip(cuts.as_ref())
            .map(|(p, c)| check_cuts(*algo, p, c));
        report(format!("cuts  {}", algo.info().id), failures);
    }
    for algo in registry::availability_algorithms() {
        let input = match algo.input() {
            SetKind::Paths => paths.as_ref(),
            SetKind::Cuts => cuts.as_ref(),
        };
        let failures = input
            .zip(table.as_ref())
            .map(|(sets, t)| check_availability(*algo, sets, t, g.node_count()));
        report(format!("avail {}", algo.info().id), failures);
    }
    Ok(all_ok)
}
