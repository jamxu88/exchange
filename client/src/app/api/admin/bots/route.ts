import { readJson, runAdminMutation, withAdminSession } from "@/app/api/admin/route-utils";

type DeleteBotBody = {
  bot_id?: string | null;
};

export async function POST(request: Request) {
  const body = await readJson<Record<string, unknown>>(request);
  const botId = String(body.bot_id ?? "").trim();
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      "/api/v1/admin/bots",
      "POST",
      `${botId} saved.`,
      body,
    ));
}

export async function DELETE(request: Request) {
  const body = await readJson<DeleteBotBody>(request);
  const botId = body.bot_id?.trim();
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      botId
        ? `/api/v1/admin/bots/${encodeURIComponent(botId)}`
        : "/api/v1/admin/bots",
      "DELETE",
      botId ? `${botId} deleted.` : "All bots deleted.",
    ));
}
