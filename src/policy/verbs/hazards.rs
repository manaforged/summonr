use super::{Cond, Rule, SubRule};

pub(crate) const INQUIRY_FLAGS: &[&str] = &["--version", "--help"];

pub(super) const IN_PLACE: &[Cond] = &[Cond::Prefix(&["-i", "--in-place"])];

pub(super) const RM: &[Rule] = &[Rule {
    when: &[
        Cond::Any(&["--recursive"]),
        Cond::Short('r'),
        Cond::Short('R'),
    ],
    broad: true,
    reason: "it deletes a system, home, or project root recursively",
}];
const DESTRUCTIVE: &[&str] = &["rm", "rmdir", "unlink", "shred", "truncate"];
pub(super) const FIND: &[Rule] = &[Rule {
    when: &[
        Cond::Any(&["-delete"]),
        Cond::Runs(&["-exec", "-execdir", "-ok", "-okdir"], DESTRUCTIVE),
    ],
    broad: true,
    reason: "it deletes files across a system, home, or project root",
}];
pub(super) const SHRED: &[Rule] = &[Rule {
    when: &[Cond::Always],
    broad: false,
    reason: "it destroys file contents",
}];
pub(super) const DESTROY: &[Rule] = &[Rule {
    when: &[Cond::Any(&["destroy"])],
    broad: false,
    reason: "it destroys infrastructure",
}];
pub(super) const CLUSTER: &[Rule] = &[Rule {
    when: &[Cond::First("delete")],
    broad: false,
    reason: "it deletes cluster resources",
}];
pub(super) const PRUNE: &[Rule] = &[Rule {
    when: &[Cond::Any(&["prune"])],
    broad: false,
    reason: "it deletes container data",
}];

pub(super) const GIT: &[SubRule] = &[
    SubRule {
        names: &["reset"],
        when: &[Cond::Any(&["--hard"])],
        reason: "it discards uncommitted changes",
    },
    SubRule {
        names: &["clean"],
        when: &[Cond::Any(&["--force"]), Cond::Short('f')],
        reason: "it deletes untracked files",
    },
    SubRule {
        names: &["checkout", "restore"],
        when: &[Cond::Any(&["."])],
        reason: "it discards uncommitted changes",
    },
    SubRule {
        names: &["branch"],
        when: &[Cond::Any(&["-D"]), Cond::All(&["--delete", "--force"])],
        reason: "it force-deletes a branch",
    },
    SubRule {
        names: &["push"],
        when: &[
            Cond::Any(&[
                "--force",
                "-f",
                "--force-with-lease",
                "--delete",
                "--mirror",
            ]),
            Cond::Prefix(&["+", ":"]),
        ],
        reason: "it rewrites or deletes remote history",
    },
    SubRule {
        names: &["stash"],
        when: &[Cond::Any(&["drop", "clear"])],
        reason: "it deletes stashed work",
    },
    SubRule {
        names: &["filter-branch", "filter-repo"],
        when: &[Cond::Always],
        reason: "it rewrites history",
    },
];

pub(super) const GIT_READS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "branch",
    "ls-files",
    "blame",
    "rev-parse",
];

pub(super) const FIND_READ: &[Cond] = &[Cond::Prefix(&["-delete", "-exec", "-ok"])];
pub(super) const FIND_WRITE: &[Cond] = &[Cond::Any(&["-delete", "-exec", "-execdir"])];

pub(crate) const TEXT_HAZARDS: &[(&str, &str)] = &[
    ("drop database", "it drops database objects"),
    ("drop table", "it drops database objects"),
];
