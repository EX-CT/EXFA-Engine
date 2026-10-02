//! eve-dogma-f CLI — stateless: JSON FitRequest in, JSON FitStats out. The dataset is compiled in.
use std::io::{BufRead, Read, Write};
use std::time::Instant;

const USAGE: &str = "eve-dogma-f <command> [args]   (dataset compiled in; --dataset PATH is accepted and ignored)

Commands:
  calc [FILE]            FitRequest JSON (file or stdin) -> FitStats JSON
  batch                  JSONL FitRequests on stdin -> JSONL FitStats on stdout
  serve-stdio            JSONL RPC: {\"id\":..,\"method\":\"calc|search|type|meta\",\"params\":..}
  search QUERY           search types by name
  type ID|NAME           show type with base attributes
  meta                   compiled dataset info
  bench [FILE] [-n N]    time N calculations of a request";

fn read_input(file: Option<&String>) -> String {
    let mut s = String::new();
    match file {
        Some(f) if f != "-" => {
            s = std::fs::read_to_string(f).unwrap_or_else(|e| {
                eprintln!("error: {f}: {e}");
                std::process::exit(2)
            })
        }
        _ => {
            std::io::stdin().read_to_string(&mut s).unwrap();
        }
    }
    s
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(p) = args.iter().position(|x| x == "--dataset") {
        args.drain(p..(p + 2).min(args.len()));
    }
    let take_flag = |args: &mut Vec<String>, f: &str| -> Option<String> {
        let p = args.iter().position(|x| x == f)?;
        let v = args.get(p + 1).cloned();
        args.drain(p..(p + 2).min(args.len()));
        v
    };
    let cmd = args.first().cloned().unwrap_or_default();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    match cmd.as_str() {
        "calc" => {
            let s = read_input(args.get(1));
            let res = eve_dogma_f::calc_json(&s);
            writeln!(out, "{res}").unwrap();
            out.flush().unwrap();
            if res.starts_with("{\"error\"") {
                std::process::exit(2);
            }
        }
        "batch" => {
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                if line.trim().is_empty() {
                    continue;
                }
                writeln!(out, "{}", eve_dogma_f::calc_json(&line)).unwrap();
            }
        }
        "serve-stdio" => {
            eprintln!("eve-dogma-f serve-stdio ready (sde {})", eve_dogma_f::data::SDE_BUILD);
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                if line.trim().is_empty() {
                    continue;
                }
                writeln!(out, "{}", serde_json::to_string(&eve_dogma_f::rpc(&line)).unwrap()).unwrap();
                out.flush().unwrap();
            }
        }
        "search" => {
            writeln!(out, "{}", serde_json::to_string_pretty(&eve_dogma_f::search(&args[1..].join(" "), 25)).unwrap()).unwrap();
        }
        "type" => {
            writeln!(out, "{}", serde_json::to_string_pretty(&eve_dogma_f::type_info(&args[1..].join(" "))).unwrap()).unwrap();
        }
        "meta" => {
            writeln!(out, "{}", serde_json::to_string_pretty(&eve_dogma_f::meta()).unwrap()).unwrap();
        }
        "bench" => {
            let n: usize = take_flag(&mut args, "-n").and_then(|v| v.parse().ok()).unwrap_or(1000);
            let s = read_input(args.get(1));
            let req: eve_dogma_f::FitRequest = serde_json::from_str(&s).expect("bad request");
            let _ = eve_dogma_f::calc(&req);
            let t1 = Instant::now();
            for _ in 0..n {
                std::hint::black_box(eve_dogma_f::calc(&req));
            }
            let el = t1.elapsed().as_secs_f64();
            writeln!(out, "{}", serde_json::json!({"iterations": n, "total_s": el, "per_calc_us": el / n as f64 * 1e6})).unwrap();
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(if cmd.is_empty() || cmd == "help" || cmd == "--help" { 0 } else { 2 });
        }
    }
}
