use super::hazards::{
    CLUSTER, DESTROY, FIND, FIND_READ, FIND_WRITE, GIT, GIT_READS, IN_PLACE, PRUNE, RM, SHRED,
};
use super::{Clip, Cwd, Hazard, Native, Read, Verb, Write};

const BASE: Verb = Verb {
    name: "",
    read: Read::Never,
    search: false,
    network: false,
    write: Write::None,
    cwd: Cwd::Stays,
    native: None,
    clip: None,
    hazard: Hazard::None,
};

const READ: Verb = Verb {
    read: Read::Always,
    ..BASE
};
const SEARCH: Verb = Verb {
    search: true,
    ..BASE
};
const NETWORK: Verb = Verb {
    network: true,
    ..BASE
};
const DESTROYS: Verb = Verb {
    write: Write::All,
    ..BASE
};
const DEST_ONLY: Verb = Verb {
    write: Write::Last,
    ..BASE
};
const CREATES: Verb = Verb {
    write: Write::Creates,
    ..BASE
};

pub(super) const VERBS: &[Verb] = &[
    Verb {
        name: "cat",
        native: Some(Native::Cat),
        ..READ
    },
    Verb { name: "ls", ..READ },
    Verb {
        name: "tree",
        ..READ
    },
    Verb { name: "wc", ..READ },
    Verb {
        name: "head",
        native: Some(Native::Head),
        clip: Some(Clip::Head),
        ..READ
    },
    Verb {
        name: "tail",
        clip: Some(Clip::Tail),
        ..READ
    },
    Verb {
        name: "grep",
        search: true,
        ..READ
    },
    Verb {
        name: "rg",
        search: true,
        ..READ
    },
    Verb {
        name: "echo",
        ..READ
    },
    Verb {
        name: "printf",
        ..READ
    },
    Verb {
        name: "pwd",
        ..READ
    },
    Verb {
        name: "which",
        ..READ
    },
    Verb {
        name: "env",
        ..READ
    },
    Verb {
        name: "printenv",
        ..READ
    },
    Verb {
        name: "type",
        ..READ
    },
    Verb {
        name: "file",
        ..READ
    },
    Verb {
        name: "stat",
        ..READ
    },
    Verb { name: "du", ..READ },
    Verb { name: "df", ..READ },
    Verb {
        name: "date",
        ..READ
    },
    Verb {
        name: "true",
        ..READ
    },
    Verb {
        name: "false",
        ..READ
    },
    Verb {
        name: "test",
        ..READ
    },
    Verb {
        name: "less",
        ..READ
    },
    Verb {
        name: "more",
        ..READ
    },
    Verb {
        name: "sort",
        ..READ
    },
    Verb {
        name: "uniq",
        ..READ
    },
    Verb {
        name: "diff",
        ..READ
    },
    Verb {
        name: "cut",
        ..READ
    },
    Verb {
        name: "awk",
        ..READ
    },
    Verb { name: "jq", ..READ },
    Verb {
        name: "xxd",
        ..READ
    },
    Verb { name: "od", ..READ },
    Verb {
        name: "sed",
        read: Read::Unless(IN_PLACE),
        write: Write::When(IN_PLACE),
        native: Some(Native::Sed),
        ..BASE
    },
    Verb {
        name: "find",
        read: Read::Unless(FIND_READ),
        search: true,
        write: Write::When(FIND_WRITE),
        hazard: Hazard::Args(FIND),
        ..BASE
    },
    Verb {
        name: "git",
        read: Read::Sub(GIT_READS),
        hazard: Hazard::Sub {
            skip: &["-C", "-c"],
            rules: GIT,
        },
        ..BASE
    },
    Verb {
        name: "ag",
        ..SEARCH
    },
    Verb {
        name: "ack",
        ..SEARCH
    },
    Verb {
        name: "fd",
        ..SEARCH
    },
    Verb {
        name: "mdfind",
        search: true,
        ..READ
    },
    Verb {
        name: "locate",
        search: true,
        ..READ
    },
    Verb {
        name: "curl",
        ..NETWORK
    },
    Verb {
        name: "wget",
        ..NETWORK
    },
    Verb {
        name: "ssh",
        ..NETWORK
    },
    Verb {
        name: "scp",
        ..NETWORK
    },
    Verb {
        name: "sftp",
        ..NETWORK
    },
    Verb {
        name: "rm",
        hazard: Hazard::Args(RM),
        ..DESTROYS
    },
    Verb {
        name: "shred",
        hazard: Hazard::Args(SHRED),
        ..DESTROYS
    },
    Verb {
        name: "rmdir",
        ..DESTROYS
    },
    Verb {
        name: "remove-item",
        hazard: Hazard::Args(RM),
        ..DESTROYS
    },
    Verb {
        name: "ri",
        hazard: Hazard::Args(RM),
        ..DESTROYS
    },
    Verb {
        name: "del",
        hazard: Hazard::Args(RM),
        ..DESTROYS
    },
    Verb {
        name: "erase",
        hazard: Hazard::Args(RM),
        ..DESTROYS
    },
    Verb {
        name: "rd",
        hazard: Hazard::Args(RM),
        ..DESTROYS
    },
    Verb {
        name: "unlink",
        ..DESTROYS
    },
    Verb {
        name: "chmod",
        ..DESTROYS
    },
    Verb {
        name: "chown",
        ..DESTROYS
    },
    Verb {
        name: "truncate",
        ..DESTROYS
    },
    Verb {
        name: "tee",
        ..DESTROYS
    },
    Verb {
        name: "dd",
        ..DESTROYS
    },
    Verb {
        name: "mv",
        ..DEST_ONLY
    },
    Verb {
        name: "cp",
        ..DEST_ONLY
    },
    Verb {
        name: "ln",
        ..DEST_ONLY
    },
    Verb {
        name: "install",
        ..DEST_ONLY
    },
    Verb {
        name: "rsync",
        ..DEST_ONLY
    },
    Verb {
        name: "mkdir",
        ..CREATES
    },
    Verb {
        name: "touch",
        ..CREATES
    },
    Verb {
        name: "md",
        ..CREATES
    },
    Verb {
        name: "cd",
        cwd: Cwd::Tracked,
        ..BASE
    },
    Verb {
        name: "pushd",
        cwd: Cwd::Changes,
        ..BASE
    },
    Verb {
        name: "terraform",
        hazard: Hazard::Args(DESTROY),
        ..BASE
    },
    Verb {
        name: "tofu",
        hazard: Hazard::Args(DESTROY),
        ..BASE
    },
    Verb {
        name: "kubectl",
        hazard: Hazard::Args(CLUSTER),
        ..BASE
    },
    Verb {
        name: "docker",
        hazard: Hazard::Args(PRUNE),
        ..BASE
    },
    Verb {
        name: "podman",
        hazard: Hazard::Args(PRUNE),
        ..BASE
    },
];
