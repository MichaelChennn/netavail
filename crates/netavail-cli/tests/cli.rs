//! End-to-end runs of the `netavail` binary on a small reference topology.

use std::path::PathBuf;
use std::process::{Command, Output};

fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/reference/Germany_17")
}

fn netavail(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_netavail"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn algorithms_lists_the_registry() {
    let out = netavail(&["algorithms"]);

    assert!(out.status.success());
    assert!(stdout(&out).contains("dfs"));
    assert!(stdout(&out).contains("cut_disjoint"));
}

#[test]
fn verify_passes_on_reference_and_fails_on_a_wrong_path() {
    let dir = reference_dir();
    let topo = dir.join("topology.json");

    let ok = netavail(&["verify", topo.to_str().unwrap(), dir.to_str().unwrap()]);

    assert!(ok.status.success(), "{}", stdout(&ok));
    assert!(stdout(&ok).contains("ok    paths dfs"));

    // Drop one path of pair 0->1: still valid, but no longer complete
    let broken = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("verify_broken");
    std::fs::create_dir_all(&broken).unwrap();
    let csv = std::fs::read_to_string(dir.join("Pathset_Germany_17.csv")).unwrap();
    let csv = csv.replacen(
        "0,1,\"[[0, 13, 1], [0, 14, 1], [0, 2, 12, 16, 9, 5, 1]]\",3",
        "0,1,\"[[0, 13, 1], [0, 14, 1]]\",2",
        1,
    );
    std::fs::write(broken.join("Pathset_Germany_17.csv"), csv).unwrap();

    let fail = netavail(&["verify", topo.to_str().unwrap(), broken.to_str().unwrap()]);

    assert!(!fail.status.success());
    assert!(stdout(&fail).contains("FAIL  paths dfs: 1 mismatches"));
    assert!(stdout(&fail).contains("skip  avail cut_disjoint"));
}

#[test]
fn run_writes_one_row_per_pair_and_exported_sets_verify() {
    let dir = reference_dir();
    let topo = dir.join("topology.json");
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("run_export");
    let csv = tmp.join("results.csv");
    let sets = tmp.join("sets");
    std::fs::create_dir_all(&tmp).unwrap();

    let out = netavail(&[
        "run",
        topo.to_str().unwrap(),
        "--out",
        csv.to_str().unwrap(),
        "--sets-dir",
        sets.to_str().unwrap(),
    ]);

    assert!(out.status.success());
    let rows = std::fs::read_to_string(&csv).unwrap();
    assert_eq!(rows.lines().count(), 1 + 17 * 16 / 2);
    assert!(rows.lines().nth(1).unwrap().starts_with("0,1,3,"));
    let check = netavail(&["verify", topo.to_str().unwrap(), sets.to_str().unwrap()]);
    assert!(check.status.success(), "{}", stdout(&check));
    assert!(sets.join("meta.json").exists());
}

/// Path a-b-c with links: a and c are joined only if a, b, c and both links are up.
#[test]
fn run_with_link_prob_lets_links_fail() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("run_links");
    std::fs::create_dir_all(&tmp).unwrap();
    let topo = tmp.join("path.json");
    std::fs::write(
        &topo,
        r#"{"nodes": [{"id": "a"}, {"id": "b"}, {"id": "c"}],
            "edges": [{"source": "a", "target": "b"}, {"source": "b", "target": "c"}]}"#,
    )
    .unwrap();
    let topo = topo.to_str().unwrap();

    let out = netavail(&[
        "run",
        topo,
        "--avail",
        "path_sdp",
        "--p",
        "0.9",
        "--link-prob",
        "0.8",
    ]);

    assert!(out.status.success());
    let csv = stdout(&out);
    let rows: Vec<Vec<&str>> = csv
        .lines()
        .skip(1)
        .map(|l| l.split(',').collect())
        .collect();
    // Link nodes 3 and 4 never appear as a source or destination
    let pairs: Vec<(&str, &str)> = rows.iter().map(|r| (r[0], r[1])).collect();
    assert_eq!(pairs, [("0", "1"), ("0", "2"), ("1", "2")]);
    let a_to_c: f64 = rows[1][4].parse().unwrap();
    assert!(
        (a_to_c - 0.9f64.powi(3) * 0.8f64.powi(2)).abs() < 1e-12,
        "{a_to_c}"
    );

    let bad = netavail(&["run", topo, "--link-prob", "1.5"]);
    assert!(!bad.status.success());
    let export = netavail(&["run", topo, "--link-prob", "0.8", "--sets-dir", "x"]);
    assert!(!export.status.success());
}

/// Path a-b-c whose nodes and links carry their own probabilities in the file.
#[test]
fn run_with_file_uses_each_node_and_link_probability() {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("run_file");
    std::fs::create_dir_all(&tmp).unwrap();
    let topo = tmp.join("path.json");
    std::fs::write(
        &topo,
        r#"{"nodes": [{"id": "a", "prob": 0.5}, {"id": "b"}, {"id": "c", "prob": 0.25}],
            "edges": [{"source": "a", "target": "b", "prob": 0.8},
                      {"source": "b", "target": "c", "prob": 0.6}]}"#,
    )
    .unwrap();
    let topo = topo.to_str().unwrap();

    let out = netavail(&[
        "run",
        topo,
        "--avail",
        "path_sdp",
        "--p",
        "0.9,file",
        "--link-prob",
        "file",
    ]);

    assert!(out.status.success());
    let csv = stdout(&out);
    let mut lines = csv.lines();
    let header = lines.next().unwrap();
    assert!(
        header.contains(",0.9 Availability,file Availability,"),
        "{header}"
    );
    let a_to_c: Vec<f64> = lines.nth(1).unwrap().split(',').collect::<Vec<_>>()[4..6]
        .iter()
        .map(|v| v.parse().unwrap())
        .collect();
    let expected = [0.9f64.powi(3) * 0.8 * 0.6, 0.5 * 0.25 * 0.8 * 0.6];
    for (got, want) in a_to_c.iter().zip(expected) {
        assert!((got - want).abs() < 1e-12, "{got} vs {want}");
    }

    let bad = netavail(&["run", topo, "--p", "high"]);
    assert!(!bad.status.success());
}

/// Links that never fail give the same availabilities as no link failures.
#[test]
fn run_with_perfect_links_matches_run_without_links() {
    let topo = reference_dir().join("topology.json");
    let topo = topo.to_str().unwrap();
    let availabilities = |extra: &[&str]| -> Vec<f64> {
        let mut args = vec!["run", topo, "--avail", "path_sdp", "--p", "0.9"];
        args.extend_from_slice(extra);
        let out = netavail(&args);
        assert!(out.status.success());
        stdout(&out)
            .lines()
            .skip(1)
            .map(|l| l.split(',').nth(4).unwrap().parse().unwrap())
            .collect()
    };

    let plain = availabilities(&[]);
    let links = availabilities(&["--link-prob", "1"]);

    assert_eq!(plain.len(), links.len());
    for (a, b) in plain.iter().zip(&links) {
        assert!((a - b).abs() < 1e-12, "{a} vs {b}");
    }
}

/// One thread and several threads give the same rows in the same order.
#[test]
fn run_threads_do_not_change_results() {
    let topo = reference_dir().join("topology.json");
    let topo = topo.to_str().unwrap();
    // Columns: source,target,paths,cuts,<p>,paths_ms,cuts_ms,prepare_ms,evaluate_ms,error
    let rows = |threads: &str| -> Vec<String> {
        let args = [
            "run",
            topo,
            "--cuts",
            "decision_tree",
            "--avail",
            "cut_disjoint",
            "--p",
            "0.9",
            "--threads",
            threads,
        ];
        let out = netavail(&args);
        assert!(out.status.success());
        stdout(&out)
            .lines()
            .map(|l| {
                let cells: Vec<_> = l.split(',').collect();
                [&cells[..5], &cells[9..]].concat().join(",")
            })
            .collect()
    };

    assert_eq!(rows("1"), rows("4"));
}
