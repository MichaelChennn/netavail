//! Small reference topologies shared by the integration tests.

use std::path::PathBuf;

/// (name, nodes, edges), counted independently with Python's json module.
pub const TOPOLOGIES: [(&str, u32, usize); 8] = [
    ("Abilene", 11, 14),
    ("Austria_24", 24, 50),
    ("Germany_17", 17, 26),
    ("HiberniaUk", 13, 13),
    ("Spain", 17, 28),
    ("Sweden", 25, 29),
    ("dfn-bwin", 10, 45),
    ("polska", 12, 18),
];

/// Reads `tests/reference/<topology>/<file>`, panicking with the path if it is missing.
pub fn read(topology: &str, file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/reference")
        .join(topology)
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
