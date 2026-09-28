import { Router } from "express";
import { z } from "zod";

const NewUser = z.object({ name: z.string().min(1), email: z.email() });
const users = new Map<string, z.infer<typeof NewUser>>();

export const usersRouter = Router();

usersRouter.get("/", (_req, res) => {
  res.json([...users.entries()].map(([id, user]) => ({ id, ...user })));
});

usersRouter.post("/", (req, res) => {
  const parsed = NewUser.safeParse(req.body);
  if (!parsed.success) return res.status(400).json(parsed.error.issues);
  const id = crypto.randomUUID();
  users.set(id, parsed.data);
  res.status(201).json({ id, ...parsed.data });
});
