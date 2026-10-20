const PIECES: [&[u8]; 14] = [
    b"{",
    b"}",
    b"[",
    b"]",
    b"\"",
    b":",
    b",",
    b"\n",
    b"\r\n",
    b"data: ",
    b"\"jsonrpc\":\"2.0\"",
    b"\"id\":1",
    b"{\"kind\":\"user\",\"text\":\"go\"}\n",
    b"\xff\xfe",
];

pub(crate) fn inputs(seed: u64, count: usize) -> Vec<Vec<u8>> {
    let mut rng = fastrand::Rng::with_seed(seed);
    (0..count)
        .map(|_| {
            let mut bytes = Vec::new();
            for _ in 0..rng.usize(0..48) {
                if rng.bool() {
                    bytes.extend_from_slice(PIECES[rng.usize(..PIECES.len())]);
                } else {
                    bytes.push(rng.u8(..));
                }
            }
            bytes
        })
        .collect()
}

#[test]
fn an_rpc_frame_of_garbage_is_an_error_not_a_panic() {
    for input in inputs(0x5eed_0001, 20_000) {
        drop(crate::rpc::parse(&String::from_utf8_lossy(&input)));
    }
}

#[test]
fn a_journal_of_garbage_reads_without_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    for input in inputs(0x5eed_0002, 2_000) {
        std::fs::write(&path, &input).unwrap();
        drop(crate::durable::log::read(&path));
    }
}
