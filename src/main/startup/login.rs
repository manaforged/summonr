use std::io::IsTerminal;

use summonr::Result;

use crate::cli::Args;

struct Raw;

impl Raw {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        drop(crossterm::terminal::disable_raw_mode());
    }
}

fn skip_csi() -> Result<()> {
    use std::io::Read as _;
    let mut byte = [0_u8; 1];
    if std::io::stdin().read(&mut byte)? == 0 || byte[0] != b'[' {
        return Ok(());
    }
    loop {
        if std::io::stdin().read(&mut byte)? == 0 {
            return Ok(());
        }
        if (0x40..=0x7e).contains(&byte[0]) {
            return Ok(());
        }
    }
}

fn read_line(prompt: &str) -> Option<String> {
    use std::io::Write as _;
    eprint!("{prompt}: ");
    std::io::stderr().flush().ok()?;
    let mut typed = String::new();
    std::io::stdin().read_line(&mut typed).ok()?;
    Some(typed)
}

pub(super) fn read_secret(prompt: &str) -> Result<String> {
    use std::io::Write as _;
    if !std::io::stdin().is_terminal() {
        let mut key = String::new();
        std::io::stdin().read_line(&mut key)?;
        return Ok(key);
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let raw = Raw::enter()?;
    let mut key = String::new();
    let outcome = masked(&mut key)?;
    drop(raw);
    eprintln!();
    outcome.map(|()| key)
}

fn masked(key: &mut String) -> Result<Result<()>> {
    use std::io::Read as _;
    let mut byte = [0_u8; 1];
    loop {
        if std::io::stdin().read(&mut byte)? == 0 {
            return Ok(Ok(()));
        }
        match byte[0] {
            b'\r' | b'\n' => return Ok(Ok(())),
            0x03 | 0x04 => return Ok(Err(summonr::Error::Config("cancelled".to_string()))),
            0x7f | 0x08 => {
                key.pop();
            }
            0x1b => skip_csi()?,
            other if (32..127).contains(&other) => {
                key.push(other as char);
            }
            _ => {}
        }
    }
}

pub(super) fn print_login_catalog() -> Result<()> {
    let (rows, errors): (Vec<_>, Vec<_>) = summonr::endpoint_names()?
        .into_iter()
        .map(|name| summonr::auth_info(&name))
        .partition(Result::is_ok);
    let mut errors: Vec<String> = errors
        .into_iter()
        .filter_map(Result::err)
        .map(|error| error.to_string())
        .collect();
    errors.dedup();
    errors.iter().for_each(|error| eprintln!("{error}"));
    let ready = summonr::endpoint_set_up();
    let mut rows: Vec<summonr::AuthInfo> = rows.into_iter().flatten().collect();
    rows.sort_by_key(|row| {
        (
            row.catalog_rank(),
            !ready.contains(&row.name),
            row.name.clone(),
        )
    });
    let name = rows.iter().map(|row| row.name.len()).max().unwrap_or(0);
    let state = rows
        .iter()
        .map(|row| row.state().chars().count())
        .max()
        .unwrap_or(0);
    let mut heading = "";
    for row in &rows {
        if row.catalog_title() != heading {
            heading = row.catalog_title();
            println!("\n{heading}");
        }
        println!("  {}", row.catalog_line(name, state));
    }
    println!(
        "
summonr login NAME                         sign in or store a key
summonr login NAME --key                   skip the question and store a key
summonr login NAME --default [--model M]   start on NAME when no endpoint is named
summonr login NAME --url URL [--model M]   add or update your own OpenAI-compatible server
summonr login NAME --model M               change the model of a server you added
summonr logout NAME --remove               remove a server you added"
    );
    Ok(())
}

fn keyless(name: &str) -> Result<Option<String>> {
    let info = summonr::auth_info(name)?;
    if !info.keyless() || info.login.signs_in() {
        return Ok(None);
    }
    let url = info.base_url.unwrap_or_default();
    Ok(Some(if info.model.is_empty() {
        format!("{name} needs no key. Pick a model: `summonr {name} --model NAME` ({url})")
    } else {
        format!("{name} needs no key and is ready: {} at {url}", info.model)
    }))
}

pub(super) async fn run_login(args: &Args) -> Result<()> {
    if args.endpoint.is_none() {
        return print_login_catalog();
    }
    let name = args.endpoint.as_deref().unwrap_or_default();
    if !args.prompt.is_empty() {
        return Err(summonr::Error::Config(
            "do not put credentials on the command line; `summonr login NAME` asks for them"
                .to_string(),
        ));
    }
    if !args.key
        && let Some(text) = keyless(name)?
    {
        println!("{text}");
        return Ok(());
    }
    if key_login(name, args.key)? {
        let key = read_secret(&format!("key for {name}: "))?;
        let source = summonr::check_key(name, key.trim()).await?;
        println!("key for {name} stored in {}", source.label());
        return Ok(());
    }
    let endpoint = summonr::login_seat(name)?;
    let flow = summonr::auth_info(name)?.login;
    match flow {
        summonr::LoginFlow::Browser => {
            println!("signing in to {} through the browser", endpoint.name)
        }
        _ => println!("signing in to {}", endpoint.name),
    }
    let mut prompt = summonr_auth::Prompt::new(
        |line| println!("{line}"),
        |line| {
            Box::pin(async move {
                tokio::task::spawn_blocking(move || read_line(&line))
                    .await
                    .ok()
                    .flatten()
            })
        },
    );
    summonr::Client::login(&endpoint, &mut prompt).await?;
    println!("signed in · {}", endpoint.name);
    Ok(())
}

fn key_login(name: &str, force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }
    let info = summonr::auth_info(name)?;
    if !info.dual() {
        return Ok(!info.login.signs_in());
    }
    eprintln!("1  sign in with {}", info.plan);
    eprintln!("2  paste an API key");
    let picked = read_line("1 or 2").unwrap_or_default();
    Ok(picked.trim() == "2")
}
