const app = {
  get: (path: string) => path,
  post: (path: string) => path,
  delete: (path: string) => path,
};

app.get("/users");
app.post("/users");
app.delete("/users/:id");