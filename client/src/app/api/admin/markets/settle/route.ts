import { readJson, runAdminMutation, withAdminSession } from "@/app/api/admin/route-utils";

type SettleMarketBody = {
  market_id: string;
  settlement_price: number;
  announcement: string | null;
};

export async function POST(request: Request) {
  const body = await readJson<SettleMarketBody>(request);
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      `/api/v1/admin/markets/${encodeURIComponent(body.market_id)}/settle`,
      "POST",
      `${body.market_id} settled.`,
      {
        settlement_price: body.settlement_price,
        announcement: body.announcement,
      },
    ));
}
