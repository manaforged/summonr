use std::thread;

pub(crate) fn harness(tests: &[(&'static str, fn())]) {
    let mut filters = Vec::new();
    let mut skips = Vec::new();
    let mut exact = false;
    let mut list = false;
    let mut threads = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--exact" => exact = true,
            "--skip" => skips.extend(args.next()),
            "--test-threads" => threads = args.next(),
            "--color" | "--format" | "-Z" => drop(args.next()),
            flag if flag.starts_with("--test-threads=") => {
                threads = flag.strip_prefix("--test-threads=").map(str::to_string);
            }
            flag if flag.starts_with('-') => {}
            filter => filters.push(filter.to_string()),
        }
    }
    let matches = |name: &str, pattern: &String| {
        if exact {
            name == pattern
        } else {
            name.contains(pattern.as_str())
        }
    };
    let chosen: Vec<_> = tests
        .iter()
        .filter(|(name, _)| {
            filters.is_empty() || filters.iter().any(|filter| matches(name, filter))
        })
        .filter(|(name, _)| !skips.iter().any(|skip| matches(name, skip)))
        .collect();
    if list {
        for (name, _) in &chosen {
            println!("{name}: test");
        }
        return;
    }
    let total = chosen.len();
    let threads = threads
        .or_else(|| std::env::var("RUST_TEST_THREADS").ok())
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&count| count > 0)
        .unwrap_or(total)
        .clamp(1, total.max(1));
    let queue = std::sync::Mutex::new(chosen.into_iter().enumerate());
    let (sender, results) = std::sync::mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..threads {
            let sender = sender.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let next = queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .next();
                    let Some((index, (name, test))) = next else {
                        break;
                    };
                    let passed = thread::spawn(*test).join().is_ok();
                    if sender.send((index, *name, passed)).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(sender);
    let mut outcomes: Vec<_> = results.into_iter().collect();
    outcomes.sort_by_key(|(index, _, _)| *index);
    let mut failed = 0;
    for (_, name, passed) in outcomes {
        println!("test {name} ... {}", if passed { "ok" } else { "FAILED" });
        failed += usize::from(!passed);
    }
    println!(
        "test result: {}. {} passed; {failed} failed; 0 ignored; 0 measured; {} filtered out",
        if failed == 0 { "ok" } else { "FAILED" },
        total - failed,
        tests.len() - total
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
