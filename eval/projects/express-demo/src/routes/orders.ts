import { Router } from "express";

export const ordersRouter = Router();

ordersRouter.get("/:userId", (req, res) => {
  res.json({ userId: req.params.userId, orders: [] });
});
