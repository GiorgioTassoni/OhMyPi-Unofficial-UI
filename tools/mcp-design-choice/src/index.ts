import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { DesignChoiceInputSchema, PreviewHtmlInputSchema } from "./types.js";
import {
  startEphemeralChoiceServer,
  startEphemeralPreviewServer,
  type EphemeralPreviewServer,
  type EphemeralServer,
} from "./server.js";

const server = new McpServer({
  name: "mcp-design-choice",
  version: "1.0.0",
});

let activeChoiceServer: EphemeralServer | null = null;
let activePreviewServer: EphemeralPreviewServer | null = null;

server.tool(
  "design_choice",
  "Present multiple HTML design options to the user for interactive visual comparison in the browser or OhMyPiApp visual panel. Pauses execution until the user selects an option or requests alternatives via the web UI, then returns the user's chosen design and feedback. Communicate with the user in English.",
  DesignChoiceInputSchema.shape,
  async (args) => {
    if (activeChoiceServer) {
      await activeChoiceServer.close().catch(() => {});
      activeChoiceServer = null;
    }

    const choiceServer = await startEphemeralChoiceServer(args);
    activeChoiceServer = choiceServer;

    const firstId = args.options[0]?.id || "id";
    console.error(`\n🎨 [design-choice] Active server: ${choiceServer.url}`);
    console.error(`👉 Open in browser or OhMyPiApp visual panel: ${choiceServer.url}`);
    console.error(`👉 Or choose from terminal: curl -X POST ${choiceServer.url}/api/choose -d '{"id":"${firstId}"}'`);
    console.error(`👉 Or request alternatives: curl -X POST ${choiceServer.url}/api/choose -d '{"action":"request_alternatives","feedback":"I want other options"}'\n`);

    try {
      const choice = await choiceServer.awaitChoice;

      if (choice.action === "request_alternatives") {
        return {
          content: [
            {
              type: "text",
              text: JSON.stringify(
                {
                  status: "rejected_request_alternatives",
                  action: "request_alternatives",
                  userFeedback: choice.feedback || "The user requested alternative options.",
                  instruction: `The user rejected all current design options and requested new alternatives. User feedback and requirements: "${choice.feedback || "Please propose new alternative designs."}". Discard the current options, generate fresh distinct HTML design options addressing this feedback, and communicate in English.`,
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
                instruction: `The user has chosen option "${choice.label}" (${choice.selectedId})${
                  choice.feedback ? ` with feedback: "${choice.feedback}"` : ""
                }. Proceed to implement or integrate this chosen design into the project. Communicate in English.`,
              },
              null,
              2
            ),
          },
        ],
      };
    } catch (err) {
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
  "Preview a live interactive HTML document or UI component in the local browser or OhMyPiApp visual panel. Communicate in English.",
  PreviewHtmlInputSchema.shape,
  async (args) => {
    if (activePreviewServer) {
      await activePreviewServer.close().catch(() => {});
      activePreviewServer = null;
    }

    const previewServer = await startEphemeralPreviewServer(
      args.title,
      args.html,
      args.description
    );
    activePreviewServer = previewServer;

    console.error(`\n👁️ [preview-html] Active server: ${previewServer.url}`);
    console.error(`👉 Open in browser or OhMyPiApp visual panel: ${previewServer.url}\n`);

    return {
      content: [
        {
          type: "text",
          text: JSON.stringify(
            {
              status: "ready",
              url: previewServer.url,
              title: args.title,
              message: `Live preview ready at ${previewServer.url}. Open this URL to view the live preview directly in OhMyPiApp.`,
            },
            null,
            2
          ),
        },
      ],
    };
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
