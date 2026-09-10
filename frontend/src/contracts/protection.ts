export interface ProtectionPlan {
  reserves: Record<string, number>;
  goal: string | null;
}

export interface ProtectionState {
  plan: ProtectionPlan;
  snapshot_id: number | null;
  items: Record<string, { owned: number; protected: number; estimated: number | null; listed: number | null; available: number | null }>;
  issues: string[];
}

export interface ProtectionInventory {
  snapshot_id: number | null;
  items: Record<string, { count: number; leveled: number }>;
}
