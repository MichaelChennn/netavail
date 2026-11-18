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
