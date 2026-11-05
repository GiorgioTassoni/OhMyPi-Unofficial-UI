import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { DesignChoiceInputSchema, PreviewHtmlInputSchema } from "./types.js";
import { startEphemeralChoiceServer } from "./server.js";

const server = new McpServer({
  name: "mcp-design-choice",
  version: "1.0.0",
});

server.tool(
  "design_choice",
  "Present multiple HTML design options to the user for visual comparison. This tool pauses execution until the user selects an option via the local web UI (or via terminal curl command), then returns the user's selected design directly to you in the same turn.",
  DesignChoiceInputSchema.shape,
  async (args) => {
    const choiceServer = await startEphemeralChoiceServer(args);

    const firstId = args.options[0]?.id || "id";
    console.error(`\n🎨 [design-choice] Active server: ${choiceServer.url}`);
    console.error(`👉 Open in browser or OhMyPiApp right panel: ${choiceServer.url}`);
    console.error(`👉 Or choose from terminal: curl -X POST ${choiceServer.url}/api/choose -d '{"id":"${firstId}"}'`);
    console.error(`👉 Or request alternatives: curl -X POST ${choiceServer.url}/api/choose -d '{"action":"request_alternatives","feedback":"Desidero altre opzioni"}'\n`);

    try {
      const choice = await choiceServer.awaitChoice;
      await choiceServer.close();

      if (choice.action === "request_alternatives") {
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify(
                {
                  status: "rejected_request_alternatives",
                  action: "request_alternatives",
                  userFeedback: choice.feedback || "The user found the current options unsatisfactory and requested alternatives.",
                  instruction: `The user rejected all current design options and requested new alternatives. User feedback and requirements: "${choice.feedback || "Please propose new alternative designs."}". Discard current options and generate fresh, distinct HTML design options addressing this feedback.`,
                },
                null,
                2
              ),
            },
          ],
        };
      }

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(
              {
                status: "selected",
                selectedId: choice.selectedId,
                label: choice.label,
                userFeedback: choice.feedback || null,
                instruction: `The user has chosen "${choice.label}" (${choice.selectedId})${
                  choice.feedback ? ` with feedback: "${choice.feedback}"` : ""
                }. Proceed to implement or integrate this chosen design.`,
              },
              null,
              2
            ),
          },
        ],
      };
    } catch (err) {
      await choiceServer.close();
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ status: "error", message: String(err) }),
          },
        ],
        isError: true,
      };
    }
  }
);

server.tool(
  "preview_html",
  "Preview a single HTML document or UI component in the local browser or OhMyPiApp visual panel.",
  PreviewHtmlInputSchema.shape,
  async (args) => {
    const choiceServer = await startEphemeralChoiceServer({
      title: args.title,
      description: args.description,
      options: [
        {
          id: "preview-1",
          label: args.title,
          html: args.html,
        },
        {
          id: "close",
          label: "Close Preview",
          html: "<div style='display:grid;place-items:center;height:100vh;font-family:sans-serif;color:#888;'>Click 'Scegli questa variante' in basso per chiudere la preview.</div>",
        },
      ],
    });

    console.error(`\n👁️ [preview-html] Active server: ${choiceServer.url}\n`);

    try {
      const choice = await choiceServer.awaitChoice;
      await choiceServer.close();

      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({
              status: "reviewed",
              userFeedback: choice.feedback || null,
            }),
          },
        ],
      };
    } catch (err) {
      await choiceServer.close();
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ status: "error", message: String(err) }),
          },
        ],
        isError: true,
      };
    }
  }
);

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error("🚀 [mcp-design-choice] MCP Stdio server connected and ready.");
}

main().catch((err) => {
  console.error("Fatal error starting mcp-design-choice:", err);
  process.exit(1);
});
