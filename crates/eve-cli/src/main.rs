//! eve-dogma-f CLI — stateless: JSON FitRequest in, JSON FitStats out. The dataset is compiled in.
//! This tool links the engine (`eve-dogma`) and the fit formats (`eve-fit-formats`); the engine itself takes
//! structured input only. `serve-stdio` serves both method tables (formats: eft_parse, eft_export, format_import,
//! format_export).
use std::io::{BufRead, Read, Write};
use std::time::Instant;

const USAGE: &str = "eve-dogma-f <command> [args]   (dataset compiled in; --dataset PATH is accepted and ignored)

Commands:
  calc [FILE]            FitRequest JSON (file or stdin) -> FitStats JSON
  batch                  JSONL FitRequests on stdin -> JSONL FitStats on stdout (a BatchRequest line -> one BatchResponse line)
  batch --request FILE|- BatchRequest JSON (docs/23: fits / variants / product / sweep) -> BatchResponse JSON
  optimize [FILE]        OptimizeRequest JSON (docs/21) -> ranked fits
  serve-stdio            JSONL RPC: {\"id\":..,\"method\":\"calc|batch|prices_load|version|sde_override|optimize|graph|search|type|meta|eft_parse|eft_export|format_import|format_export|fits.backup|item.variations|item.compare|market.group|market.search|implant_sets.list|character.import_evemon|names.resolve|pyfa_data_load|pyfa_data_status\",\"params\":..}
  eft [FILE]             EFT text (file or stdin) -> FitRequest JSON (add --calc to compute, --skills N)
  search QUERY [--limit N] [--kinds ship,module,..]  search types by name (exact > prefix > substring)
  type ID|NAME           show type with base attributes
  meta                   compiled dataset info
  version                engine / SDE / price snapshot provenance (docs/22 §2.3)
  bench [FILE] [-n N]    time N calculations of a request

Global options (before the command):
  --sde FILE.edp         SDE pack override (docs/22 §2.4); load failure -> SDE_LOAD_FAILED
  --prices FILE          market price table for this process (eve-price-snapshot v1 file, {\"isk\":{..}} or
                         {\"<type_id>\": isk}); price layer L4, see eve-fit-docs docs/23 §5
  --pyfa-data FILE       Pyfa-derived data (eve-sde-pipeline presets-pyfa-*.json, GPL-3.0; also EVE_DOGMA_PYFA_DATA):
                         search jargon for market.search, renamed-item conversions for names.resolve";

/// JSONL batch line: a FitRequest (calc) or a BatchRequest (has "batch_version").
fn calc_or_batch(line: &str) -> String {
    if line.contains("\"batch_version\"") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v.get("batch_version").is_some() {
                return serde_json::to_string(&eve_dogma::batch::run(&v)).unwrap_or_default();
            }
        }
    }
    eve_dogma::calc_json(line)
}

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

/// One RPC line: the formats methods go to `eve-fit-formats` (with the engine as stats source for `shipstats`),
/// everything else to the engine.
fn rpc(line: &str) -> serde_json::Value {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return eve_dogma::rpc(line) };
    let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("calc");
    if m == "optimize" {
        let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let p = v.get("params").cloned().unwrap_or(serde_json::Value::Null);
        return serde_json::json!({"id": id, "result": eve_optimizer::optimize_value(&p)});
    }
    if !eve_fit_formats::METHODS.contains(&m) {
        return eve_dogma::rpc(line);
    }
    let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
    let p = v.get("params").cloned().unwrap_or(serde_json::Value::Null);
    let stats = |r: &eve_dogma::FitRequest| eve_dogma::calc(r).to_value_raw();
    serde_json::json!({"id": id, "result": eve_fit_formats::rpc_method(m, &p, Some(&stats))})
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
    // global flags before the subcommand (docs/22 §2.4, docs/23 §8a): load errors are a JSON error on stdout, exit 2
    let fail = |v: serde_json::Value| -> ! {
        println!("{v}");
        std::process::exit(2)
    };
    if let Some(path) = take_flag(&mut args, "--sde") {
        if let Err(e) = eve_dogma::prov::load_pack_path(&path) {
            fail(e);
        }
    }
    if let Some(path) = take_flag(&mut args, "--pyfa-data") {
        if let Err(e) = eve_dogma::lookup::load_pyfa_data(&path) {
            fail(serde_json::json!({"error": {"code": "BAD_PYFA_DATA", "message": e}}));
        }
    }
    if let Some(path) = take_flag(&mut args, "--prices") {
        match eve_dogma::price::read_file(&path) {
            Ok(m) => eve_dogma::price::set_market(Some(m)),
            Err(e) => fail(serde_json::json!({"error": {"code": e.code, "message": format!("--prices {path}: {}", e.message)}})),
        }
    }
    let cmd = args.first().cloned().unwrap_or_default();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    match cmd.as_str() {
        "calc" => {
            let s = read_input(args.get(1));
            let res = eve_dogma::calc_json(&s);
            writeln!(out, "{res}").or_pipe();
            out.flush().or_pipe();
            if res.starts_with("{\"error\"") {
                std::process::exit(2);
            }
        }
        "batch" => {
            if let Some(f) = take_flag(&mut args, "--request") {
                let res = eve_dogma::batch_json(&read_input(Some(&f)));
                writeln!(out, "{res}").or_pipe();
                out.flush().or_pipe();
                if res.starts_with("{\"error\"") {
                    std::process::exit(2);
                }
            } else {
                batch(&mut out, calc_or_batch)
            }
        }
        "optimize" => {
            let s = read_input(args.get(1));
            let res = match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(p) => eve_optimizer::optimize_value(&p),
                Err(e) => serde_json::json!({"error": {"code": "BAD_JSON", "message": e.to_string()}}),
            };
            writeln!(out, "{}", serde_json::to_string_pretty(&res).unwrap()).or_pipe();
            out.flush().or_pipe();
            if res.get("results").is_none() {
                std::process::exit(2);
            }
        }
        "graph-batch" => batch(&mut out, eve_dogma::graphs::graph_json),
        "version" => {
            writeln!(out, "{}", serde_json::to_string_pretty(&eve_dogma::prov::version()).unwrap()).or_pipe();
        }
        "graph-specs" => {
            writeln!(out, "{}", eve_dogma::graphs::SPEC_JSON.trim()).or_pipe();
        }
        "graph" => {
            let s = read_input(args.get(1));
            let res = eve_dogma::graphs::graph_json(&s);
            writeln!(out, "{res}").or_pipe();
            out.flush().or_pipe();
            if res.starts_with("{\"error\"") {
                std::process::exit(2);
            }
        }
        "eft" => {
            let skills = args.iter().position(|a| a == "--skills").map(|p| {
                let v = args.get(p + 1).cloned().unwrap_or_default();
                args.drain(p..(p + 2).min(args.len()));
                v
            });
            let do_calc = args.iter().any(|a| a == "--calc");
            args.retain(|a| a != "--calc");
            let s = read_input(args.get(1));
            match eve_fit_formats::eft::parse(&s) {
                Ok(mut r) => {
                    if let Some(l) = skills {
                        r.character.skills.default_level = l.parse().ok();
                    }
                    let v = if do_calc { serde_json::to_value(eve_dogma::calc(&r)).unwrap() } else { serde_json::to_value(&r).unwrap() };
                    writeln!(out, "{}", serde_json::to_string_pretty(&v).unwrap()).or_pipe();
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2)
                }
            }
        }
        "serve-stdio" => {
            eprintln!("eve-dogma-f serve-stdio ready (sde {})", eve_dogma::data::SDE_BUILD);
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                if line.trim().is_empty() {
                    continue;
                }
                writeln!(out, "{}", serde_json::to_string(&rpc(&line)).unwrap()).or_pipe();
                out.flush().or_pipe();
            }
        }
        "search" => {
            let (mut q, mut limit, mut kinds): (Vec<String>, usize, Option<Vec<String>>) = (Vec::new(), 20, None);
            let mut it = args[1..].iter();
            while let Some(x) = it.next() {
                match x.as_str() {
                    "--limit" => limit = it.next().and_then(|v| v.parse().ok()).unwrap_or(20),
                    "--kinds" => kinds = it.next().map(|v| v.split(',').map(|k| k.trim().to_lowercase()).collect()),
                    _ => q.push(x.clone()),
                }
            }
            let r = eve_dogma::search_kinds(&q.join(" "), limit, kinds.as_deref());
            writeln!(out, "{}", serde_json::to_string_pretty(&r).unwrap()).or_pipe();
        }
        "type" => {
            writeln!(out, "{}", serde_json::to_string_pretty(&eve_dogma::type_info(&args[1..].join(" "))).unwrap()).or_pipe();
        }
        "meta" => {
            writeln!(out, "{}", serde_json::to_string_pretty(&eve_dogma::meta()).unwrap()).or_pipe();
        }
        "bench" => {
            let n: usize = take_flag(&mut args, "-n").and_then(|v| v.parse().ok()).unwrap_or(1000);
            let s = read_input(args.get(1));
            let req: eve_dogma::FitRequest = serde_json::from_str(&s).expect("bad request");
            let _ = eve_dogma::calc(&req);
            let t1 = Instant::now();
            for _ in 0..n {
                std::hint::black_box(eve_dogma::calc(&req));
            }
            let el = t1.elapsed().as_secs_f64();
            // phase breakdown: build+register / stats / serialize
            let (mut tb, mut ts, mut tj) = (0f64, 0f64, 0f64);
            for _ in 0..n {
                let t = Instant::now();
                let fit = eve_dogma::engine::Fit::build(&req).expect("build");
                let t2 = Instant::now();
                let v = fit.compute_stats(&req);
                let t3 = Instant::now();
                std::hint::black_box(v.to_json_string());
                tj += t3.elapsed().as_secs_f64();
                ts += (t3 - t2).as_secs_f64();
                tb += (t2 - t).as_secs_f64();
            }
            let us = |x: f64| x / n as f64 * 1e6;
            writeln!(out, "{}", serde_json::json!({"iterations": n, "total_s": el, "per_calc_us": el / n as f64 * 1e6,
                "build_us": us(tb), "stats_us": us(ts), "serialize_us": us(tj)})).or_pipe();
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(if cmd.is_empty() || cmd == "help" || cmd == "--help" { 0 } else { 2 });
        }
    }
}

fn threads() -> usize {
    if let Some(n) = std::env::var("EVE_DOGMA_THREADS").ok().and_then(|v| v.parse::<usize>().ok()) {
        return n.max(1);
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(16)
}

/// JSONL batch: requests are independent and the engine is pure, so N workers compute in parallel while the
/// output keeps input order. Output is flushed whenever it has caught up with the input (interactive use works).
fn batch(out: &mut impl Write, f: fn(&str) -> String) {
    let n = if cfg!(target_arch = "wasm32") { 1 } else { threads() };
    if n <= 1 {
        for line in std::io::stdin().lock().lines() {
            let line = line.unwrap();
            if line.trim().is_empty() {
                continue;
            }
            writeln!(out, "{}", f(&line)).or_pipe();
            out.flush().or_pipe();
        }
        return;
    }
    use std::sync::{mpsc, Arc, Mutex};
    let (job_tx, job_rx) = mpsc::channel::<(u64, String)>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    let (res_tx, res_rx) = mpsc::channel::<(u64, String)>();
    let mut workers = Vec::new();
    for _ in 0..n {
        let rx = Arc::clone(&job_rx);
        let tx = res_tx.clone();
        workers.push(std::thread::spawn(move || loop {
            let job = rx.lock().unwrap().recv();
            let Ok((seq, line)) = job else { break };
            if tx.send((seq, f(&line))).is_err() {
                break;
            }
        }));
    }
    drop(res_tx);
    let reader = std::thread::spawn(move || {
        let mut seq = 0u64;
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            if job_tx.send((seq, line)).is_err() {
                break;
            }
            seq += 1;
        }
    });
    let mut pending: std::collections::BTreeMap<u64, String> = std::collections::BTreeMap::new();
    let mut next = 0u64;
    while let Ok((seq, r)) = res_rx.recv() {
        pending.insert(seq, r);
        while let Some(r) = pending.remove(&next) {
            out.write_all(r.as_bytes()).or_pipe();
            out.write_all(b"\n").or_pipe();
            next += 1;
        }
        // drain whatever else is ready before flushing
        while let Ok((seq, r)) = res_rx.try_recv() {
            pending.insert(seq, r);
            while let Some(r) = pending.remove(&next) {
                out.write_all(r.as_bytes()).or_pipe();
                out.write_all(b"\n").or_pipe();
                next += 1;
            }
        }
        out.flush().or_pipe();
    }
    reader.join().ok();
    for w in workers {
        w.join().ok();
    }
}

/// Output errors: a closed reader (EPIPE, e.g. `| head`) ends the process quietly with status 0; anything else is fatal.
trait OrPipe {
    fn or_pipe(self);
}
impl OrPipe for std::io::Result<()> {
    fn or_pipe(self) {
        if let Err(e) = self {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            eprintln!("error: write failed: {e}");
            std::process::exit(1);
        }
    }
}
