// Photos and other attachments in texts.
//
// The iPhone never sends the attachment itself over Bluetooth. For a photo it sends words instead:
// the text (over MAP) and its notification (over ANCS) both read "Attachment: 1 Photo", or
// "Attachment: 2 Photos", "Attachment: 1 Sticker"…, on a line of its own when the photo came with
// a caption. tug turns that line into a media chip ("Photo · Open it on your iPhone") rather than
// show it raw. Only the English wording is known; anything else is shown exactly as it came (the
// backend logs the shape of a likely foreign-language line, never its words, so more can be added).

export type MediaKind = "photo" | "video" | "sticker" | "audio" | "gif" | "file" | "contact" | "location" | "attachment";

/** One or more attachments in a text. `count` is null when the phone didn't say what or how many. */
export interface Media {
  kind: MediaKind;
  count: number | null;
}

/** A text split into what the person wrote and what they attached. */
export interface ParsedText {
  /** The caption, or the whole text when nothing was attached ("" for a photo on its own). */
  text: string;
  media: Media | null;
}

// The words iOS (and a generous reading of it) uses after the count, lower-cased.
const KINDS: Record<string, MediaKind> = {
  photo: "photo",
  photos: "photo",
  image: "photo",
  images: "photo",
  picture: "photo",
  pictures: "photo",
  pic: "photo",
  pics: "photo",
  video: "video",
  videos: "video",
  movie: "video",
  movies: "video",
  sticker: "sticker",
  stickers: "sticker",
  memoji: "sticker",
  audio: "audio",
  "audio message": "audio",
  "audio messages": "audio",
  "audio file": "audio",
  "audio files": "audio",
  "voice message": "audio",
  "voice messages": "audio",
  "voice memo": "audio",
  "voice memos": "audio",
  gif: "gif",
  gifs: "gif",
  file: "file",
  files: "file",
  document: "file",
  documents: "file",
  pdf: "file",
  pdfs: "file",
  contact: "contact",
  contacts: "contact",
  "contact card": "contact",
  "contact cards": "contact",
  vcard: "contact",
  vcards: "contact",
  location: "location",
  locations: "location",
  attachment: "attachment",
  attachments: "attachment",
};

// "Attachment: 1 Photo" filling a whole line. The kind is a word or two; an unknown one-word kind
// counts as a plain attachment.
const WHOLE_LINE = /^attachments?\s*:\s*(\d{1,3})\s+(\p{L}+(?:\s+\p{L}+)?)$/iu;
// The same phrase sharing a line with a caption, before or after it. Only known kinds here, so
// ordinary words after a count ("Attachment: 2 are below") stay text.
const AT_END = /^(.*\S)\s+attachments?\s*:\s*(\d{1,3})\s+(\p{L}+(?:\s+\p{L}+)?)$/iu;
const AT_START = /^attachments?\s*:\s*(\d{1,3})\s+(\p{L}+(?:\s+\p{L}+)?)\s+(\S.*)$/iu;

const OBJECT_REPLACEMENT = "￼";

/** The known kind for a word ("Photos" → photo), or null. */
function knownKind(word: string): MediaKind | null {
  return KINDS[word.toLowerCase().replace(/\s+/g, " ")] ?? null;
}

/** A start-of-line match whose kind may be one word ("Photo Look") or two ("Audio message hi"). */
function splitStart(line: string): { count: number; kind: MediaKind; rest: string } | null {
  const m = AT_START.exec(line);
  if (!m) return null;
  const words = m[2].split(/\s+/);
  // Prefer the two-word kind, else the first word with the second given back to the caption.
  const two = knownKind(m[2]);
  if (two) return { count: Number(m[1]), kind: two, rest: m[3] };
  const one = knownKind(words[0]);
  if (one) return { count: Number(m[1]), kind: one, rest: [...words.slice(1), m[3]].join(" ") };
  return null;
}

/**
 * Split a text into its caption and its attachments. "Attachment: 1 Photo" (any case, singular or
 * plural, on its own line or sharing one with a caption) becomes media; several such lines add up
 * (mixed kinds become "attachments"). U+FFFC, which marks where an attachment sat in an iMessage,
 * counts as one attachment of unknown kind. Anything else is returned unchanged.
 */
export function parseAttachments(body: string): ParsedText {
  const found: Array<{ count: number; kind: MediaKind }> = [];
  const kept: string[] = [];
  for (const raw of body.split(/\r?\n/)) {
    let line = raw.trim();
    if (line.includes(OBJECT_REPLACEMENT)) {
      found.push({ count: 0, kind: "attachment" });
      line = line.replaceAll(OBJECT_REPLACEMENT, " ").replace(/\s+/g, " ").trim();
      if (!line) continue;
    }
    const whole = WHOLE_LINE.exec(line);
    // An unknown kind only as one word ("Attachment: 1 Hologram"): two unknown words read as a
    // sentence ("Attachment: 2 are below"), so they stay text.
    const wholeKind = whole ? (knownKind(whole[2]) ?? (/\s/.test(whole[2]) ? null : "attachment")) : null;
    if (whole && wholeKind) {
      found.push({ count: Number(whole[1]), kind: wholeKind });
      continue;
    }
    const end = AT_END.exec(line);
    const endKind = end ? knownKind(end[3]) : null;
    if (end && endKind) {
      found.push({ count: Number(end[2]), kind: endKind });
      kept.push(end[1]);
      continue;
    }
    const start = splitStart(line);
    if (start) {
      found.push({ count: start.count, kind: start.kind });
      kept.push(start.rest);
      continue;
    }
    // As it came, unless a U+FFFC was taken out of it.
    kept.push(line === raw.trim() ? raw : line);
  }
  if (found.length === 0) return { text: body, media: null };
  const kinds = new Set(found.map((f) => f.kind));
  const kind = kinds.size === 1 ? found[0].kind : "attachment";
  // A U+FFFC says "something", not how many; real counts win, and only placeholders mean unknown.
  const counted = found.filter((f) => f.count > 0);
  const count = counted.length ? counted.reduce((n, f) => n + f.count, 0) : null;
  return { text: kept.join("\n").trim(), media: { kind, count } };
}

const SINGULAR: Record<MediaKind, string> = {
  photo: "Photo",
  video: "Video",
  sticker: "Sticker",
  audio: "Audio message",
  gif: "GIF",
  file: "File",
  contact: "Contact",
  location: "Location",
  attachment: "Attachment",
};
const PLURAL: Record<MediaKind, string> = {
  photo: "photos",
  video: "videos",
  sticker: "stickers",
  audio: "audio messages",
  gif: "GIFs",
  file: "files",
  contact: "contacts",
  location: "locations",
  attachment: "attachments",
};

/** "Photo", "2 photos", "Sticker", "3 attachments"; "Photo or attachment" when the phone didn't say. */
export function mediaLabel(m: Media): string {
  if (m.count === null) return m.kind === "attachment" ? "Photo or attachment" : SINGULAR[m.kind];
  return m.count > 1 ? `${m.count} ${PLURAL[m.kind]}` : SINGULAR[m.kind];
}

const EMOJI: Record<MediaKind, string> = {
  photo: "📷",
  video: "🎥",
  sticker: "😀",
  audio: "🎤",
  gif: "🖼️",
  file: "📄",
  contact: "👤",
  location: "📍",
  attachment: "📎",
};

/** A small picture for one-line previews and pop-ups (the app itself draws an icon). */
export function mediaEmoji(kind: MediaKind): string {
  return EMOJI[kind];
}

/** Where the photo really is. Shown under the chip and in pop-ups. */
export const OPEN_ON_IPHONE = "Open it on your iPhone";

/**
 * A body with no characters left in it: only U+FFFD (what's left of characters that didn't come
 * through). The iPhone sends exactly that for some texts; the words may still be in the text's
 * notification.
 */
export function isUnreadable(body: string): boolean {
  const t = body.trim();
  return t.length > 0 && /^[�\s]+$/u.test(t);
}

/** Said in place of a text whose words didn't come through. */
export const UNREADABLE = "This text didn't come through";

/** One line for a conversation list or a Feed row: "📷 Photo", or "📷 " and the caption. */
export function previewLine(p: ParsedText): string {
  if (!p.media) return p.text;
  return `${mediaEmoji(p.media.kind)} ${p.text.replace(/\s*\n\s*/g, " ") || mediaLabel(p.media)}`;
}

/**
 * A pop-up's body with any attachment line said in words: "📷 Photo" and "Open it on your iPhone"
 * on the next line, after the caption if there is one.
 */
export function toastBody(body: string): string {
  if (isUnreadable(body)) return `${UNREADABLE}. ${OPEN_ON_IPHONE}.`;
  const p = parseAttachments(body);
  if (!p.media) return body;
  const chip = `${mediaEmoji(p.media.kind)} ${mediaLabel(p.media)}`;
  return p.text ? `${p.text}\n${chip} · ${OPEN_ON_IPHONE}` : `${chip}\n${OPEN_ON_IPHONE}`;
}
