//! Print the masked form of each file name read from stdin, one per line.
//! `cargo run -p tidyward-core --example mask -- 2026-10-25 < names.txt` (date optional).
use std::io::BufRead;

fn main() {
    let today = std::env::args()
        .nth(1)
        .and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    for name in std::io::stdin().lock().lines().map_while(Result::ok) {
        println!("{}", tidyward_core::mask::mask(&name, today));
    }
}
