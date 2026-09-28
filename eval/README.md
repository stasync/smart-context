# Eval set

Saved context packs with expectations, and the runner that checks them. The format and the runner command are in [docs/PLAN.md, section 13.2](../docs/PLAN.md#132-replay-and-eval). Cases arrive in M6.

`projects/` holds small projects to point at. `projects/express-demo` is the plan's "express example": point at `"express"` in its `package.json` and the answer should say it serves the REST API from `src/server.ts`, with routes in `src/routes/`. Its `.env` is fake and must never be read.
