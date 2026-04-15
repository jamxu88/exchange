import { readJson, runAdminMutation, withAdminSession } from "@/app/api/admin/route-utils";

type StartBotBody = {
  bot_id?: string | null;
};

export async function POST(request: Request) {
  const body = await readJson<StartBotBody>(request);
  const botId = body.bot_id?.trim();
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      botId
        ? `/api/v1/admin/bots/${encodeURIComponent(botId)}/start`
        : "/api/v1/admin/bots/start",
      "POST",
      botId ? `${botId} started.` : "All bots started.",
    ));
}
