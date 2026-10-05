import { appendFileSync } from "node:fs";
import { createServer } from "node:http";

const journal = process.env.SENTINAL_NTFY_LOG;
if (!journal) throw new Error("SENTINAL_NTFY_LOG is required");
const entries = [];

createServer((request, response) => {
  if (request.method === "GET" && request.url === "/health") {
    response.writeHead(200).end("ok");
    return;
  }
  if (request.method === "GET" && request.url === "/journal") {
    response.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(entries));
    return;
  }
  if (request.method !== "POST" || request.url !== "/sentinal-e2e") {
    response.writeHead(404).end("not found");
    return;
  }
  const chunks = [];
  let size = 0;
  request.on("data", (chunk) => {
    size += chunk.length;
    if (size <= 4096) chunks.push(chunk);
  });
  request.on("end", () => {
    if (size > 4096) {
      response.writeHead(413).end("too large");
      return;
    }
    const entry = {
      title: request.headers.title,
      tags: request.headers.tags,
      priority: request.headers.priority,
      click: request.headers.click ?? null,
      tokenConfigured: typeof request.headers.authorization === "string",
      body: Buffer.concat(chunks).toString("utf8"),
    };
    entries.push(entry);
    appendFileSync(journal, `${JSON.stringify(entry)}\n`, { mode: 0o600 });
    response.writeHead(200, { "content-type": "application/json" }).end("{}");
  });
}).listen(7440, "127.0.0.1");
