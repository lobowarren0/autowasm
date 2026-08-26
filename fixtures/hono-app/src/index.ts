import { Hono } from "hono";

const app = new Hono();

app.get("/hello", (c) => {
  return c.json({ message: "hello" });
});

app.get("/health", (c) => {
  return c.json({ status: "ok" });
});

export default app;