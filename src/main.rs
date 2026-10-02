use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use queens::{is_solution, beat, Board, Outcome};

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return ExitCode::from(2);
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "solve" => cmd_solve(&args),
        "bench" => cmd_bench(&args, false),
        "audit" => cmd_bench(&args, true),
        "race" => cmd_race(&args),
        "help" | "--help" | "-h" => {
            print_help();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown command '{other}'");
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!(
        "\
queens — LinkedIn Queens bot

  queens solve [board.txt|-]     Place every queen. Reads stdin when the file is -.
  queens bench <corpus.txt>      Time first-solution search and check every answer.
  queens audit <corpus.txt>      Recount every solution and compare with the corpus.
  queens race <corpus.txt>       Time the propagator against a bitmask search with no propagation.

A board is one region-row per line. Labels can be letters (AABBCC) or tokens
(0 0 1 1 2 2). Blank lines separate boards in a corpus. A corpus record is:

  name expected_solutions
  AABB
  ABBB
  AACC
  ACCC
"
    );
}

fn cmd_solve(args: &[String]) -> ExitCode {
    let text = if args.is_empty() || args[0] == "-" {
        let mut buf = String::new();
        if io::stdin().read_to_string(&mut buf).is_err() {
            eprintln!("could not read stdin");
            return ExitCode::from(1);
        }
        buf
    } else {
        match fs::read_to_string(&args[0]) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("could not read {}: {err}", args[0]);
                return ExitCode::from(1);
            }
        }
    };
    let rows: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();
    let started = Instant::now();
    let outcome = match beat(&rows, 1) {
        Ok(outcome) => outcome,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    let elapsed = started.elapsed();
    match outcome.queens {
        None => {
            println!("no solution  ({elapsed:?}, {} nodes)", outcome.nodes);
            ExitCode::from(1)
        }
        Some(queens) => {
            print_grid(&rows, &queens);
            println!(
                "{} queens in {:?}  ({} nodes)",
                queens.len(),
                elapsed,
                outcome.nodes
            );
            let _ = io::stdout().flush();
            ExitCode::SUCCESS
        }
    }
}

fn print_grid(rows: &[String], queens: &[usize]) {
    for (r, row) in rows.iter().enumerate() {
        let cells: Vec<String> = if row.chars().any(|ch| ch.is_whitespace()) {
            row.split_whitespace().map(|s| s.to_string()).collect()
        } else {
            row.chars().map(|ch| ch.to_string()).collect()
        };
        for (c, label) in cells.iter().enumerate() {
            if c > 0 {
                print!(" ");
            }
            if queens[r] == c {
                print!("Q");
            } else {
                print!("{label}");
            }
        }
        println!();
    }
    print!("columns:");
    for col in queens {
        print!(" {col}");
    }
    println!();
}

struct Record {
    name: String,
    expected: i64,
    rows: Vec<String>,
}

fn load_corpus(path: &str) -> Result<Vec<Record>, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("could not read {path}: {err}"))?;
    let mut records = Vec::new();
    let mut name = String::new();
    let mut expected = -1i64;
    let mut rows: Vec<String> = Vec::new();
    let flush = |name: &mut String, expected: &mut i64, rows: &mut Vec<String>, records: &mut Vec<Record>| {
        if rows.is_empty() {
            return;
        }
        records.push(Record {
            name: name.clone(),
            expected: *expected,
            rows: std::mem::take(rows),
        });
        name.clear();
        *expected = -1;
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            flush(&mut name, &mut expected, &mut rows, &mut records);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if rows.is_empty() {
            let mut parts = line.split_whitespace();
            let head = parts.next().unwrap();
            if let Some(num) = parts.next() {
                if let Ok(value) = num.parse::<i64>() {
                    if parts.next().is_none() && !head.chars().all(|ch| ch.is_ascii_digit()) {
                        name = head.to_string();
                        expected = value;
                        continue;
                    }
                }
            }
        }
        rows.push(line.to_string());
    }
    flush(&mut name, &mut expected, &mut rows, &mut records);
    if records.is_empty() {
        return Err(format!("{path} has no boards"));
    }
    Ok(records)
}

fn cmd_bench(args: &[String], audit: bool) -> ExitCode {
    if args.is_empty() {
        eprintln!("usage: queens {} <corpus.txt>", if audit { "audit" } else { "bench" });
        return ExitCode::from(2);
    }
    let records = match load_corpus(&args[0]) {
        Ok(records) => records,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };

    let mut boards_ok: Vec<(String, i64, Board)> = Vec::with_capacity(records.len());
    let mut invalid = 0usize;
    for record in &records {
        match Board::parse_rows(&record.rows) {
            Ok(board) => boards_ok.push((record.name.clone(), record.expected, board)),
            Err(err) => {
                eprintln!("{}: {err}", record.name);
                invalid += 1;
            }
        }
    }

    let limit = if audit { u64::MAX } else { 1 };
    for (_, _, board) in &boards_ok {
        let _ = board.solve(1);
    }

    let mut solved = 0usize;
    let mut unsat = 0usize;
    let mut mismatched = 0usize;
    let mut nodes = 0u64;
    let mut slowest_ns = 0u128;
    let mut slowest_name = String::new();
    let mut slowest_nodes = 0u64;
    let mut slow: Vec<(u128, u64, usize, String)> = Vec::new();
    let mut times: Vec<u128> = Vec::with_capacity(boards_ok.len());
    let mut compared = 0usize;
    let started = Instant::now();

    for (name, expected, board) in &boards_ok {
        let one = Instant::now();
        let outcome: Outcome = board.solve(limit);
        let elapsed = one.elapsed().as_nanos();
        times.push(elapsed);
        nodes += outcome.nodes;
        if elapsed > slowest_ns {
            slowest_ns = elapsed;
            slowest_name = name.clone();
            slowest_nodes = outcome.nodes;
        }
        if elapsed > 100_000 {
            slow.push((elapsed, outcome.nodes, board.n, name.clone()));
        }
        match &outcome.queens {
            None => {
                if *expected == 0 {
                    unsat += 1;
                } else if *expected > 0 {
                    eprintln!("{name}: solver found nothing, corpus expects {expected}");
                    mismatched += 1;
                } else {
                    unsat += 1;
                }
            }
            Some(queens) => {
                if !is_solution(board, queens) {
                    eprintln!("{name}: solution breaks the Queens rules");
                    invalid += 1;
                    continue;
                }
                solved += 1;
                if audit && *expected >= 0 {
                    compared += 1;
                    if outcome.solutions != *expected as u64 {
                        eprintln!(
                            "{name}: counted {} solutions, corpus expects {expected}",
                            outcome.solutions
                        );
                        mismatched += 1;
                    }
                }
            }
        }
    }

    let elapsed = started.elapsed();
    let boards = boards_ok.len();
    times.sort_unstable();
    let per = elapsed.as_nanos() / boards.max(1) as u128;
    let median = percentile(&times, 0.50);
    let p99 = percentile(&times, 0.99);
    println!("boards:     {boards}");
    println!("solved:     {solved}");
    println!("unsat:      {unsat}");
    println!("invalid:    {invalid}");
    println!("mismatches: {mismatched}");
    if audit {
        println!("counted:    {compared}");
    }
    println!("nodes:      {nodes}");
    println!("time:       {elapsed:?}");
    println!("per board:  {per} ns");
    println!("median:     {median} ns");
    println!("p99:        {p99} ns");
    println!(
        "slowest:    {slowest_name} ({:.3} µs, {slowest_nodes} nodes)",
        slowest_ns as f64 / 1000.0
    );
    slow.sort_by(|a, b| b.0.cmp(&a.0));
    for (ns, board_nodes, n, name) in slow.iter().take(8) {
        println!(
            "  {name}: {n}×{n}  {:.3} µs  {board_nodes} nodes",
            *ns as f64 / 1000.0
        );
    }
    if invalid == 0 && mismatched == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn cmd_race(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("usage: queens race <corpus.txt>");
        return ExitCode::from(2);
    }
    let records = match load_corpus(&args[0]) {
        Ok(records) => records,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    };
    let mut boards = Vec::new();
    for record in &records {
        if let Ok(board) = Board::parse_rows(&record.rows) {
            boards.push((record.name.clone(), board));
        }
    }
    for (_, board) in &boards {
        let _ = board.solve(1);
    }

    let mut smart_ns = Vec::new();
    let mut naive_ns = Vec::new();
    let mut speedups = Vec::new();
    let mut naive_timeouts = 0usize;
    let mut smart_slower = 0usize;
    let budget = std::time::Duration::from_millis(5);

    for (_name, board) in &boards {
        let start = Instant::now();
        let smart = board.solve(1);
        let smart_time = start.elapsed().as_nanos();
        smart_ns.push(smart_time);
        let deadline = Instant::now() + budget;
        let naive_start = Instant::now();
        match board.count_naive_until(1, Some(deadline)) {
            None => naive_timeouts += 1,
            Some(_) => {
                let naive_time = naive_start.elapsed().as_nanos().max(1);
                naive_ns.push(naive_time);
                let ratio = naive_time as f64 / smart_time.max(1) as f64;
                if ratio < 1.0 {
                    smart_slower += 1;
                }
                speedups.push(ratio);
            }
        }
        let _ = smart;
    }

    smart_ns.sort_unstable();
    naive_ns.sort_unstable();
    speedups.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("boards:          {}", boards.len());
    println!("agent median:      {} ns", percentile(&smart_ns, 0.50));
    println!("agent p99:         {} ns", percentile(&smart_ns, 0.99));
    println!("agent max:         {} ns", smart_ns.last().copied().unwrap_or(0));
    println!("baseline timeouts: {naive_timeouts} (cap 5 ms, first solution)");
    println!("baseline median:   {} ns", percentile(&naive_ns, 0.50));
    if !speedups.is_empty() {
        let mid = speedups[speedups.len() / 2];
        let mean = speedups.iter().sum::<f64>() / speedups.len() as f64;
        println!("median speedup:    {mid:.2}x on boards the baseline finished");
        println!("mean speedup:      {mean:.2}x");
        println!("baseline faster:   {smart_slower}");
    }
    ExitCode::SUCCESS
}
