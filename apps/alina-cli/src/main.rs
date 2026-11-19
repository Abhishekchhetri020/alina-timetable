use alina_doc::TimetableDoc;
use alina_solver::{CardRequest, Status};
use std::{env, fs, process::ExitCode, sync::Arc};

fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, path] if command == "verify" => {
            let doc: TimetableDoc = serde_json::from_str(&fs::read_to_string(path)?)?;
            let report = alina_oracle::verify(&doc);
            println!(
                "Placed: {}\nRequired: {}\nUnplaced: {}\nHard violations: {}",
                report.placed,
                report.required,
                report.unplaced,
                report.violations.len()
            );
            for violation in &report.violations {
                println!("{violation:?}");
            }
            Ok(report.violations.is_empty() && report.unplaced == 0)
        }
        [command, path, out] if command == "solve-chronexa" => {
            let imported = alina_codecs::import_chronexa_json(&fs::read_to_string(path)?)?;
            let requests: Vec<CardRequest> = imported
                .requests
                .iter()
                .map(|c| CardRequest {
                    card: c.card,
                    lesson: c.lesson,
                    rooms: c.rooms.clone(),
                })
                .collect();
            let result = alina_solver::solve(&imported.document, &requests, 100_000)
                .map_err(|e| format!("solver error: {e:?}"))?;
            let mut doc = imported.document;
            for placement in result.placements {
                Arc::make_mut(&mut doc.placements).insert(placement.card, placement);
            }
            fs::write(out, serde_json::to_string_pretty(&doc)?)?;
            println!(
                "Status: {:?}; placed: {}; unplaced: {}; search nodes: {}",
                result.status,
                doc.placements.len(),
                result.unplaced.len(),
                result.nodes
            );
            Ok(result.status == Status::Complete)
        }
        _ => Err(
            "usage: alina-cli verify <alina.json> | solve-chronexa <school.json> <output.json>"
                .into(),
        ),
    }
}
fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
