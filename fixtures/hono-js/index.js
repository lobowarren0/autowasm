import { Hono } from "hono";

const app = new Hono();

app.get("/hello", (c) => {
  return c.json({ message: "hello" });
});

app.get("/health", (c) => {
  return c.json({ status: "ok" });
});

app.get("/users/:id", (c) => {
  return c.json({ id: c.req.param("id") });
});

export default app;
