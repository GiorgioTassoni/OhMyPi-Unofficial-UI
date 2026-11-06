import http from "node:http";
import type { ChoiceResult, DesignChoiceInput } from "./types.js";
import { renderChoicePage, renderSinglePreviewPage } from "./ui.js";

export interface EphemeralServer {
  url: string;
  port: number;
  close: () => Promise<void>;
  awaitChoice: Promise<ChoiceResult>;
}

export interface EphemeralPreviewServer {
  url: string;
  port: number;
  close: () => Promise<void>;
}

export function startEphemeralPreviewServer(
  title: string,
  rawHtml: string,
  description?: string
): Promise<EphemeralPreviewServer> {
  return new Promise((resolveReady) => {
    const server = http.createServer((req, res) => {
      const url = new URL(req.url || "/", `http://${req.headers.host || "127.0.0.1"}`);

      res.setHeader("Access-Control-Allow-Origin", "*");
      res.setHeader("Access-Control-Allow-Methods", "GET, OPTIONS");
      res.setHeader("Access-Control-Allow-Headers", "Content-Type");

      if (req.method === "OPTIONS") {
        res.writeHead(204);
        res.end();
        return;
      }

      if (req.method === "GET" && url.pathname === "/") {
        const page = renderSinglePreviewPage(title, rawHtml, description);
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        res.end(page);
        return;
      }

      if (req.method === "GET" && (url.pathname === "/render" || url.pathname.startsWith("/render/"))) {
        const baseStyle = "<style>*, *::before, *::after { box-sizing: border-box; } html, body { margin: 0; padding: 0; min-height: 100vh; background-color: #ffffff; color: #111827; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; }</style>";
        const fullDoc = rawHtml.includes("<html")
          ? rawHtml
          : `<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">${baseStyle}</head><body>${rawHtml}</body></html>`;
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        res.end(fullDoc);
        return;
      }

      res.writeHead(404, { "Content-Type": "text/plain" });
      res.end("Not Found");
    });

    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      const url = `http://127.0.0.1:${port}`;

      resolveReady({
        url,
        port,
        close: () =>
          new Promise<void>((done) => {
            server.close(() => done());
          }),
      });
    });
  });
}

export function startEphemeralChoiceServer(data: DesignChoiceInput): Promise<EphemeralServer> {
  return new Promise((resolveReady) => {
    let resolveChoice: (result: ChoiceResult) => void;
    const choicePromise = new Promise<ChoiceResult>((resolve) => {
      resolveChoice = resolve;
    });

    const server = http.createServer((req, res) => {
      const url = new URL(req.url || "/", `http://${req.headers.host || "127.0.0.1"}`);

      res.setHeader("Access-Control-Allow-Origin", "*");
      res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
      res.setHeader("Access-Control-Allow-Headers", "Content-Type");

      if (req.method === "OPTIONS") {
        res.writeHead(204);
        res.end();
        return;
      }

      if (req.method === "GET" && url.pathname === "/") {
        const html = renderChoicePage(data);
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        res.end(html);
        return;
      }

      if (req.method === "GET" && url.pathname.startsWith("/render/")) {
        const id = decodeURIComponent(url.pathname.replace("/render/", ""));
        const isIframe = req.headers["sec-fetch-dest"] === "iframe" || url.searchParams.has("raw");
        if (!isIframe && req.headers.accept?.includes("text/html")) {
          res.writeHead(302, { Location: `/?option=${encodeURIComponent(id)}` });
          res.end();
          return;
        }

        const opt = data.options.find((o) => o.id === id);
        if (!opt) {
          res.writeHead(404, { "Content-Type": "text/plain" });
          res.end("Option not found");
          return;
        }

        const baseStyle = "<style>*, *::before, *::after { box-sizing: border-box; } html, body { margin: 0; padding: 0; min-height: 100vh; background-color: #ffffff; color: #111827; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; }</style>";
        const fullDoc = opt.html.includes("<html")
          ? opt.html
          : `<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">${baseStyle}<style>${opt.css || ""}</style></head><body>${opt.html}</body></html>`;

        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        res.end(fullDoc);
        return;
      }

      if (req.method === "GET" && url.pathname === "/api/options") {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify(data));
        return;
      }

      if (req.method === "POST" && url.pathname === "/api/choose") {
        let body = "";
        req.on("data", (chunk) => {
          body += chunk;
        });
        req.on("end", () => {
          try {
            const parsed = JSON.parse(body);
            const isAlternative = parsed.action === "request_alternatives" || parsed.id === "none";
            const chosen = data.options.find((o) => o.id === parsed.id) || {
              id: parsed.id || "none",
              label: parsed.label || (isAlternative ? "No satisfactory variants" : parsed.id),
            };

            const result: ChoiceResult = {
              action: isAlternative ? "request_alternatives" : "selected",
              selectedId: isAlternative ? "none" : chosen.id,
              label: isAlternative ? "No satisfactory variants" : chosen.label,
              feedback: parsed.feedback || undefined,
            };

            res.writeHead(200, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ status: "ok", selected: result }));

            resolveChoice(result);
          } catch (err) {
            res.writeHead(400, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ error: String(err) }));
          }
        });
        return;
      }

      res.writeHead(404, { "Content-Type": "text/plain" });
      res.end("Not Found");
    });

    server.listen(0, "127.0.0.1", () => {
      const addr = server.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      const url = `http://127.0.0.1:${port}`;

      resolveReady({
        url,
        port,
        close: () =>
          new Promise<void>((done) => {
            server.close(() => done());
          }),
        awaitChoice: choicePromise,
      });
    });
  });
}
