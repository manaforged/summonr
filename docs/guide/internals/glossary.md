# Domain glossary

- **Built-in modules**: Compiled modules and harness jobs inside the `summonr` binary.
  Curated. A project may disable a module, never enable a new one. RSS and
  prefix budget live here.
- **Module**: A named, switchable capability compiled into the binary
  (`src/modules.rs`). Not a user package.
- **Language module**: A curated, compiled-in description of a language family's
  project markers, source files, language server choices, and initialization
  policy.
- **User extra**: Skill, MCP server, hook, or CLI. Files and child processes.
  Other harnesses say plugin; do not load guest code in-process. Avoid as
  product names: plugin host, extension, marketplace.
- **Work**: Run a plan's `## Steps` one at a time, each gated on its check.
  Entered from `/steps` or `--steps`.
- **Goal**: Loop on an objective with no written plan until checks pass
  and a judge says done.
