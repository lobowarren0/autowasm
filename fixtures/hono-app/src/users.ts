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

app.get("/users/:id/details", (c) => {
  return c.json({ id: c.req.param("id") });
});

export default app;