use super::*;

const NAME: &str = "cap-proof";

fn running() -> usize {
    live().iter().filter(|server| server.name == NAME).count()
}

#[tokio::test(flavor = "multi_thread")]
async fn servers_held_while_they_register_are_capped_once_released() {
    let cap = crate::limits::limits().servers_max;
    let slots: Vec<Slot<u8>> = (0..=cap)
        .map(|_| Arc::new(tokio::sync::Mutex::new(Some(0))))
        .collect();
    let mut guards = Vec::new();
    for slot in &slots {
        guards.push(slot.clone().lock_owned().await);
        register(ServerKind::Mcp, NAME, None, None, slot);
    }
    drop(guards);
    for _ in 0..50 {
        if running() <= cap {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!(
        "{} keep-alive servers still run past a cap of {cap}",
        running()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_released_while_the_rest_are_busy_is_stopped_at_the_cap() {
    const EXTRA: &str = "busy-proof-extra";
    let cap = crate::limits::limits().servers_max;
    let busy: Vec<Slot<u8>> = (0..cap)
        .map(|_| Arc::new(tokio::sync::Mutex::new(Some(0))))
        .collect();
    let mut guards = Vec::new();
    for slot in &busy {
        guards.push(slot.clone().lock_owned().await);
        register(ServerKind::Mcp, "busy-proof", None, None, slot);
    }
    let extra: Slot<u8> = Arc::new(tokio::sync::Mutex::new(Some(0)));
    let held = extra.clone().lock_owned().await;
    register(ServerKind::Mcp, EXTRA, None, None, &extra);
    drop(held);
    let stopped = || !live().iter().any(|server| server.name == EXTRA);
    for _ in 0..50 {
        if stopped() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(stopped(), "the idle server stayed past a cap of {cap}");
    drop(guards);
}
