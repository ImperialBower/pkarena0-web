//! Validates a HandCollection YAML file using pkcore's deserialization
//! and bridge methods. Exits 0 on success, 1 on any error.
//!
//! Usage:
//! ```
//! cargo run --bin validate-yaml -- tests/fixtures/session.yaml
//! ```

use pkcore::hand_history::HandCollection;
use std::{env, fs, process};

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: validate-yaml <path>");
        process::exit(1);
    });

    let yaml = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("error reading {path}: {e}");
        process::exit(1);
    });

    let collection = HandCollection::from_yaml(&yaml).unwrap_or_else(|e| {
        eprintln!("parse error: {e}");
        process::exit(1);
    });

    if collection.is_empty() {
        eprintln!("error: YAML contains no hands");
        process::exit(1);
    }

    for (i, hand) in collection.iter().enumerate() {
        if hand.board.is_some() {
            hand.to_board().unwrap_or_else(|e| {
                eprintln!("hand {i}: invalid board — {e}");
                process::exit(1);
            });
        }

        for player in &hand.players {
            if player.hole_cards.is_some() {
                player.to_two().unwrap_or_else(|e| {
                    eprintln!("hand {i} seat {}: invalid hole_cards — {e}", player.seat);
                    process::exit(1);
                });
            }
        }

        if let Some(results) = &hand.results {
            for result in results {
                if result.best_hand.is_some() {
                    result.to_five().unwrap_or_else(|e| {
                        eprintln!("hand {i} result: invalid best_hand — {e}");
                        process::exit(1);
                    });
                }
            }
        }
    }

    println!("OK: {} hand(s) validated — {path}", collection.len());
}
