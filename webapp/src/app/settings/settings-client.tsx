"use client";

import { startTransition, useEffect, useState } from "react";
import { Button, Cell, List, Placeholder, Section, Spinner } from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { UserProfile } from "@/lib/api";
import { apiFetch } from "@/lib/api";

const WEEKDAYS = [
  { value: "MON", label: "Monday" },
  { value: "TUE", label: "Tuesday" },
  { value: "WED", label: "Wednesday" },
  { value: "THU", label: "Thursday" },
  { value: "FRI", label: "Friday" },
  { value: "SAT", label: "Saturday" },
  { value: "SUN", label: "Sunday" },
];

function deviceTimezone() {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

export default function SettingsClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");

  const [timezone, setTimezone] = useState("UTC");
  const [dailyCheckinTime, setDailyCheckinTime] = useState("09:00");
  const [goalUpdateTime, setGoalUpdateTime] = useState("19:00");
  const [weeklyReviewTime, setWeeklyReviewTime] = useState("18:00");
  const [weeklyReviewDay, setWeeklyReviewDay] = useState("SUN");

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    apiFetch<UserProfile>(initData, "/v1/profile")
      .then((nextProfile) => {
        if (cancelled) return;
        setProfile(nextProfile);
        setTimezone(nextProfile.timezone);
        setDailyCheckinTime(nextProfile.reminder_preferences.daily_checkin_time);
        setGoalUpdateTime(nextProfile.reminder_preferences.goal_update_time);
        setWeeklyReviewTime(nextProfile.reminder_preferences.weekly_review_time);
        setWeeklyReviewDay(nextProfile.reminder_preferences.weekly_review_day);
      })
      .catch((nextError) => {
        if (!cancelled) {
          setError(nextError instanceof Error ? nextError.message : String(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [initData]);

  async function saveSettings() {
    setSaveError(null);
    setSaveState("saving");

    try {
      const nextProfile = await apiFetch<UserProfile>(initData, "/v1/profile", {
        method: "POST",
        body: JSON.stringify({
          timezone,
          daily_checkin_time: dailyCheckinTime,
          goal_update_time: goalUpdateTime,
          weekly_review_time: weeklyReviewTime,
          weekly_review_day: weeklyReviewDay,
        }),
      });

      startTransition(() => {
        setProfile(nextProfile);
        setTimezone(nextProfile.timezone);
        setDailyCheckinTime(nextProfile.reminder_preferences.daily_checkin_time);
        setGoalUpdateTime(nextProfile.reminder_preferences.goal_update_time);
        setWeeklyReviewTime(nextProfile.reminder_preferences.weekly_review_time);
        setWeeklyReviewDay(nextProfile.reminder_preferences.weekly_review_day);
        setSaveState("saved");
      });
    } catch (nextError) {
      setSaveError(nextError instanceof Error ? nextError.message : String(nextError));
      setSaveState("idle");
      return;
    }

    window.setTimeout(() => {
      setSaveState("idle");
    }, 1800);
  }

  if (loading) {
    return (
      <Placeholder header="Loading settings" description={<Spinner size="l" />}>
        <div />
      </Placeholder>
    );
  }

  if (error) {
    return (
      <Placeholder header="API Error" description={error}>
        <div />
      </Placeholder>
    );
  }

  return (
    <div className="space-y-4">
      <Section header="Messaging cadence">
        <div className="settings-card">
          <p className="settings-copy">
            Goal creation and mood check-ins stay in chat. The mini app only controls when Happi
            nudges you for updates.
          </p>

          <label className="settings-field">
            <span className="settings-label">Timezone</span>
            <input
              className="settings-input"
              value={timezone}
              onChange={(event) => setTimezone(event.target.value)}
              placeholder="Europe/Berlin"
              autoCapitalize="none"
              autoCorrect="off"
            />
          </label>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <label className="settings-field">
              <span className="settings-label">Daily mood prompt</span>
              <input
                className="settings-input"
                type="time"
                value={dailyCheckinTime}
                onChange={(event) => setDailyCheckinTime(event.target.value)}
              />
            </label>

            <label className="settings-field">
              <span className="settings-label">Progress nudge</span>
              <input
                className="settings-input"
                type="time"
                value={goalUpdateTime}
                onChange={(event) => setGoalUpdateTime(event.target.value)}
              />
            </label>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-[1.2fr_0.8fr]">
            <label className="settings-field">
              <span className="settings-label">Weekly review time</span>
              <input
                className="settings-input"
                type="time"
                value={weeklyReviewTime}
                onChange={(event) => setWeeklyReviewTime(event.target.value)}
              />
            </label>

            <label className="settings-field">
              <span className="settings-label">Review day</span>
              <select
                className="settings-input"
                value={weeklyReviewDay}
                onChange={(event) => setWeeklyReviewDay(event.target.value)}
              >
                {WEEKDAYS.map((day) => (
                  <option key={day.value} value={day.value}>
                    {day.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="flex gap-2">
            <Button size="m" onClick={() => setTimezone(deviceTimezone())}>
              Use device timezone
            </Button>
            <Button size="m" stretched onClick={saveSettings} disabled={saveState === "saving"}>
              {saveState === "saving"
                ? "Saving..."
                : saveState === "saved"
                  ? "Saved"
                  : "Save reminder times"}
            </Button>
          </div>

          {saveError ? <p className="settings-error">{saveError}</p> : null}
        </div>
      </Section>

      <Section header="Current profile">
        <List>
          <Cell subtitle="Reminder window summary">
            {profile?.reminder_window_start ?? "--:--"} to {profile?.reminder_window_end ?? "--:--"}
          </Cell>
          <Cell subtitle="Onboarding state">{profile?.onboarding_state ?? "new"}</Cell>
        </List>
      </Section>
    </div>
  );
}
