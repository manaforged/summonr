use std::collections::BTreeMap;

use super::Question;

pub const NAMES: &[&str] = &["review", "errors", "diff", "commit"];

pub fn by_name(name: &str) -> Option<BTreeMap<String, Question>> {
    match name {
        "review" => Some(review()),
        "errors" => Some(errors()),
        "diff" => Some(diff()),
        "commit" => Some(commit()),
        _ => None,
    }
}

fn noul(instructions: &str) -> Question {
    Question::Noul {
        instructions: instructions.to_string(),
    }
}

pub fn commit() -> BTreeMap<String, Question> {
    BTreeMap::from([
        (
            "untested".to_string(),
            noul("This patch changes no file whose name marks it as a test"),
        ),
        (
            "spans_crates".to_string(),
            noul("This patch changes files in more than one crate or top level directory"),
        ),
        (
            "mixes_jobs".to_string(),
            noul(
                "This patch does more than one unrelated job, such as a fix and a rename together",
            ),
        ),
    ])
}

pub fn diff() -> BTreeMap<String, Question> {
    BTreeMap::from([
        (
            "kind".to_string(),
            Question::Choice {
                instructions: "What kind of change is this patch".to_string(),
                criteria: BTreeMap::from([
                    (
                        "feature".to_string(),
                        "It adds behaviour that did not exist".to_string(),
                    ),
                    (
                        "fix".to_string(),
                        "It corrects behaviour that was wrong".to_string(),
                    ),
                    (
                        "refactor".to_string(),
                        "It moves or renames code without changing what it does".to_string(),
                    ),
                    (
                        "test".to_string(),
                        "It only changes tests or test fixtures".to_string(),
                    ),
                    (
                        "chore".to_string(),
                        "It only changes documentation, formatting, dependencies or configuration"
                            .to_string(),
                    ),
                ]),
            },
        ),
        (
            "risk".to_string(),
            Question::Score {
                instructions: "How closely a reviewer needs to read this patch".to_string(),
                criteria: vec![
                    "Nothing to check: text, comments, formatting or dependency bumps".to_string(),
                    "Ordinary code change, read it normally".to_string(),
                    "Changes control flow, error handling or a shared interface, read every line"
                        .to_string(),
                    "Can lose data, spend money, or let something through that was blocked"
                        .to_string(),
                ],
            },
        ),
        (
            "adds_panic".to_string(),
            noul(
                "An added line calls .unwrap(), .expect(...), panic!, unreachable! or array indexing, outside a test",
            ),
        ),
        (
            "removes_guard".to_string(),
            noul(
                "The removed lines contain more conditions, checks and error branches than the added lines do",
            ),
        ),
        (
            "widens_surface".to_string(),
            noul(
                "An added line declares a new `pub` function, type, field or variant, or changes the signature of an existing one",
            ),
        ),
        (
            "touches_value".to_string(),
            noul(
                "The changed lines handle money, prices, quantities, credentials, authentication or order submission",
            ),
        ),
    ])
}

pub fn errors() -> BTreeMap<String, Question> {
    BTreeMap::from([
        (
            "kind".to_string(),
            Question::Choice {
                instructions: "Classify the root cause of this failure".to_string(),
                criteria: BTreeMap::from([
                    (
                        "antibot".to_string(),
                        "A bot defence blocked or challenged the request: a challenge or captcha page, an Access Denied or Reference # interstitial, a Cloudflare, Akamai, PerimeterX or DataDome response, or a block with no application error body".to_string(),
                    ),
                    (
                        "network".to_string(),
                        "The request never got a usable reply: connection reset, timeout, DNS failure, TLS handshake failure, or a proxy refusing the connection".to_string(),
                    ),
                    (
                        "auth".to_string(),
                        "The site understood the request and rejected the credentials: an expired or invalid token, a signed-out session, or a named authentication error code in the body".to_string(),
                    ),
                    (
                        "stock".to_string(),
                        "The site answered normally and the item cannot be bought: out of stock, unavailable, or a quantity limit".to_string(),
                    ),
                    (
                        "site_change".to_string(),
                        "The reply arrived but did not match what the code expects: a missing or renamed field, a parse failure, or a route that no longer exists".to_string(),
                    ),
                    (
                        "our_bug".to_string(),
                        "The failure happened in our own code before or after the request, and the site is not involved".to_string(),
                    ),
                ]),
            },
        ),
        (
            "rotate".to_string(),
            noul("Sending this same request again from a different proxy could succeed"),
        ),
        (
            "resession".to_string(),
            noul("This failure needs a fresh session or login before another attempt"),
        ),
        (
            "same_again".to_string(),
            noul("Sending this exact request again, unchanged and from the same session and proxy, could succeed"),
        ),
        (
            "our_side".to_string(),
            noul("The failure happened inside our own process, such as a panic, an assertion, or a deserialisation error, rather than being reported by the site"),
        ),
    ])
}

pub fn review() -> BTreeMap<String, Question> {
    BTreeMap::from([
        (
            "outcome".to_string(),
            Question::Choice {
                instructions: "What happened to the task the session was asked to do".to_string(),
                criteria: BTreeMap::from([
                    (
                        "finished".to_string(),
                        "The task was completed and the work is in place".to_string(),
                    ),
                    (
                        "partial".to_string(),
                        "Some of the task was completed and the rest was left".to_string(),
                    ),
                    (
                        "blocked".to_string(),
                        "The work stopped on something outside the agent's control".to_string(),
                    ),
                    (
                        "abandoned".to_string(),
                        "The work stopped without finishing and without a stated reason".to_string(),
                    ),
                ]),
            },
        ),
        (
            "risk".to_string(),
            Question::Score {
                instructions: "How much review the changes in this session need before they ship"
                    .to_string(),
                criteria: vec![
                    "Read-only or trivial, no review needed".to_string(),
                    "Ordinary change, normal review".to_string(),
                    "Touches shared or risky code, read it closely".to_string(),
                    "Destructive or irreversible, check before it lands".to_string(),
                ],
            },
        ),
        (
            "proved".to_string(),
            Question::Noul {
                instructions:
                    "The agent proved its claims by running a command, rather than asserting success"
                        .to_string(),
            },
        ),
        (
            "looped".to_string(),
            Question::Noul {
                instructions: "The agent repeated the same failing action three or more times"
                    .to_string(),
            },
        ),
        (
            "overreached".to_string(),
            Question::Noul {
                instructions: "The agent changed things the user did not ask it to change"
                    .to_string(),
            },
        ),
        (
            "unreported".to_string(),
            Question::Noul {
                instructions: "The session ended with a failure or a skipped step the agent did not state plainly"
                    .to_string(),
            },
        ),
    ])
}
