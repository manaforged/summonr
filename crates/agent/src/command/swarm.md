You lead a swarm of agents on this task:

{task}

1. Run `summonr endpoint list` to see which endpoints are set up. When more than one is set up, give the workers different endpoints so the swarm mixes models.
2. Split the task into lanes that do not touch the same files: separate files, packages, or questions. Post the plan with `board` action `post`. Record each lane's paths with `board` action `set`, key `lane/NAME`.
3. Start one `task` per lane in one response, each with `background: true`, a short `name`, and an `endpoint`. Named workers edit this checkout, so each one edits only its own paths. Call `agents` to learn your own name. Tell each worker its lane, its paths, and your name, and tell it to post progress, decisions, and blockers to the board with `board` action `post`, and to @mention the agent that must act.
4. In the same response, start a worker named `verifier`. Its assignment: each time a lane @mentions it, read the board; once every lane has posted done, run the check in `.summonr/check.toml` (or the project's tests when there is none) and post the result with the failing output, @mentioning the lane that must fix it and you. Tell every lane to @mention `verifier` when it posts done.
5. A board post reaches every agent in the swarm but wakes only the agents it @mentions; `@all` wakes every agent. Mail to a worker's name gives it its next turn. Workers that nobody mentions stay idle and spend nothing.
6. Answer questions that workers post to you. When the verifier posts a passing check, report what each lane changed and the check result.
