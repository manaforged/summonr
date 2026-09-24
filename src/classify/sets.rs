use std::collections::BTreeMap;

use serde_json::json;

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

fn set<const N: usize>(rows: [(&str, Question); N]) -> BTreeMap<String, Question> {
    rows.into_iter()
        .map(|(name, question)| (name.to_string(), question))
        .collect()
}

pub fn errors() -> BTreeMap<String, Question> {
    set([
        (
            "kind",
            Question::choice(
                "Classify the root cause of this failure",
                [
                    (
                        "antibot",
                        json!({
                            "what": "A bot defence stopped the request before the application saw it",
                            "examples": [
                                "a challenge or captcha page",
                                "an Access Denied or Reference # interstitial",
                                "a Cloudflare, Akamai, PerimeterX or DataDome response",
                                "an HTML block page where JSON was expected",
                            ],
                            "not_for": "a named authentication error in a JSON body, which is auth",
                        }),
                    ),
                    (
                        "network",
                        json!({
                            "what": "The request never got a usable reply",
                            "examples": [
                                "connection reset",
                                "timeout",
                                "DNS failure",
                                "TLS handshake failure",
                                "a proxy refusing the connection",
                            ],
                            "not_for": "a reply that arrived and said no, which is auth or stock",
                        }),
                    ),
                    (
                        "auth",
                        json!({
                            "what": "The site understood the request and rejected the credentials",
                            "examples": [
                                "an expired or invalid token",
                                "a signed out session",
                                "a named authentication error code in the body",
                            ],
                            "not_for": "a block page with no application error body, which is antibot",
                        }),
                    ),
                    (
                        "stock",
                        json!({
                            "what": "The site answered normally and the item cannot be bought",
                            "examples": ["out of stock", "unavailable", "a quantity limit"],
                        }),
                    ),
                    (
                        "site_change",
                        json!({
                            "what": "The reply arrived but did not match what the code expects",
                            "examples": [
                                "a missing or renamed field",
                                "a parse failure",
                                "a route that no longer exists",
                            ],
                        }),
                    ),
                    (
                        "our_bug",
                        json!({
                            "what": "The failure happened in our own code and the site is not involved",
                            "examples": ["a panic", "an assertion", "a bad argument"],
                        }),
                    ),
                    (
                        "other",
                        json!({"what": "None of the above fits this failure"}),
                    ),
                ],
            ),
        ),
        (
            "rotate",
            Question::bounded(
                "Sending this same request again from a different proxy could succeed",
                "the failure is tied to this address or this proxy",
                "the failure would repeat from any address",
            ),
        ),
        (
            "resession",
            Question::noul("This failure needs a fresh session or login before another attempt"),
        ),
        (
            "same_again",
            Question::noul(
                "Sending this exact request again, unchanged and from the same session and proxy, could succeed",
            ),
        ),
        (
            "our_side",
            Question::noul(
                "The failure happened inside our own process, such as a panic, an assertion, or a deserialisation error, rather than being reported by the site",
            ),
        ),
    ])
}

pub fn commit() -> BTreeMap<String, Question> {
    set([
        (
            "untested",
            Question::noul("This patch changes no file whose name marks it as a test"),
        ),
        (
            "spans_crates",
            Question::noul(
                "This patch changes files in more than one crate or top level directory",
            ),
        ),
        (
            "mixes_jobs",
            Question::noul(
                "This patch does more than one unrelated job, such as a fix and a rename together",
            ),
        ),
    ])
}

pub fn diff() -> BTreeMap<String, Question> {
    set([
        (
            "kind",
            Question::choice(
                "What kind of change is this patch",
                [
                    ("feature", "It adds behaviour that did not exist"),
                    ("fix", "It corrects behaviour that was wrong"),
                    (
                        "refactor",
                        "It moves or renames code without changing what it does",
                    ),
                    ("test", "It only changes tests or test fixtures"),
                    (
                        "chore",
                        "It only changes documentation, formatting, dependencies or configuration",
                    ),
                ],
            ),
        ),
        (
            "risk",
            Question::score(
                "How closely a reviewer needs to read this patch",
                [
                    "Nothing to check: text, comments, formatting or dependency bumps",
                    "Ordinary code change, read it normally",
                    "Changes control flow, error handling or a shared interface, read every line",
                    "Can lose data, spend money, or let something through that was blocked",
                ],
            ),
        ),
        (
            "adds_panic",
            Question::noul(
                "An added line calls .unwrap(), .expect(...), panic!, unreachable! or array indexing, outside a test",
            ),
        ),
        (
            "removes_guard",
            Question::noul(
                "The removed lines contain more conditions, checks and error branches than the added lines do",
            ),
        ),
        (
            "widens_surface",
            Question::noul(
                "An added line declares a new `pub` function, type, field or variant, or changes the signature of an existing one",
            ),
        ),
        (
            "touches_value",
            Question::noul(
                "The changed lines handle money, prices, quantities, credentials, authentication or order submission",
            ),
        ),
    ])
}

pub fn review() -> BTreeMap<String, Question> {
    set([
        (
            "outcome",
            Question::choice(
                "What happened to the task the session was asked to do",
                [
                    (
                        "finished",
                        "The task was completed and the work is in place",
                    ),
                    (
                        "partial",
                        "Some of the task was completed and the rest was left",
                    ),
                    (
                        "blocked",
                        "The work stopped on something outside the agent's control",
                    ),
                    (
                        "abandoned",
                        "The work stopped without finishing and without a stated reason",
                    ),
                ],
            ),
        ),
        (
            "risk",
            Question::score(
                "How much review the changes in this session need before they ship",
                [
                    "Read-only or trivial, no review needed",
                    "Ordinary change, normal review",
                    "Touches shared or risky code, read it closely",
                    "Destructive or irreversible, check before it lands",
                ],
            ),
        ),
        (
            "proved",
            Question::noul(
                "The session ran a command that checks its own work, such as a build, a test or a lint",
            ),
        ),
        (
            "looped",
            Question::noul("One command ran three or more times in this session"),
        ),
        (
            "overreached",
            Question::noul("The agent changed things the user did not ask it to change"),
        ),
        (
            "unreported",
            Question::noul(
                "The session ended with a failure or a skipped step the agent did not state plainly",
            ),
        ),
    ])
}
