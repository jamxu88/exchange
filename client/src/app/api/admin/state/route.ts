import { getAdminState } from "@/lib/exchange-server";
import { withAdminSession } from "@/app/api/admin/route-utils";

export async function GET() {
  return withAdminSession(async (apiKey) => await getAdminState(apiKey));
}
