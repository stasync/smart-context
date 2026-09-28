import express from "express";
import { ordersRouter } from "./routes/orders.js";
import { usersRouter } from "./routes/users.js";

const app = express();
app.use(express.json());

app.use("/users", usersRouter);
app.use("/orders", ordersRouter);

app.get("/health", (_req, res) => {
  res.json({ ok: true });
});

const port = Number(process.env.PORT ?? 3000);
app.listen(port, () => {
  console.log(`Shop API on http://localhost:${port}`);
});
