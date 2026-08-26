import { Hono } from "hono";

const app = new Hono();

app.get("/users", (c) => {
  return c.json({ users: [] });
});

app.post("/users", (c) => {
  return c.json({ created: true });
});

app.delete("/users/:id", (c) => {
  return c.json({ deleted: true });
});

export default app;