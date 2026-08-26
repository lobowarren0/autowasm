import { Hono } from "hono";

const app = new Hono();

app.get("/hello", (c) => {
  return c.json({ message: "hello" });
});

app.get("/health", (c) => {
  return c.json({ status: "ok" });
});

app.get("/external", async (c) => {
  const response = await fetch("https://example.com");
  return c.json(await response.json());
});

export default app;