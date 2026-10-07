// Apple model identifier ("iPhone16,2", read from the phone's Bluetooth Device Information
// Service) → the name people know it by and the shape tug draws for it (PhoneArt.vue).
//
// Identifiers verified against the community list maintained at
// https://gist.github.com/adamawolf/3048717 (main file, revised 2026-10-06; its comments-only
// iPhone19,x entries are unconfirmed and deliberately left out) and cross-checked with
// https://everymac.com/devices/by-identifier/ for iPhone Air (iPhone18,4) and iPhone 17e
// (iPhone18,5). Face/size per model follow Apple's published designs: 16e and 17e kept the notch.

/** What's on the front: a Touch ID home button, the notch, or the Dynamic Island. */
export type PhoneFace = "homeButton" | "notch" | "island";
/** Body size class: mini (5.4"), regular (4.7" home-button, 6.1"/6.3"), large (Plus, Max, Air). */
export type PhoneSize = "mini" | "regular" | "large";

export interface PhoneModel {
  /** Marketing name ("iPhone 15 Pro Max"), or just "iPhone" when tug doesn't know the model. */
  name: string;
  face: PhoneFace;
  size: PhoneSize;
  /** Whether the identifier was in the table. Unknown ones get the newest generic design. */
  known: boolean;
}

type Entry = readonly [name: string, face: PhoneFace, size: PhoneSize];

const MODELS: Readonly<Record<string, Entry>> = {
  // 2017
  "iPhone10,1": ["iPhone 8", "homeButton", "regular"],
  "iPhone10,4": ["iPhone 8", "homeButton", "regular"],
  "iPhone10,2": ["iPhone 8 Plus", "homeButton", "large"],
  "iPhone10,5": ["iPhone 8 Plus", "homeButton", "large"],
  "iPhone10,3": ["iPhone X", "notch", "regular"],
  "iPhone10,6": ["iPhone X", "notch", "regular"],
  // 2018
  "iPhone11,2": ["iPhone XS", "notch", "regular"],
  "iPhone11,4": ["iPhone XS Max", "notch", "large"],
  "iPhone11,6": ["iPhone XS Max", "notch", "large"],
  "iPhone11,8": ["iPhone XR", "notch", "regular"],
  // 2019–2020
  "iPhone12,1": ["iPhone 11", "notch", "regular"],
  "iPhone12,3": ["iPhone 11 Pro", "notch", "regular"],
  "iPhone12,5": ["iPhone 11 Pro Max", "notch", "large"],
  "iPhone12,8": ["iPhone SE", "homeButton", "regular"],
  "iPhone13,1": ["iPhone 12 mini", "notch", "mini"],
  "iPhone13,2": ["iPhone 12", "notch", "regular"],
  "iPhone13,3": ["iPhone 12 Pro", "notch", "regular"],
  "iPhone13,4": ["iPhone 12 Pro Max", "notch", "large"],
  // 2021–2022
  "iPhone14,4": ["iPhone 13 mini", "notch", "mini"],
  "iPhone14,5": ["iPhone 13", "notch", "regular"],
  "iPhone14,2": ["iPhone 13 Pro", "notch", "regular"],
  "iPhone14,3": ["iPhone 13 Pro Max", "notch", "large"],
  "iPhone14,6": ["iPhone SE", "homeButton", "regular"],
  "iPhone14,7": ["iPhone 14", "notch", "regular"],
  "iPhone14,8": ["iPhone 14 Plus", "notch", "large"],
  "iPhone15,2": ["iPhone 14 Pro", "island", "regular"],
  "iPhone15,3": ["iPhone 14 Pro Max", "island", "large"],
  // 2023
  "iPhone15,4": ["iPhone 15", "island", "regular"],
  "iPhone15,5": ["iPhone 15 Plus", "island", "large"],
  "iPhone16,1": ["iPhone 15 Pro", "island", "regular"],
  "iPhone16,2": ["iPhone 15 Pro Max", "island", "large"],
  // 2024–2025
  "iPhone17,3": ["iPhone 16", "island", "regular"],
  "iPhone17,4": ["iPhone 16 Plus", "island", "large"],
  "iPhone17,1": ["iPhone 16 Pro", "island", "regular"],
  "iPhone17,2": ["iPhone 16 Pro Max", "island", "large"],
  "iPhone17,5": ["iPhone 16e", "notch", "regular"],
  // 2025–2026
  "iPhone18,3": ["iPhone 17", "island", "regular"],
  "iPhone18,1": ["iPhone 17 Pro", "island", "regular"],
  "iPhone18,2": ["iPhone 17 Pro Max", "island", "large"],
  "iPhone18,4": ["iPhone Air", "island", "large"],
  "iPhone18,5": ["iPhone 17e", "notch", "regular"],
};

/** The newest generic look, for a phone that hasn't said what it is or that tug doesn't know. */
const GENERIC: PhoneModel = { name: "iPhone", face: "island", size: "regular", known: false };

/** Name and design for a model identifier. Anything unknown (or missing) is a generic iPhone. */
export function phoneModel(identifier: string | null | undefined): PhoneModel {
  const entry = identifier ? MODELS[identifier.trim()] : undefined;
  if (!entry) return GENERIC;
  const [name, face, size] = entry;
  return { name, face, size, known: true };
}

/** Every identifier tug knows, for tests. */
export const knownIdentifiers = (): string[] => Object.keys(MODELS);
