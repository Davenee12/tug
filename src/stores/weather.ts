import { defineStore } from "pinia";
import { ref } from "vue";
import { api, errorMessage } from "../lib/ipc";
import { defaultUnit, fetchForecast, isStale, nameFor, REFRESH_MS, type Forecast, type Place, type Unit } from "../lib/weather";

const CACHE_KEY = "tug.weather.v1";
/** Shown only until the place's real name comes back ("Portland, Oregon"). */
const UNNAMED = "Your location";

/**
 * The Feed's weather card. Off until the user picks a place (their location or a city);
 * the choice lives in settings (`ui.weather`), the last forecast in local storage so the
 * card paints instantly on launch.
 */
export const useWeatherStore = defineStore("weather", () => {
  /** null = not set up yet; "off" = the user hid it. */
  const place = ref<Place | "off" | null>(null);
  const unit = ref<Unit>(defaultUnit());
  const forecast = ref<Forecast | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const ready = ref(false);
  let timer: number | undefined;

  function readCache(p: Place) {
    try {
      const c = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null") as { place: Place; forecast: Forecast } | null;
      if (c && c.place.latitude === p.latitude && c.place.longitude === p.longitude) forecast.value = c.forecast;
    } catch {
      /* no cache */
    }
  }

  function writeCache(p: Place, f: Forecast) {
    try {
      localStorage.setItem(CACHE_KEY, JSON.stringify({ place: p, forecast: f }));
    } catch {
      /* storage full or blocked: the card still works */
    }
  }

  /**
   * Called each time the Feed mounts, which is every trip back from Settings. The saved
   * choice is read once: re-reading it each time could land before a change just made in
   * Settings was saved (Weather › Change place snapping back to the old place) and repainted
   * the card from the cache for nothing. Later calls refresh a stale forecast; the clock runs once.
   */
  async function init() {
    if (ready.value) {
      void refresh();
      startClock();
      return;
    }
    try {
      const raw = await api.getSettings();
      const saved = raw["ui.weather"];
      place.value = saved === "off" ? "off" : saved ? (JSON.parse(saved) as Place) : null;
      if (raw["ui.weatherUnit"] === "f" || raw["ui.weatherUnit"] === "c") unit.value = raw["ui.weatherUnit"];
    } catch {
      place.value = null;
    }
    ready.value = true;
    if (place.value && place.value !== "off") readCache(place.value);
    void refresh();
    startClock();
  }

  // Coming back to tug (from the tray or another window) refreshes a forecast that's gone stale:
  // a hidden window's timers can be throttled, so the clock alone isn't enough.
  const onShow = () => {
    if (document.visibilityState === "visible") void refresh();
  };

  /**
   * The refresh clock runs once for the life of the app. Restarting it on every Feed visit (each
   * trip back from Settings) kept pushing the next refresh out, so the numbers rarely changed.
   */
  function startClock() {
    if (timer !== undefined) return;
    timer = window.setInterval(() => void refresh(), REFRESH_MS);
    document.addEventListener("visibilitychange", onShow);
    window.addEventListener("focus", onShow);
  }

  /** A failed name lookup at setup must not stick: try again until the place has a name. */
  async function nameIfNeeded() {
    const p = place.value;
    if (!p || p === "off" || p.name !== UNNAMED) return;
    const name = await nameFor(p.latitude, p.longitude).catch(() => null);
    if (!name || place.value !== p) return;
    place.value = { ...p, name };
    await api.setSetting("ui.weather", JSON.stringify(place.value)).catch(() => undefined);
  }

  /**
   * Bumped by every download that starts and by every place change. A download only lands if it's
   * still the latest one and its place is still the chosen one: otherwise changing place while a
   * refresh was in flight showed (and cached) the old place's forecast under the new name.
   */
  let generation = 0;

  async function refresh(force = false) {
    void nameIfNeeded();
    const p = place.value;
    if (!p || p === "off") return;
    // A routine refresh waits for the one in flight; a forced one (a new place) supersedes it.
    if (loading.value && !force) return;
    if (!force && forecast.value && !isStale(forecast.value.fetchedAt, Date.now())) return;
    const mine = ++generation;
    loading.value = true;
    try {
      const f = await fetchForecast(p);
      if (mine !== generation || place.value !== p) return;
      forecast.value = f;
      error.value = null;
      writeCache(p, f);
    } catch (e) {
      // Keep showing the last forecast if there is one; the card says how old it is.
      if (mine === generation) error.value = errorMessage(e);
    } finally {
      if (mine === generation) loading.value = false;
    }
  }

  async function setPlace(p: Place) {
    // Whatever was downloading was for the old place: let it finish, but never land.
    generation++;
    loading.value = false;
    place.value = p;
    forecast.value = null;
    error.value = null;
    await api.setSetting("ui.weather", JSON.stringify(p)).catch(() => undefined);
    await refresh(true);
  }

  /** This PC's place, named when the lookup answers ("Portland, Oregon"). Throws if it can't. */
  async function findMyPlace(): Promise<Place> {
    const pos = await api.locate();
    const name = await nameFor(pos.latitude, pos.longitude).catch(() => null);
    return { name: name ?? UNNAMED, ...pos };
  }

  async function useMyLocation(): Promise<string | null> {
    try {
      const pos = await api.locate();
      const name = await nameFor(pos.latitude, pos.longitude).catch(() => null);
      await setPlace({ name: name ?? UNNAMED, ...pos });
      return null;
    } catch (e) {
      return errorMessage(e);
    }
  }

  function hide() {
    place.value = "off";
    void api.setSetting("ui.weather", "off").catch(() => undefined);
  }

  /** Back to the set-up card (from Settings, or "Change place"). */
  function reset() {
    place.value = null;
    void api.setSetting("ui.weather", "").catch(() => undefined);
  }

  function toggleUnit() {
    unit.value = unit.value === "f" ? "c" : "f";
    void api.setSetting("ui.weatherUnit", unit.value).catch(() => undefined);
  }

  function dispose() {
    window.clearInterval(timer);
    timer = undefined;
    document.removeEventListener("visibilitychange", onShow);
    window.removeEventListener("focus", onShow);
  }

  return { place, unit, forecast, loading, error, ready, init, refresh, setPlace, findMyPlace, useMyLocation, hide, reset, toggleUnit, dispose };
});
