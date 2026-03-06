export type UserProfile = {
  user_id: number;
  timezone: string;
  reminder_window_start: string | null;
  reminder_window_end: string | null;
  quiet_hours_start: string | null;
  quiet_hours_end: string | null;
  onboarding_state: string;
};

export type Goal = {
  id: string;
  title: string;
  why: string | null;
  metric: string | null;
  target_kind: string;
  target_value: number | null;
  target_text: string | null;
  deadline: string | null;
  cadence: string | null;
  tags: string[];
  status: string;
};

export type MoodPoint = {
  date: string;
  happiness: number;
  energy: number;
  stress: number;
  note: string | null;
};

export type WeeklyReviewStats = {
  user_id: number;
  week: string;
  mood_days: number;
  progress_logs: number;
  active_goals: number;
};

function apiBaseUrl() {
  return process.env.NEXT_PUBLIC_API_BASE_URL || "http://localhost:8080";
}

function devUserId(): string | undefined {
  return process.env.NEXT_PUBLIC_DEV_USER_ID;
}

export async function apiFetch<T>(
  initData: string | undefined,
  path: string,
  init?: RequestInit,
): Promise<T> {
  const headers = new Headers(init?.headers);

  if (initData) {
    headers.set("x-telegram-init-data", initData);
  } else if (devUserId()) {
    headers.set("x-user-id", devUserId()!);
  } else {
    throw new Error(
      "No Telegram initData available and NEXT_PUBLIC_DEV_USER_ID is not set.",
    );
  }

  if (!headers.has("content-type") && init?.body) {
    headers.set("content-type", "application/json");
  }

  const res = await fetch(`${apiBaseUrl()}${path}`, {
    ...init,
    headers,
    cache: "no-store",
  });

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`${res.status} ${res.statusText}${text ? `: ${text}` : ""}`);
  }

  if (res.status === 204) return undefined as T;

  const contentType = res.headers.get("content-type") || "";
  if (contentType.includes("application/json")) return (await res.json()) as T;
  return (await res.text()) as T;
}

