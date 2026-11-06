import { describe, expect, test } from "bun:test";
import { startEphemeralChoiceServer, startEphemeralPreviewServer } from "./server";

describe("mcp design choice ephemeral server", () => {
  const sampleData = {
    title: "Test Navbar Layout",
    options: [
      { id: "opt-1", label: "Minimalist", html: "<nav>Home</nav>" },
      { id: "opt-2", label: "Expanded", html: "<nav>Home | About | Contact</nav><script>console.log('test');</script>" },
    ],
  };

  test("renders choice page with alternative request button and modal form", async () => {
    const srv = await startEphemeralChoiceServer(sampleData);
    const res = await fetch(srv.url);
    expect(res.status).toBe(200);
    const html = await res.text();
    expect(html).toContain("Nessuna mi convince");
    expect(html).toContain("rejectModal");
    expect(html).toContain("rejectFeedbackInput");
    expect(html).not.toContain("</script>;");

    await fetch(`${srv.url}/api/choose`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ id: "opt-1" }),
    });
    const choice = await srv.awaitChoice;
    expect(choice.selectedId).toBe("opt-1");
    await srv.close();
  });

  test("handles request_alternatives action with user feedback", async () => {
    const srv = await startEphemeralChoiceServer(sampleData);
    const userFeedback = "Le opzioni sono troppo semplici, vorrei una sidebar moderna";
    const res = await fetch(`${srv.url}/api/choose`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        action: "request_alternatives",
        id: "none",
        feedback: userFeedback,
      }),
    });
    expect(res.status).toBe(200);
    const body = await res.json();
    expect(body.status).toBe("ok");
    expect(body.selected.action).toBe("request_alternatives");
    expect(body.selected.feedback).toBe(userFeedback);

    const choice = await srv.awaitChoice;
    expect(choice.action).toBe("request_alternatives");
    expect(choice.feedback).toBe(userFeedback);
    await srv.close();
  });

  test("starts standalone preview server immediately without blocking", async () => {
    const previewSrv = await startEphemeralPreviewServer("Button Preview", "<button>Click Me</button>", "Single button demo");
    expect(previewSrv.url).toStartWith("http://127.0.0.1:");

    const pageRes = await fetch(previewSrv.url);
    expect(pageRes.status).toBe(200);
    const pageHtml = await pageRes.text();
    expect(pageHtml).toContain("Button Preview");
    expect(pageHtml).toContain("<iframe");

    const renderRes = await fetch(`${previewSrv.url}/render`);
    expect(renderRes.status).toBe(200);
    const renderHtml = await renderRes.text();
    expect(renderHtml).toContain("<button>Click Me</button>");

    await previewSrv.close();
  });
});
