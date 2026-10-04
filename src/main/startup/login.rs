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

fn mask_one() -> Result<()> {
    use std::io::Write as _;
    eprint!("*");
    std::io::stderr().flush()?;
    Ok(())
}

fn unmask_one() -> Result<()> {
    use std::io::Write as _;
    eprint!("\x08 \x08");
    std::io::stderr().flush()?;
    Ok(())
}

fn read_line(prompt: &str) -> Option<String> {
    use std::io::Write as _;
    eprint!("{prompt}: ");
    std::io::stderr().flush().ok()?;
    let mut typed = String::new();
    std::io::stdin().read_line(&mut typed).ok()?;
    Some(typed)
}

fn read_secret(prompt: &str) -> Result<String> {
    use std::io::{Read as _, Write as _};
    if !std::io::stdin().is_terminal() {
        let mut key = String::new();
        std::io::stdin().read_line(&mut key)?;
        return Ok(key);
    }
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    let raw = Raw::enter()?;
    let mut key = String::new();
    let mut byte = [0_u8; 1];
    let outcome = loop {
        if std::io::stdin().read(&mut byte)? == 0 {
            break Ok(());
        }
        match byte[0] {
            b'\r' | b'\n' => break Ok(()),
            0x03 | 0x04 => break Err(summonr::Error::Config("cancelled".to_string())),
            0x7f | 0x08 => {
                if key.pop().is_some() {
                    unmask_one()?;
                }
            }
            0x1b => skip_csi()?,
            other if (32..127).contains(&other) => {
                key.push(other as char);
                mask_one()?;
            }
            _ => {}
        }
    };
    drop(raw);
    eprintln!();
    outcome.map(|()| key)
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
    let mut rows: Vec<summonr::AuthInfo> = rows.into_iter().flatten().collect();
    rows.sort_by_key(|row| (row.catalog_rank(), row.name.clone()));
    let mut heading = "";
    for row in &rows {
        if row.catalog_heading() != heading {
            heading = row.catalog_heading();
            if heading == "Coding plans" {
                println!("\n{heading}  (ChatGPT, SuperGrok, Copilot, Kimi, GLM/Z.AI, OpenCode)");
            } else if heading == "Decisions" {
                println!("\n{heading}  (summonr classify / Jev)");
            } else if heading == "Local" {
                println!("\n{heading}  (no key)");
            } else {
                println!("\n{heading}");
            }
        }
        println!("  {}", row.catalog_line());
    }
    println!("\nsummonr login NAME     store a key (or device login)");
    println!("summonr login NAME --key   force the key prompt");
    Ok(())
}

pub(super) async fn run_login(args: &Args) -> Result<()> {
    if args.endpoint.is_none() {
        return print_login_catalog();
    }
    let name = args.endpoint.as_deref().unwrap_or_default();
    if !args.prompt.is_empty() {
        return Err(summonr::Error::Config(
            "do not put credentials on the command line — `summonr login NAME` asks for them"
                .to_string(),
        ));
    }
    if key_login(name, args.key)? {
        let key = read_secret(&format!("key for {name}: "))?;
        let source = summonr::check_key(name, key.trim()).await?;
        println!(
            "key for {name} stored in {} ({} chars)",
            source.label(),
            key.trim().chars().count()
        );
        return Ok(());
    }
    let endpoint = summonr::resolve_endpoint(
        args.endpoint.as_deref(),
        args.model.as_deref(),
        summonr::Need::Seat,
    )?;
    let flow = summonr::auth_info(name)?.flow;
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
    Ok(force || !summonr::auth_info(name)?.flow.signs_in())
}
