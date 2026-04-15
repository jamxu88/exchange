import { readJson, runAdminMutation, withAdminSession } from "@/app/api/admin/route-utils";

type PauseBotBody = {
  bot_id?: string | null;
};

export async function POST(request: Request) {
  const body = await readJson<PauseBotBody>(request);
  const botId = body.bot_id?.trim();
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      botId
        ? `/api/v1/admin/bots/${encodeURIComponent(botId)}/pause`
        : "/api/v1/admin/bots/pause",
      "POST",
      botId ? `${botId} paused.` : "All bots paused.",
    ));
}
