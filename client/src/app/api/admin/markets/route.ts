import { readJson, runAdminMutation, withAdminSession } from "@/app/api/admin/route-utils";

type CreateMarketBody = {
  market_id: string;
  display_name: string;
  base_asset: string;
  quote_asset: string;
  tick_size: number;
  min_order_quantity: number;
  min: number | null;
  max: number | null;
  reference_price: number | null;
  enabled: boolean;
};

type UpdateMarketBody = {
  market_id: string;
  enabled: boolean;
};

type DeleteMarketBody = {
  market_id: string;
};

export async function POST(request: Request) {
  const body = await readJson<CreateMarketBody>(request);
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      "/api/v1/admin/markets",
      "POST",
      `${body.market_id} saved.`,
      body,
    ));
}

export async function PATCH(request: Request) {
  const body = await readJson<UpdateMarketBody>(request);
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      `/api/v1/admin/markets/${encodeURIComponent(body.market_id)}`,
      "PATCH",
      body.enabled ? `${body.market_id} enabled.` : `${body.market_id} disabled.`,
      { enabled: body.enabled },
    ));
}

export async function DELETE(request: Request) {
  const body = await readJson<DeleteMarketBody>(request);
  return withAdminSession(async (apiKey) =>
    await runAdminMutation(
      apiKey,
      `/api/v1/admin/markets/${encodeURIComponent(body.market_id)}`,
      "DELETE",
      `${body.market_id} deleted.`,
    ));
}
