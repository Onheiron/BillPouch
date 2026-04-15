#!/usr/bin/env node
/**
 * BillPouch MCP Server
 *
 * Exposes atomic dev tools to AI agents (Copilot, Claude, etc.) so they can
 * interact with the local BillPouch workspace and Docker environment without
 * needing direct shell access.
 *
 * Tools exposed:
 *   docker_run         — exec a shell command in a running container
 *   bp_cmd             — run `bp <args>` in a container
 *   bp_control         — send a JSON ControlRequest to the daemon socket
 *   compose_up         — docker compose up for a given compose file
 *   compose_down       — docker compose down
 *   list_containers    — list running BillPouch containers
 *   git_commit_push    — git add -A + commit + push in one shot
 *   git_diff           — current working tree diff
 *   read_wiki          — read a page from wiki/
 *   write_spec         — write a feature spec to specs/
 *   read_spec          — read a spec from specs/
 */

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { execSync, exec } from "node:child_process";
import { promisify } from "node:util";
import { readFileSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const execAsync = promisify(exec);

// ── Workspace root (two levels up from mcp/src/) ─────────────────────────────
const __dirname = fileURLToPath(new URL(".", import.meta.url));
const WORKSPACE = resolve(__dirname, "..", "..");

function workspacePath(...parts: string[]): string {
  return join(WORKSPACE, ...parts);
}

// ── Shell helper ──────────────────────────────────────────────────────────────
async function sh(
  cmd: string,
  cwd: string = WORKSPACE
): Promise<{ stdout: string; stderr: string; ok: boolean }> {
  try {
    const { stdout, stderr } = await execAsync(cmd, { cwd, timeout: 120_000 });
    return { stdout: stdout.trim(), stderr: stderr.trim(), ok: true };
  } catch (e: unknown) {
    const err = e as { stdout?: string; stderr?: string; message?: string };
    return {
      stdout: err.stdout?.trim() ?? "",
      stderr: (err.stderr?.trim() ?? err.message ?? String(e)),
      ok: false,
    };
  }
}

function text(content: string) {
  return { content: [{ type: "text" as const, text: content }] };
}

function result(r: { stdout: string; stderr: string; ok: boolean }) {
  const out = [r.stdout, r.stderr].filter(Boolean).join("\n");
  return text(r.ok ? out || "(no output)" : `ERROR:\n${out}`);
}

// ── Tool definitions ──────────────────────────────────────────────────────────

const TOOLS = [
  {
    name: "docker_run",
    description:
      "Execute a shell command inside a running BillPouch Docker container. " +
      "Use this to inspect state, run scripts, or interact with the daemon.",
    inputSchema: {
      type: "object",
      properties: {
        container: {
          type: "string",
          description: "Container name (e.g. 'bp-carlo', 'bp-pouch', 'bp-bill')",
        },
        cmd: {
          type: "string",
          description: "Shell command to run inside the container",
        },
      },
      required: ["container", "cmd"],
    },
  },
  {
    name: "bp_cmd",
    description:
      "Run a `bp` CLI command inside a container. " +
      "Equivalent to: docker exec <container> bp <args>",
    inputSchema: {
      type: "object",
      properties: {
        container: { type: "string", description: "Container name" },
        args: {
          type: "array",
          items: { type: "string" },
          description: "Arguments to pass to bp (e.g. ['status'], ['flock'], ['hatch', 'pouch', '--network', 'public'])",
        },
      },
      required: ["container", "args"],
    },
  },
  {
    name: "bp_control",
    description:
      "Send a raw JSON ControlRequest to the BillPouch daemon socket inside a container " +
      "and return the ControlResponse. Use this for low-level daemon interaction.",
    inputSchema: {
      type: "object",
      properties: {
        container: { type: "string", description: "Container name" },
        request: {
          type: "object",
          description: "ControlRequest object (e.g. {\"cmd\": \"Ping\"} or {\"cmd\": \"Status\"})",
        },
      },
      required: ["container", "request"],
    },
  },
  {
    name: "compose_up",
    description:
      "Start a Docker Compose cluster. Builds images if needed. " +
      "compose_file is relative to the workspace root (e.g. 'docker-compose.playground.yml').",
    inputSchema: {
      type: "object",
      properties: {
        compose_file: { type: "string", description: "Compose file path (relative to workspace root)" },
        detach: {
          type: "boolean",
          description: "Run in detached mode (default: true)",
          default: true,
        },
      },
      required: ["compose_file"],
    },
  },
  {
    name: "compose_down",
    description: "Stop and remove a Docker Compose cluster.",
    inputSchema: {
      type: "object",
      properties: {
        compose_file: { type: "string" },
        volumes: {
          type: "boolean",
          description: "Also remove volumes (default: false)",
          default: false,
        },
      },
      required: ["compose_file"],
    },
  },
  {
    name: "list_containers",
    description: "List running Docker containers whose name contains 'bp-'.",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
    },
  },
  {
    name: "git_commit_push",
    description:
      "Stage ALL changes (git add -A), commit with the given message, and push to origin main. " +
      "Use this instead of separate git commands — it always pushes.",
    inputSchema: {
      type: "object",
      properties: {
        message: { type: "string", description: "Commit message" },
        tag: {
          type: "string",
          description: "Optional semver tag to create and push (e.g. 'v0.3.19')",
        },
      },
      required: ["message"],
    },
  },
  {
    name: "git_diff",
    description: "Return the current git diff (unstaged + staged changes).",
    inputSchema: {
      type: "object",
      properties: {
        staged: {
          type: "boolean",
          description: "Show staged diff only (default: false = show all changes)",
          default: false,
        },
      },
      required: [],
    },
  },
  {
    name: "read_wiki",
    description: "Read a page from the wiki/ directory.",
    inputSchema: {
      type: "object",
      properties: {
        page: {
          type: "string",
          description: "Wiki page filename (e.g. '05-control-protocol.md'). Omit to list all pages.",
        },
      },
      required: [],
    },
  },
  {
    name: "write_spec",
    description:
      "Write a feature specification to specs/<name>.md. " +
      "Specs are committed to git and serve as input to other agents.",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Spec name (no extension, e.g. 'invite-expiry')" },
        content: { type: "string", description: "Markdown content of the spec" },
      },
      required: ["name", "content"],
    },
  },
  {
    name: "read_spec",
    description: "Read a spec from specs/. Omit name to list all specs.",
    inputSchema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Spec name (without .md). Omit to list." },
      },
      required: [],
    },
  },
];

// ── Tool handlers ─────────────────────────────────────────────────────────────

async function handleTool(
  name: string,
  args: Record<string, unknown>
): Promise<{ content: Array<{ type: "text"; text: string }> }> {
  switch (name) {
    // ── docker_run ──────────────────────────────────────────────────────────
    case "docker_run": {
      const { container, cmd } = args as { container: string; cmd: string };
      return result(await sh(`docker exec ${container} bash -c ${JSON.stringify(cmd)}`));
    }

    // ── bp_cmd ──────────────────────────────────────────────────────────────
    case "bp_cmd": {
      const { container, args: bpArgs } = args as { container: string; args: string[] };
      const escaped = bpArgs.map((a) => JSON.stringify(a)).join(" ");
      return result(await sh(`docker exec ${container} bp ${escaped}`));
    }

    // ── bp_control ──────────────────────────────────────────────────────────
    case "bp_control": {
      const { container, request } = args as { container: string; request: object };
      const payload = JSON.stringify(request).replace(/'/g, "'\\''");
      const cmd =
        `docker exec ${container} bash -c ` +
        `"echo '${payload}' | socat -T5 - UNIX-CONNECT:\\${HOME}/.local/share/billpouch/control.sock"`;
      return result(await sh(cmd));
    }

    // ── compose_up ──────────────────────────────────────────────────────────
    case "compose_up": {
      const { compose_file, detach = true } = args as { compose_file: string; detach?: boolean };
      const flag = detach ? "-d" : "";
      return result(await sh(`docker compose -f ${compose_file} up --build ${flag}`));
    }

    // ── compose_down ────────────────────────────────────────────────────────
    case "compose_down": {
      const { compose_file, volumes = false } = args as { compose_file: string; volumes?: boolean };
      const flag = volumes ? "-v" : "";
      return result(await sh(`docker compose -f ${compose_file} down ${flag}`));
    }

    // ── list_containers ─────────────────────────────────────────────────────
    case "list_containers": {
      return result(
        await sh(`docker ps --filter "name=bp-" --format "table {{.Names}}\t{{.Status}}\t{{.Image}}"`)
      );
    }

    // ── git_commit_push ─────────────────────────────────────────────────────
    case "git_commit_push": {
      const { message, tag } = args as { message: string; tag?: string };
      let cmd = `git add -A && git commit -m ${JSON.stringify(message)}`;
      if (tag) {
        cmd += ` && git tag ${tag}`;
        cmd += ` && git push origin main refs/tags/${tag}`;
      } else {
        cmd += ` && git push origin main`;
      }
      return result(await sh(cmd));
    }

    // ── git_diff ────────────────────────────────────────────────────────────
    case "git_diff": {
      const { staged = false } = args as { staged?: boolean };
      const flag = staged ? "--staged" : "HEAD";
      return result(await sh(`git diff ${flag}`));
    }

    // ── read_wiki ───────────────────────────────────────────────────────────
    case "read_wiki": {
      const { page } = args as { page?: string };
      const wikiDir = workspacePath("wiki");
      if (!page) {
        const pages = readdirSync(wikiDir)
          .filter((f) => f.endsWith(".md"))
          .join("\n");
        return text(`Wiki pages:\n${pages}`);
      }
      try {
        const content = readFileSync(join(wikiDir, page), "utf-8");
        return text(content);
      } catch {
        return text(`ERROR: wiki/${page} not found. Use read_wiki without 'page' to list all pages.`);
      }
    }

    // ── write_spec ──────────────────────────────────────────────────────────
    case "write_spec": {
      const { name: specName, content } = args as { name: string; content: string };
      const specsDir = workspacePath("specs");
      mkdirSync(specsDir, { recursive: true });
      const filePath = join(specsDir, `${specName}.md`);
      writeFileSync(filePath, content, "utf-8");
      return text(`Written: specs/${specName}.md (${content.length} chars)`);
    }

    // ── read_spec ───────────────────────────────────────────────────────────
    case "read_spec": {
      const { name: specName } = args as { name?: string };
      const specsDir = workspacePath("specs");
      if (!specName) {
        try {
          const files = readdirSync(specsDir)
            .filter((f) => f.endsWith(".md"))
            .join("\n");
          return text(`Specs:\n${files || "(none yet)"}`);
        } catch {
          return text("No specs/ directory yet.");
        }
      }
      try {
        const content = readFileSync(join(specsDir, `${specName}.md`), "utf-8");
        return text(content);
      } catch {
        return text(`ERROR: specs/${specName}.md not found.`);
      }
    }

    default:
      return text(`ERROR: unknown tool '${name}'`);
  }
}

// ── Server setup ──────────────────────────────────────────────────────────────

const server = new Server(
  { name: "billpouch-mcp", version: "1.0.0" },
  { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => ({ tools: TOOLS }));

server.setRequestHandler(CallToolRequestSchema, async (req) => {
  const { name, arguments: args = {} } = req.params;
  return handleTool(name, args as Record<string, unknown>);
});

// ── Start ─────────────────────────────────────────────────────────────────────

const transport = new StdioServerTransport();
await server.connect(transport);
