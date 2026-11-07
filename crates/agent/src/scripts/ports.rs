use std::collections::BTreeSet;
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

use summonr_core::error::{Error, Result};

use crate::limits::limits;
use crate::session::sidecar::Sidecar;

const FILE: &str = "ports.json";

pub(super) fn block(home: &Path, workspace: &Path) -> Result<u16> {
    let key = workspace.to_string_lossy().into_owned();
    let mut ports = Sidecar::<u16>::at(home.join(FILE));
    let live: Vec<String> = ports
        .entries()
        .map(|(path, _)| path)
        .filter(|path| *path == key || Path::new(path).is_dir())
        .map(str::to_string)
        .collect();
    let port = match ports.get(&key) {
        Some(&port) => port,
        None => {
            let taken = live.iter().filter_map(|path| ports.get(path).copied());
            let port = free(&taken.collect())?;
            ports.put(&key, port);
            port
        }
    };
    ports.save(live.iter().map(String::as_str).chain([key.as_str()]));
    Ok(port)
}

fn free(taken: &BTreeSet<u16>) -> Result<u16> {
    let base = limits().port_base;
    let size = limits().port_block.max(1);
    let last = u32::from(u16::MAX) + 1 - u32::from(size);
    (u32::from(base)..=last)
        .step_by(usize::from(size))
        .filter_map(|port| u16::try_from(port).ok())
        .find(|port| {
            !taken.contains(port) && TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).is_ok()
        })
        .ok_or_else(|| Error::Config(format!("no free block of {size} ports from {base}")))
}
