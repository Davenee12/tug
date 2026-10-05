import { defineStore } from "pinia";
import { ref } from "vue";
import { api, errorMessage } from "../lib/ipc";
import { defaultUnit, fetchForecast, nameFor, type Forecast, type Place, type Unit } from "../lib/weather";

const STALE_MS = 30 * 60 * 1000;
const CACHE_KEY = "tug.weather.v1";
/** Shown only until the place's real name comes back ("Orlando, Florida"). */
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
   * the card from the cache for nothing. Later calls only restart the refresh clock.
   */
  async function init() {
    if (ready.value) {
      void refresh();
      window.clearInterval(timer);
      timer = window.setInterval(() => void refresh(), STALE_MS);
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
    window.clearInterval(timer);
    timer = window.setInterval(() => void refresh(), STALE_MS);
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

  async function refresh(force = false) {
    void nameIfNeeded();
    const p = place.value;
    if (!p || p === "off" || loading.value) return;
    if (!force && forecast.value && Date.now() - forecast.value.fetchedAt < STALE_MS) return;
    loading.value = true;
    try {
      const f = await fetchForecast(p);
      forecast.value = f;
      error.value = null;
      writeCache(p, f);
    } catch (e) {
      // Keep showing the last forecast if there is one; the card says how old it is.
      error.value = errorMessage(e);
    } finally {
      loading.value = false;
    }
  }

  async function setPlace(p: Place) {
    place.value = p;
    forecast.value = null;
    error.value = null;
    await api.setSetting("ui.weather", JSON.stringify(p)).catch(() => undefined);
    await refresh(true);
  }

  /** This PC's place, named when the lookup answers ("Orlando, Florida"). Throws if it can't. */
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
  }

  return { place, unit, forecast, loading, error, ready, init, refresh, setPlace, findMyPlace, useMyLocation, hide, reset, toggleUnit, dispose };
});
