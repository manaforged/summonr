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
