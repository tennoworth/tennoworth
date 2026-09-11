export interface UsagePreferences { enabled: boolean; available: boolean }
export interface UsageDay { date: string; count: number; complete: boolean }
export interface UsageDaily { updated_at: string; days: UsageDay[] }

export function parseUsageDaily(value: unknown): UsageDaily {
  if (!value || typeof value !== 'object') throw new Error('Usage counts are unavailable.');
  const data = value as UsageDaily;
  if (typeof data.updated_at !== 'string' || !Number.isFinite(Date.parse(data.updated_at)) || !Array.isArray(data.days) || data.days.length > 90) throw new Error('Usage counts are unavailable.');
  let previous = '';
  for (const day of data.days) {
    if (!day || typeof day.date !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(day.date)
      || !Number.isFinite(Date.parse(day.date)) || new Date(day.date).toISOString().slice(0, 10) !== day.date
      || day.date <= previous || !Number.isSafeInteger(day.count) || day.count < 0 || typeof day.complete !== 'boolean') throw new Error('Usage counts are unavailable.');
    previous = day.date;
  }
  return data;
}
