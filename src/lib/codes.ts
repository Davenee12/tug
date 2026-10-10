// One-time codes: spot the verification code in a text or notification so it can be
// copied in one click. Anchored on wording ("code", "verification", "OTP", …) so phone
// numbers, prices, times and order numbers aren't mistaken for codes.

/**
 * Words that mean "this message carries a code", in English and the iPhone's other major
 * languages. Matched as whole words with Unicode-aware edges (JavaScript's `\b` only knows ASCII,
 * so "código" or "код" would never match). Mirrors `CUE` in `src-tauri/src/codes.rs`.
 */
const CUE_WORDS = [
  // en
  "code", "codes", "passcode", "pass code", "verification", "verify", "otp", "one[- ]time", "2fa", "two[- ]factor",
  "security", "login", "log in", "sign[- ]in", "pin", "authenticat\\p{L}*", "confirm\\p{L}*",
  // es, pt
  "código", "codigo", "verificación", "verificação", "clave", "senha",
  // fr
  "vérification", "vérifier",
  // de
  "bestätigungscode", "sicherheitscode", "anmeldecode", "verifizierungscode", "aktivierungscode", "freischaltcode",
  // it
  "codice", "verifica",
  // nl
  "verificatiecode", "beveiligingscode", "inlogcode", "bevestigingscode",
  // sv, no, da, pl, tr, ru
  "kod", "koden", "kode", "kodu", "engångskod", "doğrulama", "код", "кода",
];
const W = "[\\p{L}\\p{N}_]";
const CUE = new RegExp(`(?<!${W})(?:${CUE_WORDS.join("|")})(?!${W})`, "iu");
/** Scripts without spaces between words: matched anywhere (ja, zh, ko). */
const CUE_ANYWHERE = /コード|验证码|驗證碼|校验码|认证码|인증\s?번호|인증\s?코드/u;

/**
 * Candidate codes: 4–8 digits, optionally split once by a dash or space (482-913), or a
 * provider prefix like G-482913. Not part of a longer number, a price, a time or a phone
 * number.
 */
const CANDIDATE = /(?<![\w$£€#.:/-])(?:[A-Z]{1,3}-)?(\d{3,4}[- ]\d{3,4}|\d{4,8})(?![\w/]|[.:,-]\d)/g;

/** Phone-number shapes to rule out ("302-555-0173", "(302) 555-0173", "+1 302…"). */
const PHONE = /(\+?\d{1,2}[\s.-]?)?\(?\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}/g;

export interface FoundCode {
  /** What to paste: digits only (separators removed). */
  code: string;
  /** As written in the text, for highlighting. */
  shown: string;
}

/**
 * The one code in `text`, or null. Always needs a cue word: a bare number from a short code is as
 * likely an order number, a price or a time, and a wrong "Copy code" is worse than none.
 */
export function findCode(text: string | null | undefined): FoundCode | null {
  if (!text || !(CUE.test(text) || CUE_ANYWHERE.test(text))) return null;
  const phones = [...text.matchAll(PHONE)].map((m) => [m.index!, m.index! + m[0].length] as const);
  const inPhone = (i: number) => phones.some(([a, b]) => i >= a && i < b);
  const found: FoundCode[] = [];
  for (const m of text.matchAll(CANDIDATE)) {
    if (inPhone(m.index!)) continue;
    const digits = m[1].replace(/[- ]/g, "");
    if (digits.length < 4 || digits.length > 8) continue;
    // A bare year in prose ("since 2019") isn't a code.
    if (digits.length === 4 && /^(19|20)\d\d$/.test(digits) && !/code|pin/i.test(text)) continue;
    found.push({ code: digits, shown: m[0] });
  }
  // Several numbers and nothing to tell them apart: don't guess.
  const distinct = new Set(found.map((f) => f.code));
  return distinct.size === 1 ? found[0] : null;
}
